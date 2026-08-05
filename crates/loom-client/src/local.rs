//! Local client session foundations.
//!
//! `LocalLoomClient` is bound to one local `.loom` path and owns a session registry over real
//! `Loom<FileStore>` opens. This module provides the open, save, and close foundations that the
//! generated `LoomApi` trait impls build on; per-facet method groups are layered on
//! top of `with_session`.
//!
//! Licensed under BUSL-1.1.

use crate::identity_authority_policy::IdentityAuthorityPolicyService;
use crate::locks::{InProcessLocksAuthority, LocksAuthority};
use crate::security_admin::SecurityAdminService;
use crate::types::{HandleId, LoomSession, RowIter, SqlBatch, SqlSession, Task};
use loom_compute::{
    ExecError, Manifest, ProgramBody, StoredProgram, apply, execute_cbor, program_get,
    program_inspect, program_list, program_put, program_remove,
};
use loom_core::acl::{
    AclGrant, AclResource, AclResourceScope, AclRight, AclScopeKind, AclStore, AclSubject,
};
use loom_core::digest::{Algo, Digest};
use loom_core::document::Collection;
use loom_core::identity::{
    ExternalCredentialSpec, IdentityPublicKeySpec, IdentityStore, Principal, PrincipalId,
    PrincipalKind, RoleId,
};
use loom_core::keys::{EncryptionMeta, KEY_LEN, KeySpec, Suite};
use loom_core::lock::{LockMode, LockOwner, LockToken};
use loom_core::tabular::{CmpOp, ColumnType, Value};
use loom_core::{
    AcceleratorPolicy, AclDomain, AtomicityBoundary, Bundle, ColumnarAggregate, ColumnarInspect,
    CommitReceipt, DataframePlan, DocumentBinary, DocumentText, Edge, EmbeddingModel, FacetKind,
    FileKind, FileStat, GraphQuery, GraphQueryExplain, GraphQueryResult, Hit, KvMapConfig, Loom,
    MetaFilter, Metric, Object, ObjectStore, OpenMode, OverlayDurabilityPolicy, Props,
    ProtectedRefPolicy, RuntimeProfile, Series, TriggerId, VectorCondition, VectorEntry,
    WatchBatch, WatchCursor, WatchSelector, WorkflowAuditWrite, WorkflowControlWrite,
    WorkflowReferenceUpdate, WorkflowTransaction, WorkspaceId, WorkspaceInfo, WsSelector,
    bundle_import, cas_delete, cas_get, cas_has, cas_list, cas_put, columnar_aggregate,
    columnar_append, columnar_columns, columnar_compact, columnar_create, columnar_from_arrow_ipc,
    columnar_from_parquet, columnar_inspect, columnar_rows, columnar_scan, columnar_select,
    columnar_source_digest, dataframe_collect, dataframe_create, dataframe_materialize,
    dataframe_plan_digest, dataframe_preview, dataframe_source_digests, doc_delete,
    doc_delete_collection, doc_list_collections, document_get_binary, document_get_text,
    document_list_binary, fire_record_to_cbor, get_columnar, graph_explain_query, graph_get_edge,
    graph_get_node, graph_in_edges, graph_neighbors, graph_out_edges, graph_query, graph_reachable,
    graph_remove_edge, graph_remove_node, graph_shortest_path, graph_upsert_edge,
    graph_upsert_node, inference_instance_state, ledger_append, ledger_get, ledger_head,
    ledger_len, ledger_list_collections, ledger_verify, put_columnar, put_inference_instance_state,
    trigger_binding_from_cbor, trigger_binding_to_cbor, ts_get, ts_latest, ts_list_collections,
    ts_put, ts_range, vector_create, vector_create_metadata_index, vector_delete,
    vector_drop_metadata_index, vector_embedding_model, vector_exact_token, vector_get, vector_ids,
    vector_metadata_index_keys, vector_search, vector_search_with_pq_policy, vector_source_text,
    vector_upsert, vector_upsert_with_source, vector_upsert_with_source_conditioned,
};
use loom_core::{
    ConflictResolution, MergeOutcome, ReplayOutcome, Status, calendar, contacts, kv, log, mail,
    search,
};
use loom_core::{
    LogQuery, LogQueryResult, LogRecord, MetricDescriptor, MetricObservation, MetricQuery,
    MetricQueryResult, SpanRecord, TraceQuery, TraceQueryResult, logs_get_record, logs_put_record,
    logs_query, metrics_get_descriptor, metrics_put_descriptor, metrics_put_observation,
    metrics_query_observations, traces_get_span, traces_put_span, traces_query, traces_trace_spans,
};
use loom_lanes::{Lane, LaneStatusTextWarning, LaneTicketView, LaneView};
use loom_sql::LoomSqlStore;
use loom_store::{
    FileStore, LocalOpenAuth, StoreMaintenanceClock, StorePolicy, SystemStoreMaintenanceClock,
    attach_local_auth, open_loom, open_loom_daemon_authorized_unlocked, open_loom_read,
    open_loom_read_unlocked, open_loom_registry_read_unlocked, open_loom_unlocked,
    open_store_metadata_checked, run_store_maintenance_once_with_clock, save_loom,
};
use loom_substrate::OperationEnvelope;
use loom_substrate::drive::{
    DriveOperationLog, DrivePolicyRegistry, DrivePolicyTarget, drive_operation_log_key,
    drive_policy_registry_key,
};
use loom_substrate::lifecycle::{LifecycleOperationLog, lifecycle_operation_log_key};
use loom_substrate::meetings::{
    MeetingsProfileSnapshot, ProjectionAction, ProjectionKind, ProjectionOutput,
    ProjectionOutputSet, meetings_profile_key,
};
use loom_substrate::refs::{EntityRef, UnresolvedReference};
use loom_substrate::search::{
    EMBEDDING_PROJECTION_JOBS_DIR, EmbeddingProjectionJob, EmbeddingProjectionKey,
    EmbeddingProjectionStamp,
};
use loom_substrate::versioning::{
    BodyRef, RevisionBackfillUpdate, RevisionIndex, load_optional_current_revision_index,
    persist_current_revision_index,
};
use loom_types::{Code, InferenceModelKind, LoomError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

static NEXT_LOCK_SESSION_NAMESPACE: AtomicU64 = AtomicU64::new(1);

#[derive(Deserialize)]
struct LifecycleGateEvaluationJson {
    gate_id: String,
    passed: bool,
    principal_id: Option<String>,
    evidence_digest: Option<String>,
    evaluated_at_ms: Option<u64>,
}

fn json_string<T: Serialize>(value: &T) -> Result<String, LoomError> {
    serde_json::to_string(value)
        .map_err(|err| LoomError::new(Code::InvalidArgument, err.to_string()))
}

fn parse_lifecycle_gate_evaluations(
    input: &str,
    now_ms: u64,
) -> Result<Vec<loom_lifecycle::LifecycleGateEvaluationInput>, LoomError> {
    let values: Vec<LifecycleGateEvaluationJson> = serde_json::from_str(input)
        .map_err(|err| LoomError::new(Code::InvalidArgument, err.to_string()))?;
    Ok(values
        .into_iter()
        .map(|value| loom_lifecycle::LifecycleGateEvaluationInput {
            gate_id: value.gate_id,
            passed: value.passed,
            principal_id: value.principal_id,
            evidence_digest: value.evidence_digest,
            evaluated_at_ms: value.evaluated_at_ms.unwrap_or(now_ms),
        })
        .collect())
}

fn reconcile_ticket_references(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    now_ms: u64,
    max: usize,
) -> Result<loom_reference::ReconciliationSummary, LoomError> {
    let principal = loom.effective_principal()?.unwrap_or(ns).to_string();
    let mut summary = loom_reference::status(loom, ns)?;
    let mut index = loom_reference::load_or_rebuild_index(loom, ns)?;
    let records = loom_tickets::reconcile_reference_candidates(
        loom,
        ns,
        workspace_id,
        now_ms,
        max,
        &principal,
    )?;
    loom_reference::apply_resolved_edges(&mut index, &records)?;
    loom_reference::save_index(loom, ns, &index)?;
    summary.processed = records.len() as u64;
    let current = loom_reference::status(loom, ns)?;
    summary.pending = current.pending;
    summary.resolved = current.resolved;
    summary.failed = current.failed;
    Ok(summary)
}

fn reconcile_chat_references(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    now_ms: u64,
    max: usize,
) -> Result<loom_reference::ReconciliationSummary, LoomError> {
    let principal = loom.effective_principal()?.unwrap_or(ns).to_string();
    let mut index = loom_reference::load_or_rebuild_index(loom, ns)?;
    let mut processed = 0u64;
    for target in loom_reference::targets(loom, ns)? {
        if target.source_profile != "chat"
            || !target.source_scope.starts_with(&format!("{workspace_id}:"))
        {
            continue;
        }
        let remaining = max.saturating_sub(processed as usize);
        if remaining == 0 {
            break;
        }
        let records = loom_reference::reconcile(
            loom,
            ns,
            &target,
            now_ms,
            remaining,
            &principal,
            |loom, candidate| resolve_chat_candidate(loom, ns, &target.source_scope, candidate),
        )?;
        processed = processed.saturating_add(records.len() as u64);
        loom_reference::apply_resolved_edges(&mut index, &records)?;
    }
    loom_reference::save_index(loom, ns, &index)?;
    let mut summary = loom_reference::status(loom, ns)?;
    summary.processed = processed;
    Ok(summary)
}

fn resolve_chat_candidate(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    source_scope: &str,
    candidate: &UnresolvedReference,
) -> Result<Option<EntityRef>, LoomError> {
    let (workspace_id, _) = source_scope
        .rsplit_once(':')
        .ok_or_else(|| LoomError::corrupt("chat reference scope is invalid"))?;
    if let Some(handle) = candidate.alias_text.strip_prefix('@') {
        return loom
            .identity_store()
            .map(|identity| identity.resolve_handle(handle))
            .transpose()?
            .flatten()
            .map(|principal| EntityRef::parse(&format!("principal:{principal}")))
            .transpose();
    }
    if let Some(handle) = candidate.alias_text.strip_prefix('#') {
        return loom_chat::resolve_channel_id(loom, ns, workspace_id, handle)
            .ok()
            .map(|channel| EntityRef::parse(&format!("channel:{channel}")))
            .transpose();
    }
    if let Some(key) = candidate.alias_text.strip_prefix("!ticket:") {
        return resolve_ticket_candidate(loom, ns, workspace_id, key);
    }
    if let Some(target) = candidate.alias_text.strip_prefix('!') {
        return EntityRef::parse(target).map(Some);
    }
    Ok(None)
}

fn resolve_ticket_candidate(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace_id: &str,
    key: &str,
) -> Result<Option<EntityRef>, LoomError> {
    let Some(profile) = loom_tickets::TicketProfileReader::open(loom, ns, workspace_id)? else {
        return Ok(None);
    };
    profile
        .resolve_ticket_key(key)?
        .map(|resolution| EntityRef::parse(&format!("ticket:{}", resolution.ticket_id)))
        .transpose()
}

fn parse_table_csv_import_mode(
    mode: &str,
) -> Result<loom_interchange_io::TableImportMode, LoomError> {
    match mode {
        "snapshot" => Ok(loom_interchange_io::TableImportMode::Snapshot),
        "append-only" => Ok(loom_interchange_io::TableImportMode::AppendOnly),
        other => Err(LoomError::new(
            Code::InvalidArgument,
            format!(
                "unsupported table CSV import mode {other:?}; expected snapshot or append-only"
            ),
        )),
    }
}

fn parse_table_csv_primary_key(value: &str) -> Result<Vec<String>, LoomError> {
    let columns = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if columns.is_empty() {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "table CSV primary key is empty",
        ));
    }
    Ok(columns)
}

fn parse_table_csv_schema(value: &str) -> Result<Vec<(String, ColumnType)>, LoomError> {
    let mut columns = Vec::new();
    for item in value.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let (name, ty) = item.split_once(':').ok_or_else(|| {
            LoomError::new(
                Code::InvalidArgument,
                format!("table CSV schema item {item:?} is missing ':'"),
            )
        })?;
        let name = name.trim();
        if name.is_empty() {
            return Err(LoomError::new(
                Code::InvalidArgument,
                format!("table CSV schema item {item:?} has an empty name"),
            ));
        }
        columns.push((name.to_string(), parse_table_csv_column_type(ty.trim())?));
    }
    if columns.is_empty() {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "table CSV schema is empty",
        ));
    }
    Ok(columns)
}

fn parse_table_csv_column_type(value: &str) -> Result<ColumnType, LoomError> {
    match value {
        "int" | "integer" => Ok(ColumnType::Int),
        "float" | "double" => Ok(ColumnType::Float),
        "text" | "string" => Ok(ColumnType::Text),
        "bool" | "boolean" => Ok(ColumnType::Bool),
        "i8" => Ok(ColumnType::I8),
        "i16" => Ok(ColumnType::I16),
        "i32" => Ok(ColumnType::I32),
        "i128" => Ok(ColumnType::I128),
        "u8" => Ok(ColumnType::U8),
        "u16" => Ok(ColumnType::U16),
        "u32" => Ok(ColumnType::U32),
        "u64" => Ok(ColumnType::U64),
        "u128" => Ok(ColumnType::U128),
        "f32" => Ok(ColumnType::F32),
        "decimal" | "numeric" => Ok(ColumnType::Decimal),
        "date" => Ok(ColumnType::Date),
        "time" => Ok(ColumnType::Time),
        "timestamp" => Ok(ColumnType::Timestamp),
        "uuid" => Ok(ColumnType::Uuid),
        other => Err(LoomError::new(
            Code::InvalidArgument,
            format!(
                "unsupported table CSV column type {other:?}; expected int, float, text, bool, decimal, date, time, timestamp, uuid, or sized integer/float aliases"
            ),
        )),
    }
}

fn parse_ticket_import_field_policy(
    field_policy: &str,
) -> Result<loom_interchange_io::TicketImportFieldPolicy, LoomError> {
    loom_interchange_io::TicketImportFieldPolicy::parse(field_policy)
}

fn retained_import_input_from_bytes<S: loom_core::ObjectStore>(
    loom: &Loom<S>,
    source_scope: &str,
    snapshot_payload: &[u8],
) -> loom_interchange_io::ResolvedImportInput {
    loom_interchange_io::ResolvedImportInput {
        source_scope: source_scope.to_string(),
        kind: loom_interchange_io::ImportInputKind::File,
        source_digest: Digest::hash(loom.store().digest_algo(), snapshot_payload),
        size_bytes: snapshot_payload.len() as u64,
        item_count: 1,
        path: PathBuf::from(source_scope),
        bytes: Some(snapshot_payload.to_vec()),
    }
}

fn persist_profile_import_artifacts(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    profile: &str,
    input: &loom_interchange_io::ResolvedImportInput,
    report: &loom_interchange::ImportReport,
) -> Result<bool, LoomError> {
    if report.dry_run {
        return Ok(false);
    }
    let retained = loom_interchange_io::retain_import_input(loom, ns, profile, input, None)?;
    let checkpoint_id = format!("{}:{}", profile, input.source_digest);
    let mut checkpoint = input.checkpoint(profile, &checkpoint_id)?;
    checkpoint.profile_state_digest = Some(retained.manifest_digest);
    loom_interchange_io::persist_import_checkpoint(loom, ns, &checkpoint, None)?;
    Ok(true)
}

fn update_lane_metadata(lane: &mut Lane, updated_by: &str) {
    lane.updated_at = now_ms();
    lane.updated_by = updated_by.to_string();
}

const DERIVE_LANE_ACTOR: &str = "system:derive-lane-actor";

fn lane_actor_for_update(
    loom: &Loom<FileStore>,
    workspace_id: WorkspaceId,
    updated_by: &str,
) -> Result<String, LoomError> {
    if !updated_by.trim().is_empty() && updated_by != DERIVE_LANE_ACTOR {
        return Ok(updated_by.to_string());
    }
    Ok(loom
        .effective_principal()?
        .map(|principal| principal.to_string())
        .unwrap_or_else(|| workspace_id.to_string()))
}

fn service_workspace_selector(workspace: &str) -> WsSelector {
    match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Name(workspace.to_string()),
    }
}

pub struct LaneUpdateInput<'a> {
    pub lane_id: &'a str,
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub lane_status: Option<&'a str>,
    pub status_report: Option<&'a str>,
    pub reviewer_feedback: Option<&'a str>,
    pub updated_by: &'a str,
}

pub fn apply_pages_update_text(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    page_workspace_id: &str,
    page_id: &str,
    body_text: &str,
    expected_root: Option<&str>,
) -> Result<loom_pages::PageUpdateSummary, LoomError> {
    let ns = loom
        .registry()
        .open(&service_workspace_selector(workspace))?;
    loom_pages::update_page_text(
        loom,
        ns,
        page_workspace_id,
        page_id,
        body_text,
        now_ms(),
        expected_root,
    )
}

pub fn apply_pages_publish(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    page_workspace_id: &str,
    page_id: &str,
    expected_root: Option<&str>,
) -> Result<loom_pages::PagePublishSummary, LoomError> {
    let ns = loom
        .registry()
        .open(&service_workspace_selector(workspace))?;
    loom_pages::publish_page(
        loom,
        ns,
        page_workspace_id,
        page_id,
        now_ms(),
        expected_root,
    )
}

pub fn apply_lanes_update(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    input: LaneUpdateInput<'_>,
) -> Result<Lane, LoomError> {
    if input.title.is_none()
        && input.description.is_none()
        && input.lane_status.is_none()
        && input.status_report.is_none()
        && input.reviewer_feedback.is_none()
    {
        return Err(LoomError::invalid(
            "lane update requires at least one field",
        ));
    }
    let ns = read_ns(loom, workspace, FacetKind::Document)?
        .ok_or_else(|| LoomError::new(Code::NotFound, "lane workspace not found"))?;
    let mut lane = loom_lanes::get_lane(loom, ns, input.lane_id)?
        .ok_or_else(|| LoomError::new(Code::NotFound, "lane not found"))?;
    let prior_lane = lane.clone();
    if let Some(title) = input.title {
        lane.title = title.to_string();
    }
    if let Some(description) = input.description {
        lane.description = description.to_string();
    }
    if let Some(lane_status) = input.lane_status {
        lane.lane_status = loom_lanes::LaneStatus::parse(lane_status)?
            .as_str()
            .to_string();
    }
    if let Some(status_report) = input.status_report {
        lane.status_report = status_report.to_string();
    }
    if let Some(reviewer_feedback) = input.reviewer_feedback {
        lane.reviewer_feedback = reviewer_feedback.to_string();
    }
    let actor = lane_actor_for_update(loom, ns, input.updated_by)?;
    update_lane_metadata(&mut lane, &actor);
    loom_lanes::put_lane_current_record(loom, ns, &lane, Some(&prior_lane))?;
    Ok(lane)
}

pub struct LaneCloseoutInput<'a> {
    pub lane_id: &'a str,
    pub ticket_workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub comment_type: &'a str,
    pub comment_body: &'a str,
    pub evidence: Option<loom_tickets::TicketCommentEvidence>,
    pub status_report: &'a str,
    pub updated_by: &'a str,
    pub expected_root: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LaneCleanupReport {
    pub lane_id: String,
    pub would_remove: Vec<String>,
    pub removed: Vec<String>,
    pub remaining_count: usize,
    pub status_counts: loom_lanes::LaneStatusCounts,
}

pub fn build_lane_view(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    ticket_workspace_id: &str,
    lane: &Lane,
) -> LaneView {
    let lane_tickets = lane
        .lane_tickets
        .iter()
        .map(|lane_ticket| {
            let ticket = loom_tickets::get_ticket(
                loom,
                workspace,
                ticket_workspace_id,
                &lane_ticket.ticket_id,
            )
            .ok()
            .flatten();
            LaneTicketView {
                ticket_id: lane_ticket.ticket_id.clone(),
                status: Some(
                    ticket
                        .as_ref()
                        .map(ticket_status)
                        .unwrap_or_else(|| "missing".to_string()),
                ),
                priority: ticket
                    .as_ref()
                    .and_then(|ticket| ticket_text_field(ticket, "priority")),
                title: ticket.as_ref().map(ticket_title),
            }
        })
        .collect();
    let mut view = loom_lanes::lane_view(lane, lane_tickets);
    view.owner_display = view
        .owner_principal
        .as_deref()
        .map(|id| loom_tickets::resolve_principal_display(loom.identity_store(), id));
    // If lane text describes a review/closeout handoff but the active ticket has no matching typed
    // comment, the durable record is missing: surface it as a status warning so agents record it on
    // the ticket (e.g. via the lanes_closeout helper) rather than leaving it only in lane text.
    if let Some(active) = lane.active_ticket_id.as_deref() {
        for (field, text) in [
            ("status_report", lane.status_report.as_str()),
            ("reviewer_feedback", lane.reviewer_feedback.as_str()),
        ] {
            let Some(event) = loom_lanes::lane_text_closeout_event(text) else {
                continue;
            };
            let has_matching_comment =
                loom_tickets::list_ticket_comments(loom, workspace, ticket_workspace_id, active)
                    .map(|comments| {
                        comments
                            .iter()
                            .any(|comment| closeout_comment_matches(event, &comment.comment_type))
                    })
                    .unwrap_or(false);
            if !has_matching_comment {
                view.status_warnings.push(LaneStatusTextWarning {
                    field: field.to_string(),
                    stated_status: event.to_string(),
                    ticket_id: Some(active.to_string()),
                    ticket_status: None,
                    message: format!(
                        "{field} describes a {event} handoff but ticket {active} has no matching comment; record it on the ticket (lanes_closeout)"
                    ),
                });
            }
        }
    }
    view
}

/// Whether a ticket comment of `comment_type` records the closeout `event` detected in lane text.
fn closeout_comment_matches(event: &str, comment_type: &str) -> bool {
    match event {
        "review_request" => comment_type == "review_request",
        "blocker" => comment_type == "blocker",
        "closeout_evidence" => matches!(comment_type, "closeout_evidence" | "acceptance_evidence"),
        "command_requested" => matches!(comment_type, "blocker" | "decision" | "progress"),
        "feedback_addressed" => {
            matches!(comment_type, "resolution" | "progress" | "review_feedback")
        }
        _ => false,
    }
}

fn cleanup_target_lanes(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    lane_id: Option<&str>,
) -> Result<Vec<Lane>, LoomError> {
    match lane_id {
        Some(lane_id) => {
            let lane = loom_lanes::get_lane(loom, ns, lane_id)?
                .ok_or_else(|| LoomError::new(Code::NotFound, "lane not found"))?;
            Ok(vec![lane])
        }
        None => Ok(loom_lanes::list_lanes(loom, ns)?
            .into_iter()
            .filter(|lane| lane.lane_kind == loom_lanes::LaneKind::Assignment.as_str())
            .collect()),
    }
}

fn cleanup_remaining_summary(
    view: &loom_lanes::LaneView,
    terminal_ids: &[String],
) -> (usize, loom_lanes::LaneStatusCounts) {
    let remaining = view
        .lane_tickets
        .iter()
        .filter(|ticket| !terminal_ids.iter().any(|id| id == &ticket.ticket_id))
        .cloned()
        .collect::<Vec<_>>();
    let counts = loom_lanes::lane_status_counts(&remaining);
    (remaining.len(), counts)
}

fn ticket_text_field(ticket: &loom_tickets::TicketSummary, field: &str) -> Option<String> {
    match ticket.fields.get(field)? {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Object(map) => map
            .get("String")
            .or_else(|| map.get("Text"))
            .or_else(|| map.get("EnumOption"))
            .or_else(|| map.get("Principal"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn ticket_status(ticket: &loom_tickets::TicketSummary) -> String {
    ticket_text_field(ticket, "status").unwrap_or_else(|| "unknown".to_string())
}

fn ticket_title(ticket: &loom_tickets::TicketSummary) -> String {
    ticket_text_field(ticket, "title")
        .or_else(|| ticket_text_field(ticket, "summary"))
        .unwrap_or_default()
}

fn enforce_document_entity_tag<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    collection: &str,
    id: &str,
    expected_entity_tag: Option<&str>,
) -> Result<(), LoomError> {
    let Some(expected_entity_tag) = expected_entity_tag else {
        return Ok(());
    };
    match loom_core::document::document_get_binary(loom, ns, collection, id)? {
        Some(current) if current.entity_tag == expected_entity_tag => Ok(()),
        Some(_) | None => Err(LoomError::new(
            Code::Conflict,
            loom_types::ConflictReason::ExpectedTagMismatch.as_str(),
        )),
    }
}

struct PlannedDocumentPut {
    result: Vec<u8>,
    state: Vec<u8>,
    overlay: loom_core::MutableOverlay,
    transaction: WorkflowTransaction,
}

fn plan_document_put(
    loom: &mut Loom<loom_core::provider::PlanningObjectStore<'_, FileStore>>,
    workspace: &str,
    collection: &str,
    id: &str,
    bytes: Vec<u8>,
    expected_entity_tag: Option<&str>,
) -> Result<PlannedDocumentPut, LoomError> {
    let ns = loom.registry_mut().ensure_for_write(
        &ns_selector(workspace, FacetKind::Document),
        mint_workspace_id()?,
    )?;
    enforce_document_entity_tag(loom, ns, collection, id, expected_entity_tag)?;
    let digest = Digest::hash(loom.store().digest_algo(), &bytes);
    loom_reference::put_document_indexed(loom, ns, collection, id, bytes)?;
    let result = loom_wire::document::put_result_to_cbor(
        &digest.to_string(),
        &loom_core::document_entity_tag_string_from_digest(digest),
    )?;
    let (root, state_objects) = loom.save_state_objects()?;
    let mut objects = loom.store().objects()?;
    objects.extend(state_objects);
    let state = loom.export_state();
    let overlay = loom.mutable_overlay().clone();
    let direct_controls = loom.store().direct_control_writes()?;
    if !direct_controls.is_empty() {
        return Err(LoomError::corrupt(
            "document put planning candidate left direct control writes",
        ));
    }
    let mut workflow_transactions = loom.store().workflow_transactions()?;
    if workflow_transactions.len() != 1 {
        return Err(LoomError::corrupt(
            "document put planning candidate must publish exactly one workflow transaction",
        ));
    }
    let mut transaction = workflow_transactions.remove(0);
    transaction.owner_state.objects.extend(objects);
    transaction.owner_state.reference = WorkflowReferenceUpdate::Set(Some(root));
    Ok(PlannedDocumentPut {
        result,
        state,
        overlay,
        transaction,
    })
}

fn synchronize_document_transaction_receipt<S: ObjectStore>(
    loom: &mut Loom<S>,
    receipt: &CommitReceipt,
) -> Result<(), LoomError> {
    for outcome in &receipt.writes {
        let current = loom
            .store()
            .mutable_overlay_current_entry(&outcome.target)?
            .ok_or_else(|| {
                LoomError::corrupt("document workflow transaction omitted current record")
            })?;
        loom.mutable_overlay_mut()
            .synchronize_current_entry(current)?;
    }
    Ok(())
}

fn publish_document_put_candidate(
    loom: &mut Loom<FileStore>,
    planned: PlannedDocumentPut,
    before_commit: impl FnOnce(&mut Loom<FileStore>) -> Result<(), LoomError>,
) -> Result<Vec<u8>, LoomError> {
    let result = planned.result.clone();
    before_commit(loom)?;
    let receipt = loom
        .store()
        .commit_workflow_transaction(planned.transaction)?;
    loom.import_state(&planned.state)?;
    *loom.mutable_overlay_mut() = planned.overlay;
    synchronize_document_transaction_receipt(loom, &receipt)?;
    Ok(result)
}

pub fn publish_document_put_binary(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    collection: &str,
    id: &str,
    bytes: Vec<u8>,
    expected_entity_tag: Option<&str>,
    before_commit: impl FnOnce(&mut Loom<FileStore>) -> Result<(), LoomError>,
) -> Result<Vec<u8>, LoomError> {
    let mut candidate =
        loom.fork_state_into(loom_core::provider::PlanningObjectStore::new(loom.store()))?;
    let planned = plan_document_put(
        &mut candidate,
        workspace,
        collection,
        id,
        bytes,
        expected_entity_tag,
    )?;
    drop(candidate);
    publish_document_put_candidate(loom, planned, before_commit)
}

pub fn publish_document_put_text(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    collection: &str,
    id: &str,
    text: &str,
    expected_entity_tag: Option<&str>,
    before_commit: impl FnOnce(&mut Loom<FileStore>) -> Result<(), LoomError>,
) -> Result<Vec<u8>, LoomError> {
    publish_document_put_binary(
        loom,
        workspace,
        collection,
        id,
        text.as_bytes().to_vec(),
        expected_entity_tag,
        before_commit,
    )
}

fn mint_workspace_id() -> Result<WorkspaceId, LoomError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|err| LoomError::new(Code::Internal, format!("rng failure: {err}")))?;
    Ok(WorkspaceId::v4_from_bytes(bytes))
}

fn fill_random(buf: &mut [u8]) -> Result<(), LoomError> {
    getrandom::fill(buf).map_err(|err| LoomError::new(Code::Internal, format!("rng: {err}")))
}

/// Mint an app credential: generate a fresh id, 32-byte secret, and 16-byte salt, store only the salted
/// verifier in `identity`, and return the record with the one-time plaintext bearer token. The plaintext
/// secret is never persisted or returned again. Shared by the local CLI path and the server dispatch so
/// both mint the secret identically, server-side.
pub fn mint_app_credential(
    identity: &mut IdentityStore,
    principal: WorkspaceId,
    label: &str,
) -> Result<(loom_core::AppCredential, String), LoomError> {
    let id = mint_workspace_id()?;
    let mut secret = [0u8; 32];
    let mut salt = [0u8; 16];
    fill_random(&mut secret)?;
    fill_random(&mut salt)?;
    let credential = identity.create_app_credential(principal, id, label, &secret, &salt)?;
    let token = loom_core::identity::app_credential_token(id, &secret);
    Ok((credential, token))
}

/// The identity profile (digest algorithm) named by `profile`.
fn parse_profile(profile: &str) -> Result<Algo, LoomError> {
    match profile {
        "default" | "blake3" => Ok(Algo::Blake3),
        "fips" | "sha256" => Ok(Algo::Sha256),
        other => Err(LoomError::new(
            Code::InvalidArgument,
            format!(
                "unknown identity profile {other:?} (expected `default`/`blake3` or `fips`/`sha256`)"
            ),
        )),
    }
}

/// A deterministic workspace id from the SQL session workspace name, matching the `loom` CLI and the FFI
/// (`derive_sql_ns_id`) so the same name resolves to the same SQL workspace across every consumer.
fn derive_sql_ns_id(name: &str) -> WorkspaceId {
    let d = Digest::blake3(format!("{}:{name}", FacetKind::Sql.as_str()).as_bytes());
    let mut id = [0u8; 16];
    id.copy_from_slice(&d.bytes()[..16]);
    WorkspaceId::from_bytes(id)
}

/// A `LocalOpenAuth` carrying a passphrase-derived unlock key (for an encrypted store).
fn sql_auth_keyed(passphrase: &[u8]) -> Result<LocalOpenAuth, LoomError> {
    let passphrase = std::str::from_utf8(passphrase)
        .map_err(|_| LoomError::new(Code::InvalidArgument, "passphrase is not valid utf-8"))?;
    Ok(LocalOpenAuth {
        unlock_key: Some(KeySpec::passphrase(passphrase)),
        ..LocalOpenAuth::default()
    })
}

/// A `LocalOpenAuth` carrying a raw 256-bit KEK unlock key (for an encrypted store).
fn sql_auth_kek(kek: [u8; KEY_LEN]) -> LocalOpenAuth {
    LocalOpenAuth {
        unlock_key: Some(KeySpec::raw_kek(kek)),
        ..LocalOpenAuth::default()
    }
}

/// Add principal passphrase auth to a `LocalOpenAuth` (for an authenticated store), minting a session id.
fn sql_auth_with_principal(
    mut auth: LocalOpenAuth,
    principal: PrincipalId,
    passphrase: &[u8],
) -> Result<LocalOpenAuth, LoomError> {
    let passphrase = std::str::from_utf8(passphrase)
        .map_err(|_| LoomError::new(Code::InvalidArgument, "passphrase is not valid utf-8"))?;
    auth.principal = Some(principal);
    auth.passphrase = Some(passphrase.to_string());
    auth.session_id = Some(mint_workspace_id()?.to_string());
    Ok(auth)
}

fn ns_selector(workspace: &str, facet: FacetKind) -> WsSelector {
    match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: facet,
            name: workspace.to_string(),
        },
    }
}

fn ns_selector_by_name(workspace: &str) -> WsSelector {
    match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Name(workspace.to_string()),
    }
}

fn ensure_generated_facet_workspace<S: ObjectStore>(
    loom: &mut Loom<S>,
    workspace: &str,
    facet: FacetKind,
) -> Result<WorkspaceId, LoomError> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: facet,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, mint_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, facet)?;
    Ok(ns)
}

/// Resolve a workspace for a read, returning `None` when it does not exist yet (so absent facets read
/// as empty rather than an error).
fn read_ns(
    loom: &Loom<FileStore>,
    workspace: &str,
    facet: FacetKind,
) -> Result<Option<WorkspaceId>, LoomError> {
    match loom.registry().open(&ns_selector(workspace, facet)) {
        Ok(id) => Ok(Some(id)),
        Err(err) if err.code == Code::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// Serialize a canonical JSON value (from the document-index JSON encoders) to the `Vec<u8>` the
/// generated `Document` methods return.
fn document_json_to_bytes(value: &serde_json::Value) -> Result<Vec<u8>, LoomError> {
    serde_json::to_vec(value).map_err(|e| LoomError::new(Code::CorruptObject, format!("json: {e}")))
}

/// The state behind one open SQL session handle: its parent store session, the SQL facet workspace and
/// database name it was opened against, and the live eager-loaded SQL store.
/// A path-bound SQL session: a cheap reopenable handle to
/// `(workspace, database)` over the client's bound store, plus optional unlock key material. It holds no
/// open loom or in-memory store between calls; each `sql_exec`/`sql_query`/`sql_commit` reopens the bound
/// path, matching the engine's single-writer / lock-free-reader model. Cross-statement transactions use a
/// `SqlBatch`, not a session.
#[derive(Clone)]
struct SqlSessionState {
    ns_name: String,
    ns: WorkspaceId,
    db: String,
    /// Unlock key (for an encrypted store) and principal auth (for an authenticated store), applied at
    /// each reopen via `attach_local_auth` - mirroring the loom-ffi `LocalOpenAuth` model.
    auth: LocalOpenAuth,
}

/// A SQL transaction batch: it holds the exclusive write loom for its
/// whole lifetime and a mutation-capable store over a read snapshot; statements accumulate in the store
/// and only `sql_batch_commit`/`sql_batch_commit_vcs` flush and save (the atomic persistence boundary).
/// Not `Clone` - operated in place under the registry lock, which is safe because a batch already holds
/// the single write lock.
struct SqlBatchState {
    loom: Loom<FileStore>,
    store: LoomSqlStore,
    ns: WorkspaceId,
    db: String,
    auth: LocalOpenAuth,
}

/// The deferred work behind an asynchronous [`Task`] (portable cooperative model). Each
/// variant reruns an existing synchronous op on first poll; handles are captured by value so the task
/// borrows nothing.
#[derive(Clone)]
enum TaskWork {
    SqlExec {
        session: SqlSession,
        sql: String,
    },
    ReadTable {
        session: LoomSession,
        workspace: String,
        table: String,
    },
    IndexScan {
        session: LoomSession,
        workspace: String,
        table: String,
        index: String,
        prefix: Vec<u8>,
    },
    Blame {
        session: LoomSession,
        workspace: String,
        branch: String,
        table: String,
    },
    Diff {
        session: LoomSession,
        workspace: String,
        table: String,
        from: String,
        to: String,
    },
    LogAsync {
        session: LoomSession,
        workspace: String,
        branch: String,
    },
    MergeAsync {
        session: LoomSession,
        workspace: String,
        from_branch: String,
        author: String,
        cell_level: bool,
    },
    ImportFsAsync {
        session: LoomSession,
        workspace: String,
        src_path: String,
        author: Option<String>,
        message: Option<String>,
        commit: bool,
        dry_run: bool,
    },
    ExportFsAsync {
        session: LoomSession,
        workspace: String,
        dst_path: String,
        revision: Option<String>,
        dry_run: bool,
    },
    ArchiveImportAsync {
        session: LoomSession,
        workspace: String,
        src_path: String,
        kind: String,
        gzip_output_path: Option<String>,
        commit: bool,
        author: Option<String>,
        message: Option<String>,
        dry_run: bool,
    },
    ArchiveExportAsync {
        session: LoomSession,
        workspace: String,
        dst_path: String,
        kind: String,
        revision: Option<String>,
        dry_run: bool,
    },
    CarImportAsync {
        session: LoomSession,
        src_path: String,
        dry_run: bool,
    },
    CarExportAsync {
        session: LoomSession,
        workspace: String,
        dst_path: String,
        dry_run: bool,
    },
}

/// The lifecycle of an asynchronous task: `Pending` until the first `task_poll` runs it to a terminal
/// `Ready`/`Errored`/`Cancelled`; `task_result`/`task_wait` take the buffer, leaving `Taken`.
enum TaskState {
    Pending(TaskWork),
    Ready(Vec<u8>),
    Errored(LoomError),
    Cancelled,
    Taken,
}

/// A local Loom client bound to one `.loom` path.
pub struct LocalLoomClient {
    path: PathBuf,
    sessions: Mutex<HashMap<u64, LocalSessionSlot>>,
    sql_sessions: Mutex<HashMap<u64, SqlSessionState>>,
    sql_batches: Mutex<HashMap<u64, SqlBatchState>>,
    row_iters: Mutex<HashMap<u64, VecDeque<Vec<u8>>>>,
    tasks: Mutex<HashMap<u64, TaskState>>,
    result_views: Mutex<HashMap<u64, loom_result::result_view::ResultPayload>>,
    next_id: Mutex<u64>,
    locks: Arc<dyn LocksAuthority>,
    lock_session_namespace: u64,
    last_error: Mutex<Option<LoomError>>,
    transfers: Mutex<HashMap<Vec<u8>, TransferEntry>>,
    store_maintenance_clock: Arc<dyn StoreMaintenanceClock + Send + Sync>,
}

pub struct VectorTextUpsertRequest {
    pub workspace: String,
    pub name: String,
    pub id: String,
    pub vector: Vec<f32>,
    pub metadata: BTreeMap<String, Value>,
    pub source_text: String,
    pub model: Option<EmbeddingModel>,
    pub create: bool,
    pub metric: Metric,
    pub condition: VectorCondition,
}

pub fn vector_text_upsert_request_from_cbor(
    request: &[u8],
) -> Result<VectorTextUpsertRequest, LoomError> {
    let request = loom_wire::vector::text_upsert_request_from_cbor(request)?;
    if request.expected_token.is_some() && request.expect_absent {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "expected_token and expect_absent are mutually exclusive",
        ));
    }
    let vector = loom_wire::vector::floats_from_bytes(&request.vector)?;
    let metadata = loom_wire::vector::metadata_from_cbor(&request.metadata)?;
    let source_text = std::str::from_utf8(&request.source_text)
        .map_err(|err| LoomError::new(Code::InvalidArgument, format!("source_text: {err}")))?
        .to_string();
    let model = request
        .model_id
        .map(|id| loom_core::EmbeddingModel::new(id, vector.len(), request.weights_digest));
    let metric = loom_wire::vector::metric_from_int(request.metric)?;
    let condition = if let Some(token) = request.expected_token {
        loom_core::VectorCondition::Exact(loom_core::VectorExactToken::from_bytes(token))
    } else if request.expect_absent {
        loom_core::VectorCondition::Absent
    } else {
        loom_core::VectorCondition::Any
    };
    Ok(VectorTextUpsertRequest {
        workspace: request.workspace,
        name: request.name,
        id: request.id,
        vector,
        metadata,
        source_text,
        model,
        create: request.create,
        metric,
        condition,
    })
}

pub fn apply_vector_text_upsert_generated(
    loom: &mut Loom<FileStore>,
    request: VectorTextUpsertRequest,
) -> Result<Vec<u8>, LoomError> {
    if let loom_core::VectorCondition::Exact(expected) = &request.condition {
        let ns = loom
            .registry()
            .open(&ns_selector(&request.workspace, FacetKind::Vector))
            .map_err(|error| {
                if error.code == Code::NotFound {
                    LoomError::new(Code::Conflict, "vector exact condition did not match")
                } else {
                    error
                }
            })?;
        match vector_exact_token(loom, ns, &request.name, &request.id) {
            Ok(Some(current)) if &current == expected => {}
            Ok(_) => {
                return Err(LoomError::new(
                    Code::Conflict,
                    "vector exact condition did not match",
                ));
            }
            Err(error) if error.code == Code::NotFound => {
                return Err(LoomError::new(
                    Code::Conflict,
                    "vector exact condition did not match",
                ));
            }
            Err(error) => return Err(error),
        }
    }
    let ns = if request.create {
        loom.registry_mut().ensure_for_write(
            &ns_selector(&request.workspace, FacetKind::Vector),
            mint_workspace_id()?,
        )?
    } else {
        loom.registry()
            .open(&ns_selector(&request.workspace, FacetKind::Vector))?
    };
    if request.create {
        match vector_create(
            loom,
            ns,
            &request.name,
            request
                .model
                .as_ref()
                .map_or(request.vector.len(), |model| model.dimension),
            request.metric,
        ) {
            Ok(()) => {}
            Err(error) if error.code == Code::Conflict => {}
            Err(error) => return Err(error),
        }
    }
    let id = request.id.clone();
    let name = request.name.clone();
    let token = vector_upsert_with_source_conditioned(
        loom,
        ns,
        &request.name,
        &request.id,
        request.vector,
        request.metadata,
        &request.source_text,
        request.model,
        request.condition,
    )?;
    Ok(loom_wire::vector::text_upsert_report_with_token_to_cbor(
        &id,
        &name,
        token.as_bytes(),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct VectorWorkspaceConfigureRequest {
    embedding_instance: String,
}

pub fn parse_vector_workspace_configure_request(
    request_json: &str,
) -> Result<VectorWorkspaceConfigureRequest, LoomError> {
    serde_json::from_str(request_json).map_err(|error| {
        LoomError::new(
            Code::InvalidArgument,
            format!("invalid vector workspace configure request: {error}"),
        )
    })
}

pub fn apply_vector_workspace_configure_json(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    request: VectorWorkspaceConfigureRequest,
) -> Result<String, LoomError> {
    let workspace_id = loom.registry().open(&ns_selector_by_name(workspace))?;
    let mut state = inference_instance_state(loom, workspace_id)?;
    let instance = state
        .find_instance(&request.embedding_instance)
        .cloned()
        .ok_or_else(|| {
            LoomError::new(
                Code::NotFound,
                format!(
                    "inference instance {:?} not found",
                    request.embedding_instance
                ),
            )
        })?;
    if instance.kind != InferenceModelKind::TextEmbedding {
        return Err(LoomError::new(
            Code::InvalidArgument,
            format!(
                "inference instance {:?} is not a text-embedding instance",
                request.embedding_instance
            ),
        ));
    }
    let binding = loom_inference::VectorWorkspaceBinding {
        workspace: workspace_id.to_string(),
        embedding_instance: request.embedding_instance,
    };
    state.upsert_vector_binding(binding.clone());
    put_inference_instance_state(loom, workspace_id, &state)?;
    serde_json::to_string(&binding)
        .map_err(|error| LoomError::new(Code::CorruptObject, format!("json: {error}")))
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct StudioReindexReport {
    pub workspace: String,
    pub profile: String,
    pub job_path: String,
    pub state: String,
    pub source_digest: String,
    pub model_id: String,
    pub vector_records_indexed: u64,
    pub vector_records_deleted: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct StudioRevisionRebuildReport {
    pub workspace: String,
    pub scope_id: String,
    pub profile: String,
    pub index_present_before: bool,
    pub candidates: u64,
    pub inserted: u64,
    pub skipped_existing: u64,
    pub dry_run: bool,
}

struct StudioVectorDrainSummary {
    indexed: u64,
    deleted: u64,
}

struct ResolvedStudioTextEmbeddingInstance {
    instance: loom_types::InferenceInstanceDescriptor,
    handle: loom_inference::TextEmbeddingHandle,
}

#[derive(Clone)]
struct LocalSessionSlot {
    loom: Arc<Mutex<Loom<FileStore>>>,
    auth_view: LocalSessionAuthView,
    lock_owner_session: Option<String>,
}

#[derive(Clone)]
enum LocalSessionAuthView {
    Preserve,
    Clear,
    Authenticated {
        principal: PrincipalId,
        session_id: String,
    },
}

/// One in-flight byte-transfer import staging area (`specs/0067` §17.3), keyed by its opaque
/// `TransferId` bytes. `commit_report` caches the canonical import-report after a successful
/// `finish` so a replayed `finish` is finalize-once.
struct TransferEntry {
    staging: loom_interchange_io::transfer::TransferStaging,
    workspace: String,
    commit_report: Option<Vec<u8>>,
}

/// Arguments for [`LocalLoomClient::document_replace_text_indexed`]: the timestamp-free find/replace over
/// document `collection`/`id`, guarded by `base_digest`.
pub struct DocumentReplaceTextArgs<'a> {
    pub workspace: &'a str,
    pub collection: &'a str,
    pub id: &'a str,
    pub find: &'a str,
    pub replace: &'a str,
    pub replace_all: bool,
    pub base_digest: &'a str,
}

impl LocalLoomClient {
    pub(crate) fn store_path(&self) -> &Path {
        &self.path
    }

    /// Bind a client to a local `.loom` path. The path is opened lazily by [`LocalLoomClient::open`].
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self::with_store_maintenance_clock(path, Arc::new(SystemStoreMaintenanceClock))
    }

    /// Bind a local client to a shared coordination-lock authority.
    pub fn with_locks_authority(path: impl Into<PathBuf>, locks: Arc<dyn LocksAuthority>) -> Self {
        Self::with_locks_authority_and_clock(path, locks, Arc::new(SystemStoreMaintenanceClock))
    }

    pub(crate) fn with_store_maintenance_clock(
        path: impl Into<PathBuf>,
        store_maintenance_clock: Arc<dyn StoreMaintenanceClock + Send + Sync>,
    ) -> Self {
        Self::with_locks_authority_and_clock(
            path,
            Arc::new(InProcessLocksAuthority::default()),
            store_maintenance_clock,
        )
    }

    fn with_locks_authority_and_clock(
        path: impl Into<PathBuf>,
        locks: Arc<dyn LocksAuthority>,
        store_maintenance_clock: Arc<dyn StoreMaintenanceClock + Send + Sync>,
    ) -> Self {
        Self {
            path: path.into(),
            sessions: Mutex::new(HashMap::new()),
            sql_sessions: Mutex::new(HashMap::new()),
            sql_batches: Mutex::new(HashMap::new()),
            row_iters: Mutex::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
            result_views: Mutex::new(HashMap::new()),
            next_id: Mutex::new(0),
            locks,
            lock_session_namespace: NEXT_LOCK_SESSION_NAMESPACE.fetch_add(1, Ordering::Relaxed),
            last_error: Mutex::new(None),
            transfers: Mutex::new(HashMap::new()),
            store_maintenance_clock,
        }
    }

    /// Create a fresh unencrypted store at the bound path under the default (BLAKE3) profile.
    ///
    /// # Errors
    /// Returns [`LoomError`] if the store cannot be created or the initial save fails.
    pub fn create(&self) -> Result<(), LoomError> {
        let store = FileStore::create_with_profile(&self.path, Algo::Blake3)?;
        let mut loom = Loom::new(store);
        save_loom(&mut loom)?;
        Ok(())
    }

    /// Open a session over the bound store and register it, returning its handle.
    ///
    /// # Errors
    /// Returns [`LoomError`] if the store cannot be opened.
    pub fn open(&self) -> Result<LoomSession, LoomError> {
        self.register_open_loom(open_loom(&self.path)?)
    }

    pub fn open_with_local_auth(
        &self,
        auth: &LocalOpenAuth,
        daemon_authorized: bool,
    ) -> Result<LoomSession, LoomError> {
        let loom = if daemon_authorized {
            open_loom_daemon_authorized_unlocked(&self.path, auth.unlock_key.as_ref())?
        } else {
            open_loom_unlocked(&self.path, auth.unlock_key.as_ref())?
        };
        self.register_open_loom(attach_local_auth(loom, auth)?)
    }

    /// Open a read-only session over the bound store and register it, returning its handle.
    ///
    /// # Errors
    /// Returns [`LoomError`] if the store cannot be opened.
    pub fn open_read(&self) -> Result<LoomSession, LoomError> {
        self.register_open_loom(open_loom_read(&self.path)?)
    }

    /// Open a metadata-only session over the bound store.
    ///
    /// # Errors
    /// Returns [`LoomError`] if the store cannot be opened.
    pub fn open_metadata(&self) -> Result<LoomSession, LoomError> {
        self.register_open_loom(open_loom_registry_read_unlocked(&self.path, None)?)
    }

    /// Create a store at the bound path under `profile`, optionally encrypted with `key`, and seed its
    /// identity and admin ACL for the generated root principal.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown profile, a suite without a credential, or a store failure.
    pub(crate) fn create_store(
        &self,
        profile: &str,
        suite: Option<&str>,
        key: Option<KeySpec>,
    ) -> Result<(), LoomError> {
        let digest_algo = parse_profile(profile)?;
        let store = match key {
            None => {
                if suite.is_some() {
                    return Err(LoomError::new(
                        Code::InvalidArgument,
                        "a suite was given without a credential; encryption requires a passphrase or KEK",
                    ));
                }
                FileStore::create_with_profile(&self.path, digest_algo)?
            }
            Some(spec) => {
                // The FIPS profile pairs AES-256-GCM by default; the default profile pairs XChaCha20.
                let suite = match suite {
                    Some(s) => Suite::parse(s)?,
                    None if digest_algo == Algo::Sha256 => Suite::Aes256Gcm,
                    None => Suite::XChaCha20Poly1305,
                };
                let mut salt = [0u8; 16];
                let mut dek = [0u8; KEY_LEN];
                let mut wrap_nonce = [0u8; 24];
                fill_random(&mut salt)?;
                fill_random(&mut dek)?;
                fill_random(&mut wrap_nonce)?;
                let (meta, session) =
                    EncryptionMeta::create(&spec, suite, salt.to_vec(), dek, wrap_nonce.to_vec())?;
                FileStore::create_encrypted_with_profile(
                    &self.path,
                    meta.encode(),
                    session,
                    digest_algo,
                )?
            }
        };
        let root = mint_workspace_id()?;
        store.save_identity_store(&IdentityStore::new(root))?;
        let mut acl = AclStore::new();
        acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])?;
        store.save_acl_store(&acl)?;
        Ok(())
    }

    /// Open an encrypted store unlocked with `passphrase`, registering a session handle.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a non-utf-8 passphrase, a wrong key, or an open failure.
    pub fn open_keyed(&self, passphrase: &[u8]) -> Result<LoomSession, LoomError> {
        let passphrase = std::str::from_utf8(passphrase)
            .map_err(|_| LoomError::new(Code::InvalidArgument, "passphrase is not valid utf-8"))?;
        self.open_with_key(KeySpec::passphrase(passphrase))
    }

    /// Open a read-only encrypted-store session unlocked with `passphrase`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a non-utf-8 passphrase, a wrong key, or an open failure.
    pub fn open_read_keyed(&self, passphrase: &[u8]) -> Result<LoomSession, LoomError> {
        let passphrase = std::str::from_utf8(passphrase)
            .map_err(|_| LoomError::new(Code::InvalidArgument, "passphrase is not valid utf-8"))?;
        self.open_read_with_key(KeySpec::passphrase(passphrase))
    }

    /// Open a metadata-only encrypted-store session unlocked with `passphrase`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a non-utf-8 passphrase, a wrong key, or an open failure.
    pub fn open_metadata_keyed(&self, passphrase: &[u8]) -> Result<LoomSession, LoomError> {
        let passphrase = std::str::from_utf8(passphrase)
            .map_err(|_| LoomError::new(Code::InvalidArgument, "passphrase is not valid utf-8"))?;
        self.register_open_loom(open_loom_registry_read_unlocked(
            &self.path,
            Some(&KeySpec::passphrase(passphrase)),
        )?)
    }

    /// Open an encrypted store unlocked with a raw 256-bit KEK, registering a session handle.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a wrong key or an open failure.
    pub fn open_with_kek(&self, kek: [u8; KEY_LEN]) -> Result<LoomSession, LoomError> {
        self.open_with_key(KeySpec::raw_kek(kek))
    }

    /// Open a read-only encrypted-store session unlocked with a raw 256-bit KEK.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a wrong key or an open failure.
    pub fn open_read_with_kek(&self, kek: [u8; KEY_LEN]) -> Result<LoomSession, LoomError> {
        self.open_read_with_key(KeySpec::raw_kek(kek))
    }

    fn open_with_key(&self, spec: KeySpec) -> Result<LoomSession, LoomError> {
        self.register_open_loom(open_loom_unlocked(&self.path, Some(&spec))?)
    }

    fn open_read_with_key(&self, spec: KeySpec) -> Result<LoomSession, LoomError> {
        self.register_open_loom(open_loom_read_unlocked(&self.path, Some(&spec))?)
    }

    fn register_open_loom(&self, mut loom: Loom<FileStore>) -> Result<LoomSession, LoomError> {
        if let Some(identity) = loom.store().identity_store()? {
            loom.set_identity_store(identity);
        }
        if let Some(acl) = loom.store().acl_store()? {
            loom.set_acl_store(acl);
        }
        let id = {
            let mut next = self.next_id.lock().expect("session id lock");
            *next += 1;
            *next
        };
        self.sessions.lock().expect("session lock").insert(
            id,
            LocalSessionSlot {
                loom: Arc::new(Mutex::new(loom)),
                auth_view: LocalSessionAuthView::Preserve,
                lock_owner_session: None,
            },
        );
        Ok(LoomSession(HandleId {
            kind: "session".to_string(),
            id: id.to_be_bytes().to_vec(),
            generation: 1,
            owner_session: Vec::new(),
        }))
    }

    /// Register an existing daemon-owned engine slot as a client session handle.
    pub fn register_daemon_shared_loom(
        &self,
        loom: Arc<Mutex<Loom<FileStore>>>,
        auth_session: Option<(PrincipalId, String)>,
    ) -> Result<LoomSession, LoomError> {
        let id = {
            let mut next = self.next_id.lock().expect("session id lock");
            *next += 1;
            *next
        };
        let auth_view = match auth_session {
            Some((principal, session_id)) => LocalSessionAuthView::Authenticated {
                principal,
                session_id,
            },
            None => LocalSessionAuthView::Clear,
        };
        self.sessions.lock().expect("session lock").insert(
            id,
            LocalSessionSlot {
                loom,
                auth_view,
                lock_owner_session: None,
            },
        );
        Ok(LoomSession(HandleId {
            kind: "session".to_string(),
            id: id.to_be_bytes().to_vec(),
            generation: 1,
            owner_session: Vec::new(),
        }))
    }

    /// The content address of `data` as a blob object.
    pub fn blob_digest(&self, data: &[u8]) -> Digest {
        Object::Blob(data.to_vec()).digest()
    }

    /// Run `f` against the open `Loom` for `session`.
    ///
    /// # Errors
    /// Returns [`Code::NotFound`] when the session handle is unknown, or the error `f` returns.
    pub fn with_session<T>(
        &self,
        session: &LoomSession,
        f: impl FnOnce(&mut Loom<FileStore>) -> Result<T, LoomError>,
    ) -> Result<T, LoomError> {
        self.with_session_inner(session, true, f)
    }

    fn with_metadata_session<T>(
        &self,
        session: &LoomSession,
        f: impl FnOnce(&mut Loom<FileStore>) -> Result<T, LoomError>,
    ) -> Result<T, LoomError> {
        self.with_session_inner(session, false, f)
    }

    fn with_session_inner<T>(
        &self,
        session: &LoomSession,
        materialize: bool,
        f: impl FnOnce(&mut Loom<FileStore>) -> Result<T, LoomError>,
    ) -> Result<T, LoomError> {
        let key = handle_key(session)?;
        let slot = self
            .sessions
            .lock()
            .expect("session lock")
            .get(&key)
            .cloned()
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown session handle"))?;
        let mut loom = slot
            .loom
            .lock()
            .map_err(|_| LoomError::new(Code::Internal, "session lock poisoned"))?;
        if materialize {
            loom.ensure_full_state_loaded()?;
        }
        match &slot.auth_view {
            LocalSessionAuthView::Preserve => {}
            LocalSessionAuthView::Clear => loom.clear_session(),
            LocalSessionAuthView::Authenticated {
                principal,
                session_id,
            } => {
                if let Some(identity) = loom.identity_store_mut() {
                    identity.bind_session(*principal, session_id.clone())?;
                }
                loom.set_session(session_id.clone());
            }
        }
        f(&mut loom)
    }

    /// Persist the working state of `session` (the save boundary).
    ///
    /// # Errors
    /// Returns [`LoomError`] when the handle is unknown or the save fails.
    pub fn save(&self, session: &LoomSession) -> Result<(), LoomError> {
        self.with_session(session, save_loom)
    }

    /// Close `session`, releasing its open store. Idempotent: returns whether it was open.
    pub fn close(&self, session: &LoomSession) -> bool {
        match handle_key(session) {
            Ok(key) => self
                .sessions
                .lock()
                .expect("session lock")
                .remove(&key)
                .is_some(),
            Err(_) => false,
        }
    }

    /// The number of open sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.lock().expect("session lock").len()
    }

    /// Store `content` in the content-addressed facet of `workspace`, ensuring the workspace on first
    /// write, and persist. Returns the content address.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an rng failure, or an engine or save failure.
    pub fn cas_put(
        &self,
        session: &LoomSession,
        workspace: &str,
        content: &[u8],
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let selector = ns_selector(workspace, FacetKind::Cas);
            let ns = loom
                .registry_mut()
                .ensure_for_write(&selector, mint_workspace_id()?)?;
            let digest = cas_put(loom, ns, content)?;
            save_loom(loom)?;
            Ok(digest)
        })
    }

    /// Read the blob addressed by `digest` from the content-addressed facet of `workspace`, or `None`
    /// when the digest or workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn cas_get(
        &self,
        session: &LoomSession,
        workspace: &str,
        digest: &Digest,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            let selector = ns_selector(workspace, FacetKind::Cas);
            match loom.registry().open(&selector) {
                Ok(ns) => cas_get(loom, ns, digest),
                Err(err) if err.code == Code::NotFound => Ok(None),
                Err(err) => Err(err),
            }
        })
    }

    /// Report whether `digest` is present in the content-addressed facet of `workspace`; `false` when
    /// the digest or workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn cas_has(
        &self,
        session: &LoomSession,
        workspace: &str,
        digest: &Digest,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            let selector = ns_selector(workspace, FacetKind::Cas);
            match loom.registry().open(&selector) {
                Ok(ns) => cas_has(loom, ns, digest),
                Err(err) if err.code == Code::NotFound => Ok(false),
                Err(err) => Err(err),
            }
        })
    }

    /// Drop the blob addressed by `digest` from `workspace`'s working tree and persist; returns
    /// whether it was present. `false` when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn cas_delete(
        &self,
        session: &LoomSession,
        workspace: &str,
        digest: &Digest,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            let selector = ns_selector(workspace, FacetKind::Cas);
            let ns = match loom.registry().open(&selector) {
                Ok(ns) => ns,
                Err(err) if err.code == Code::NotFound => return Ok(false),
                Err(err) => return Err(err),
            };
            let present = cas_delete(loom, ns, digest)?;
            if present {
                save_loom(loom)?;
            }
            Ok(present)
        })
    }

    /// List the digests reachable in `workspace`'s content-addressed facet, sorted; empty when the
    /// workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn cas_list(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<Digest>, LoomError> {
        self.with_session(session, |loom| {
            let selector = ns_selector(workspace, FacetKind::Cas);
            match loom.registry().open(&selector) {
                Ok(ns) => cas_list(loom, ns),
                Err(err) if err.code == Code::NotFound => Ok(Vec::new()),
                Err(err) => Err(err),
            }
        })
    }

    /// Create a workspace, optionally naming it and marking a facet present, and persist. Returns the
    /// new workspace id.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a name clash, an rng failure, or a save failure.
    pub fn workspace_create(
        &self,
        session: &LoomSession,
        name: Option<&str>,
        facet: Option<FacetKind>,
    ) -> Result<WorkspaceId, LoomError> {
        self.with_session(session, |loom| {
            let id = mint_workspace_id()?;
            let created = match facet {
                Some(facet) => loom.registry_mut().create(facet, name, id)?,
                None => loom.registry_mut().create_workspace(name, id)?,
            };
            save_loom(loom)?;
            Ok(created)
        })
    }

    /// List the workspaces in the store.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session.
    pub fn workspace_list(&self, session: &LoomSession) -> Result<Vec<WorkspaceInfo>, LoomError> {
        self.with_metadata_session(session, |loom| Ok(loom.registry().list(None)))
    }

    /// Rename a workspace (selected by name or id) and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, or a name clash.
    pub fn workspace_rename(
        &self,
        session: &LoomSession,
        workspace: &str,
        new_name: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let id = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.registry_mut().rename(id, new_name)?;
            save_loom(loom)
        })
    }

    /// Delete a workspace (selected by name or id) and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace.
    pub fn workspace_delete(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let id = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.registry_mut().delete(id)?;
            save_loom(loom)
        })
    }

    /// Resolve a workspace name-or-id string to its stable [`WorkspaceId`] within `session`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or for an unknown session.
    pub fn resolve_workspace_id(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<WorkspaceId, LoomError> {
        self.with_metadata_session(session, |loom| {
            loom.registry().open(&ns_selector_by_name(workspace))
        })
    }

    /// Append `payload` to the ledger `collection` in `workspace`, ensuring the workspace on first
    /// write, and persist. Returns the new zero-based sequence.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an rng failure, or an engine or save failure.
    pub fn ledger_append(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        payload: &[u8],
    ) -> Result<u64, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Ledger),
                mint_workspace_id()?,
            )?;
            let seq = ledger_append(loom, ns, collection, payload.to_vec())?;
            save_loom(loom)?;
            Ok(seq)
        })
    }

    /// Read the ledger entry at `seq` in `collection`, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn ledger_get(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        seq: u64,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Ledger)? {
                Some(ns) => ledger_get(loom, ns, collection, seq),
                None => Ok(None),
            }
        })
    }

    /// The ledger chain head digest for `collection`, or `None` for an empty or absent ledger.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn ledger_head(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
    ) -> Result<Option<Digest>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Ledger)? {
                Some(ns) => ledger_head(loom, ns, collection),
                None => Ok(None),
            }
        })
    }

    /// The number of entries in the ledger `collection`, or `0` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn ledger_len(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
    ) -> Result<u64, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Ledger)? {
                Some(ns) => ledger_len(loom, ns, collection),
                None => Ok(0),
            }
        })
    }

    /// Verify the hash chain of the ledger `collection`. An absent ledger verifies trivially.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`INTEGRITY_FAILURE`) on a broken chain, or for an unknown session.
    pub fn ledger_verify(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Ledger)? {
                Some(ns) => ledger_verify(loom, ns, collection),
                None => Ok(()),
            }
        })
    }

    /// The ledger collection names in `workspace`, sorted; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn ledger_list_collections(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Ledger)? {
                Some(ns) => ledger_list_collections(loom, ns),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Put `value` at timestamp `ts` in the time-series `collection`, ensuring the workspace on first
    /// write, and persist. A repeated timestamp replaces the point.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an rng failure, or an engine or save failure.
    pub fn ts_put(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        ts: i64,
        value: &[u8],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::TimeSeries),
                mint_workspace_id()?,
            )?;
            ts_put(loom, ns, collection, ts, value.to_vec())?;
            save_loom(loom)
        })
    }

    /// Read the time-series point at `ts` in `collection`, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn ts_get(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        ts: i64,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::TimeSeries)? {
                Some(ns) => ts_get(loom, ns, collection, ts),
                None => Ok(None),
            }
        })
    }

    /// The most recent `(timestamp, value)` in `collection`, or `None` when empty or absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn ts_latest(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
    ) -> Result<Option<(i64, Vec<u8>)>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::TimeSeries)? {
                Some(ns) => ts_latest(loom, ns, collection),
                None => Ok(None),
            }
        })
    }

    /// The half-open `[from, to)` window of time-series `collection` as canonical CBOR; an empty
    /// series when the collection or workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn ts_range(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        from: i64,
        to: i64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::TimeSeries)? {
                Some(ns) => Ok(ts_range(loom, ns, collection, from, to)?.encode()),
                None => Ok(Series::new().encode()),
            }
        })
    }

    /// The time-series collection names in `workspace`, sorted; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn ts_list_collections(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::TimeSeries)? {
                Some(ns) => ts_list_collections(loom, ns),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Store the canonical-CBOR metric `descriptor` in `workspace`, ensuring the workspace on first
    /// write, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed descriptor, or an engine/save failure.
    pub fn metrics_put_descriptor(
        &self,
        session: &LoomSession,
        workspace: &str,
        descriptor: &[u8],
    ) -> Result<(), LoomError> {
        let descriptor = MetricDescriptor::decode(descriptor)?;
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Metrics),
                mint_workspace_id()?,
            )?;
            metrics_put_descriptor(loom, ns, &descriptor)?;
            save_loom(loom)
        })
    }

    /// The canonical-CBOR metric descriptor named `name` in `workspace`, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn metrics_get_descriptor(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Metrics)? {
                Some(ns) => match metrics_get_descriptor(loom, ns, name)? {
                    Some(descriptor) => Ok(Some(descriptor.encode()?)),
                    None => Ok(None),
                },
                None => Ok(None),
            }
        })
    }

    /// Append the canonical-CBOR `observation` to the metric named `descriptor_name` in `workspace`,
    /// ensuring the workspace on first write, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed or unknown observation, or an
    /// engine/save failure.
    pub fn metrics_put_observation(
        &self,
        session: &LoomSession,
        workspace: &str,
        descriptor_name: &str,
        observation: &[u8],
    ) -> Result<(), LoomError> {
        let observation = MetricObservation::decode(observation)?;
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Metrics),
                mint_workspace_id()?,
            )?;
            metrics_put_observation(loom, ns, descriptor_name, &observation)?;
            save_loom(loom)
        })
    }

    /// Query observations of `descriptor_name` in the half-open window `[from_timestamp_ms,
    /// to_timestamp_ms)`, bounded by the scan/return limits, returning canonical CBOR
    /// `[observations, partial, stale]`. An absent workspace yields an empty, non-partial result.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, invalid query bounds, an unknown descriptor, or an
    /// engine failure.
    #[allow(clippy::too_many_arguments)]
    pub fn metrics_query(
        &self,
        session: &LoomSession,
        workspace: &str,
        descriptor_name: &str,
        from_timestamp_ms: u64,
        to_timestamp_ms: u64,
        max_series: u32,
        max_groups: u32,
        max_samples: u32,
        max_output_bytes: u64,
        now_timestamp_ms: u64,
    ) -> Result<Vec<u8>, LoomError> {
        let query = MetricQuery {
            from_timestamp_ms,
            to_timestamp_ms,
            max_series,
            max_groups,
            max_samples,
            max_output_bytes,
            now_timestamp_ms,
        };
        self.with_session(session, |loom| {
            let result = match read_ns(loom, workspace, FacetKind::Metrics)? {
                Some(ns) => metrics_query_observations(loom, ns, descriptor_name, &query)?,
                None => MetricQueryResult {
                    observations: Vec::new(),
                    partial: false,
                    stale: false,
                },
            };
            metric_query_result_to_cbor(&result)
        })
    }

    pub fn logs_put_record(
        &self,
        session: &LoomSession,
        workspace: &str,
        record: &[u8],
    ) -> Result<String, LoomError> {
        let record = LogRecord::decode(record)?;
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Logs),
                mint_workspace_id()?,
            )?;
            let record_id = logs_put_record(loom, ns, &record)?;
            save_loom(loom)?;
            Ok(record_id)
        })
    }

    pub fn logs_get_record(
        &self,
        session: &LoomSession,
        workspace: &str,
        record_id: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Logs)? {
                Some(ns) => match logs_get_record(loom, ns, record_id)? {
                    Some(record) => Ok(Some(record.encode()?)),
                    None => Ok(None),
                },
                None => Ok(None),
            }
        })
    }

    pub fn logs_query(
        &self,
        session: &LoomSession,
        workspace: &str,
        from_time_unix_nano: u64,
        to_time_unix_nano: u64,
        max_records: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, LoomError> {
        let query = LogQuery {
            from_time_unix_nano,
            to_time_unix_nano,
            max_records,
            max_output_bytes,
        };
        self.with_session(session, |loom| {
            let result = match read_ns(loom, workspace, FacetKind::Logs)? {
                Some(ns) => logs_query(loom, ns, &query)?,
                None => LogQueryResult {
                    records: Vec::new(),
                    partial: false,
                },
            };
            log_query_result_to_cbor(&result)
        })
    }

    pub fn traces_put_span(
        &self,
        session: &LoomSession,
        workspace: &str,
        span: &[u8],
    ) -> Result<(), LoomError> {
        let span = SpanRecord::decode(span)?;
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Traces),
                mint_workspace_id()?,
            )?;
            traces_put_span(loom, ns, &span)?;
            save_loom(loom)
        })
    }

    pub fn traces_get_span(
        &self,
        session: &LoomSession,
        workspace: &str,
        trace_id_hex: &str,
        span_id_hex: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Traces)? {
                Some(ns) => match traces_get_span(loom, ns, trace_id_hex, span_id_hex)? {
                    Some(span) => Ok(Some(span.encode()?)),
                    None => Ok(None),
                },
                None => Ok(None),
            }
        })
    }

    pub fn traces_trace_spans(
        &self,
        session: &LoomSession,
        workspace: &str,
        trace_id_hex: &str,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let result = match read_ns(loom, workspace, FacetKind::Traces)? {
                Some(ns) => {
                    traces_trace_spans(loom, ns, trace_id_hex, max_spans, max_output_bytes)?
                }
                None => TraceQueryResult {
                    spans: Vec::new(),
                    partial: false,
                },
            };
            trace_query_result_to_cbor(&result)
        })
    }

    pub fn traces_query(
        &self,
        session: &LoomSession,
        workspace: &str,
        from_start_time_ns: u64,
        to_start_time_ns: u64,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, LoomError> {
        let query = TraceQuery {
            from_start_time_ns,
            to_start_time_ns,
            max_spans,
            max_output_bytes,
        };
        self.with_session(session, |loom| {
            let result = match read_ns(loom, workspace, FacetKind::Traces)? {
                Some(ns) => traces_query(loom, ns, &query)?,
                None => TraceQueryResult {
                    spans: Vec::new(),
                    partial: false,
                },
            };
            trace_query_result_to_cbor(&result)
        })
    }

    /// Append `entry` to the queue `stream` in `workspace`, ensuring the workspace on first write, and
    /// persist. Returns the assigned zero-based sequence.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an rng failure, or an engine or save failure.
    pub fn queue_append(
        &self,
        session: &LoomSession,
        workspace: &str,
        stream: &str,
        entry: &[u8],
    ) -> Result<u64, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Queue),
                mint_workspace_id()?,
            )?;
            let seq = log::append(loom, ns, stream, entry)?;
            save_loom(loom)?;
            Ok(seq as u64)
        })
    }

    /// Read the queue entry at `seq` in `stream`, or `None` when out of range or absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn queue_get(
        &self,
        session: &LoomSession,
        workspace: &str,
        stream: &str,
        seq: u64,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Queue)? {
                Some(ns) => log::get(loom, ns, stream, seq as usize),
                None => Ok(None),
            }
        })
    }

    /// Read entries with `lo <= seq < hi` (clamped) from `stream`, oldest first.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn queue_range(
        &self,
        session: &LoomSession,
        workspace: &str,
        stream: &str,
        lo: u64,
        hi: u64,
    ) -> Result<Vec<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Queue)? {
                Some(ns) => log::range(loom, ns, stream, lo as usize, hi as usize),
                None => Ok(Vec::new()),
            }
        })
    }

    /// The number of entries in the queue `stream`, or `0` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn queue_len(
        &self,
        session: &LoomSession,
        workspace: &str,
        stream: &str,
    ) -> Result<u64, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Queue)? {
                Some(ns) => Ok(log::len(loom, ns, stream)? as u64),
                None => Ok(0),
            }
        })
    }

    /// The queue stream names in `workspace`, sorted; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn queue_list_streams(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Queue)? {
                Some(ns) => log::list_streams(loom, ns),
                None => Ok(Vec::new()),
            }
        })
    }

    pub fn lanes_create(
        &self,
        session: &LoomSession,
        workspace: &str,
        mut lane: Lane,
    ) -> Result<Lane, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Document),
                mint_workspace_id()?,
            )?;
            lane.updated_by = lane_actor_for_update(loom, ns, &lane.updated_by)?;
            let lane = loom_lanes::create_lane(loom, ns, lane)?;
            save_loom(loom)?;
            Ok(lane)
        })
    }

    pub fn lanes_get(
        &self,
        session: &LoomSession,
        workspace: &str,
        lane_id: &str,
    ) -> Result<Option<Lane>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => loom_lanes::get_lane(loom, ns, lane_id),
                None => Ok(None),
            }
        })
    }

    pub fn lanes_get_view(
        &self,
        session: &LoomSession,
        workspace: &str,
        ticket_workspace_id: &str,
        lane_id: &str,
    ) -> Result<Option<LaneView>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => loom_lanes::get_lane(loom, ns, lane_id).map(|lane| {
                    lane.map(|lane| build_lane_view(loom, ns, ticket_workspace_id, &lane))
                }),
                None => Ok(None),
            }
        })
    }

    pub fn lanes_list(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<Lane>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => loom_lanes::list_lanes(loom, ns),
                None => Ok(Vec::new()),
            }
        })
    }

    pub fn lanes_list_views(
        &self,
        session: &LoomSession,
        workspace: &str,
        ticket_workspace_id: &str,
    ) -> Result<Vec<LaneView>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => loom_lanes::list_lanes(loom, ns).map(|lanes| {
                    lanes
                        .into_iter()
                        .map(|lane| build_lane_view(loom, ns, ticket_workspace_id, &lane))
                        .collect()
                }),
                None => Ok(Vec::new()),
            }
        })
    }

    pub fn lanes_update(
        &self,
        session: &LoomSession,
        workspace: &str,
        input: LaneUpdateInput<'_>,
    ) -> Result<Lane, LoomError> {
        self.with_session(session, |loom| apply_lanes_update(loom, workspace, input))
    }

    pub fn lanes_ticket_add(
        &self,
        session: &LoomSession,
        workspace: &str,
        lane_id: &str,
        ticket_id: &str,
        placement: loom_lanes::LaneTicketPlacement<'_>,
        updated_by: &str,
    ) -> Result<Lane, LoomError> {
        self.lanes_mutate(session, workspace, lane_id, |lane, loom, ns| {
            loom_lanes::place_lane_ticket(lane, ticket_id, placement)?;
            let actor = lane_actor_for_update(loom, ns, updated_by)?;
            update_lane_metadata(lane, &actor);
            Ok(())
        })
    }

    pub fn lanes_ticket_remove(
        &self,
        session: &LoomSession,
        workspace: &str,
        lane_id: &str,
        ticket_id: &str,
        updated_by: &str,
    ) -> Result<Lane, LoomError> {
        self.lanes_mutate(session, workspace, lane_id, |lane, loom, ns| {
            lane.lane_tickets
                .retain(|lane_ticket| lane_ticket.ticket_id != ticket_id);
            if lane.active_ticket_id.as_deref() == Some(ticket_id) {
                lane.active_ticket_id = None;
            }
            let actor = lane_actor_for_update(loom, ns, updated_by)?;
            update_lane_metadata(lane, &actor);
            Ok(())
        })
    }

    pub fn lanes_ticket_transfer(
        &self,
        session: &LoomSession,
        workspace: &str,
        source_lane_id: &str,
        target_lane_id: &str,
        ticket_id: &str,
        updated_by: &str,
    ) -> Result<Lane, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Document),
                mint_workspace_id()?,
            )?;
            let actor = lane_actor_for_update(loom, ns, updated_by)?;
            let (_, target) = loom_lanes::transfer_assignment_lane_ticket(
                loom,
                ns,
                source_lane_id,
                target_lane_id,
                ticket_id,
                now_ms(),
                &actor,
            )?;
            save_loom(loom)?;
            Ok(target)
        })
    }

    pub fn lanes_closeout(
        &self,
        session: &LoomSession,
        workspace: &str,
        input: LaneCloseoutInput<'_>,
    ) -> Result<Lane, LoomError> {
        if input.comment_body.trim().is_empty() {
            return Err(LoomError::invalid(
                "lane closeout requires a non-empty ticket comment body",
            ));
        }
        self.lanes_mutate(session, workspace, input.lane_id, |lane, loom, ns| {
            loom_tickets::add_ticket_comment(
                loom,
                ns,
                loom_tickets::TicketCommentRequest {
                    workspace_id: input.ticket_workspace_id,
                    ticket_id: input.ticket_id,
                    comment_id: None,
                    comment_type: Some(input.comment_type),
                    body: input.comment_body,
                    evidence: input.evidence,
                    expected_root: input.expected_root,
                },
            )?;
            lane.status_report = input.status_report.to_string();
            let actor = lane_actor_for_update(loom, ns, input.updated_by)?;
            update_lane_metadata(lane, &actor);
            Ok(())
        })
    }

    pub fn lanes_cleanup(
        &self,
        session: &LoomSession,
        workspace: &str,
        lane_id: Option<&str>,
        apply: bool,
        updated_by: &str,
    ) -> Result<Vec<LaneCleanupReport>, LoomError> {
        if apply {
            self.with_session(session, |loom| {
                let ns = loom.registry_mut().ensure_for_write(
                    &ns_selector(workspace, FacetKind::Document),
                    mint_workspace_id()?,
                )?;
                let mut reports = Vec::new();
                for mut lane in cleanup_target_lanes(loom, ns, lane_id)? {
                    let view = build_lane_view(loom, ns, &ns.to_string(), &lane);
                    let terminal_ids = loom_lanes::terminal_lane_ticket_ids(&view.lane_tickets);
                    let (remaining_count, status_counts) =
                        cleanup_remaining_summary(&view, &terminal_ids);
                    let removed = loom_lanes::remove_lane_tickets(&mut lane, &terminal_ids);
                    if !removed.is_empty() {
                        let actor = lane_actor_for_update(loom, ns, updated_by)?;
                        update_lane_metadata(&mut lane, &actor);
                        loom_lanes::put_lane(loom, ns, lane)?;
                    }
                    reports.push(LaneCleanupReport {
                        lane_id: view.lane_id,
                        would_remove: Vec::new(),
                        removed,
                        remaining_count,
                        status_counts,
                    });
                }
                save_loom(loom)?;
                Ok(reports)
            })
        } else {
            self.with_session(session, |loom| {
                let Some(ns) = read_ns(loom, workspace, FacetKind::Document)? else {
                    return Ok(Vec::new());
                };
                let mut reports = Vec::new();
                for lane in cleanup_target_lanes(loom, ns, lane_id)? {
                    let view = build_lane_view(loom, ns, &ns.to_string(), &lane);
                    let terminal_ids = loom_lanes::terminal_lane_ticket_ids(&view.lane_tickets);
                    let (remaining_count, status_counts) =
                        cleanup_remaining_summary(&view, &terminal_ids);
                    reports.push(LaneCleanupReport {
                        lane_id: view.lane_id,
                        would_remove: terminal_ids,
                        removed: Vec::new(),
                        remaining_count,
                        status_counts,
                    });
                }
                Ok(reports)
            })
        }
    }

    pub fn lanes_delete(
        &self,
        session: &LoomSession,
        workspace: &str,
        lane_id: &str,
        updated_by: &str,
    ) -> Result<Lane, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Document),
                mint_workspace_id()?,
            )?;
            let actor = lane_actor_for_update(loom, ns, updated_by)?;
            let lane = loom_lanes::delete_lane(loom, ns, lane_id, now_ms(), &actor)?;
            save_loom(loom)?;
            Ok(lane)
        })
    }

    fn lanes_mutate<F>(
        &self,
        session: &LoomSession,
        workspace: &str,
        lane_id: &str,
        mutate: F,
    ) -> Result<Lane, LoomError>
    where
        F: FnOnce(&mut Lane, &mut Loom<FileStore>, WorkspaceId) -> Result<(), LoomError>,
    {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Document),
                mint_workspace_id()?,
            )?;
            let mut lane = loom_lanes::get_lane(loom, ns, lane_id)?
                .ok_or_else(|| LoomError::new(Code::NotFound, "lane not found"))?;
            mutate(&mut lane, loom, ns)?;
            let lane = loom_lanes::put_lane(loom, ns, lane)?;
            save_loom(loom)?;
            Ok(lane)
        })
    }

    /// The next sequence the named consumer should read from `stream`; `0` when none is stored or the
    /// workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn consumer_position(
        &self,
        session: &LoomSession,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
    ) -> Result<u64, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Queue)? {
                Some(ns) => log::consumer_position(loom, ns, stream, consumer_id),
                None => Ok(0),
            }
        })
    }

    /// Read up to `max` entries from the consumer's stored next sequence, oldest first; does not
    /// advance.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn consumer_read(
        &self,
        session: &LoomSession,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
        max: u64,
    ) -> Result<Vec<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Queue)? {
                Some(ns) => log::consumer_read(loom, ns, stream, consumer_id, max as usize),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Advance the named consumer's next sequence to `next_seq` (monotonic; rejects backward) and
    /// persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, backward movement, an rng failure, or a save
    /// failure.
    pub fn consumer_advance(
        &self,
        session: &LoomSession,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
        next_seq: u64,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Queue),
                mint_workspace_id()?,
            )?;
            log::consumer_advance(loom, ns, stream, consumer_id, next_seq)?;
            save_loom(loom)
        })
    }

    /// Set the named consumer's next sequence to `next_seq` (may move backward) and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an rng failure, or a save failure.
    pub fn consumer_reset(
        &self,
        session: &LoomSession,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
        next_seq: u64,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Queue),
                mint_workspace_id()?,
            )?;
            log::consumer_reset(loom, ns, stream, consumer_id, next_seq)?;
            save_loom(loom)
        })
    }

    /// Store `text` as the UTF-8 document at `id` in `collection`, ensuring the workspace on first
    /// write, and persist. The optional `expected_entity_tag` guards replacement against the
    /// current document entity tag. Returns canonical CBOR `[digest, entity_tag]`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed `expected_entity_tag`, a compare-and-swap
    /// mismatch, or an engine/save failure.
    pub fn document_put_text(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        id: &str,
        text: &str,
        expected_entity_tag: Option<&str>,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            publish_document_put_text(
                loom,
                workspace,
                collection,
                id,
                text,
                expected_entity_tag,
                |_| exec_apply_candidate_save_hook(&self.path),
            )
        })
    }

    /// The UTF-8 document text at `id` as canonical CBOR `[text, digest, entity_tag]`, or `None`
    /// when the id or workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, `DOCUMENT_NOT_TEXT` when the stored bytes are not
    /// valid UTF-8, or an engine failure.
    pub fn document_get_text(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => match document_get_text(loom, ns, collection, id)? {
                    Some(DocumentText {
                        text,
                        digest,
                        entity_tag,
                    }) => Ok(Some(loom_wire::document::text_result_to_cbor(
                        &text,
                        &digest.to_string(),
                        &entity_tag,
                    )?)),
                    None => Ok(None),
                },
                None => Ok(None),
            }
        })
    }

    /// Store `bytes` as the document at `id` in `collection`, ensuring the workspace on first
    /// write, and persist. The optional `expected_entity_tag` guards replacement against the
    /// current document entity tag. Returns canonical CBOR `[digest, entity_tag]`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed `expected_entity_tag`, a compare-and-swap
    /// mismatch, or an engine/save failure.
    pub fn document_put_binary(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        id: &str,
        bytes: &[u8],
        expected_entity_tag: Option<&str>,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            publish_document_put_binary(
                loom,
                workspace,
                collection,
                id,
                bytes.to_vec(),
                expected_entity_tag,
                |_| exec_apply_candidate_save_hook(&self.path),
            )
        })
    }

    /// The document bytes at `id` as canonical CBOR `[bytes, digest, entity_tag]`, or `None` when
    /// the id or workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn document_get_binary(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => match document_get_binary(loom, ns, collection, id)? {
                    Some(DocumentBinary {
                        bytes,
                        digest,
                        entity_tag,
                    }) => Ok(Some(loom_wire::document::binary_result_to_cbor(
                        &bytes,
                        &digest.to_string(),
                        &entity_tag,
                    )?)),
                    None => Ok(None),
                },
                None => Ok(None),
            }
        })
    }

    /// The encoded document collection for `collection` as canonical CBOR (the `[id, bytes]` map); an
    /// empty collection when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn document_list_binary(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => document_list_binary(loom, ns, collection),
                None => Ok(Collection::new().encode()),
            }
        })
    }

    /// `document.put` plus the MCP substrate reference-index overlay, in one write/save unit (backs the
    /// remote MCP `document_put`). The overlay runs before the save, so a failure leaves neither applied.
    pub fn document_put_binary_indexed(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        id: &str,
        doc: Vec<u8>,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Document),
                mint_workspace_id()?,
            )?;
            loom_reference::put_document_indexed(loom, ns, collection, id, doc)
        })
    }

    /// `document.delete` plus the reference-index overlay; returns whether the document existed.
    pub fn document_delete_indexed(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => {
                    let present =
                        loom_reference::delete_document_indexed(loom, ns, collection, id)?;
                    Ok(present)
                }
                None => Ok(false),
            }
        })
    }

    /// `document.replace_text` plus the reference-index overlay; returns `(replacements, digest, entity_tag)`.
    pub fn document_replace_text_indexed(
        &self,
        session: &LoomSession,
        args: DocumentReplaceTextArgs<'_>,
    ) -> Result<(u64, String, String), LoomError> {
        self.with_session(session, |loom| {
            let ns = read_ns(loom, args.workspace, FacetKind::Document)?
                .ok_or_else(|| LoomError::new(Code::NotFound, "document not found"))?;
            let outcome = loom_reference::replace_text_indexed(
                loom,
                ns,
                args.collection,
                args.id,
                args.find,
                args.replace,
                args.replace_all,
                args.base_digest,
            )?;
            save_loom(loom)?;
            let digest = Digest::parse(&outcome.digest)?;
            let entity_tag = loom_core::document_entity_tag_string_from_digest(digest);
            Ok((outcome.replacements, outcome.digest, entity_tag))
        })
    }

    /// Declare a document index `name` over the dotted `path` in `collection`, and persist.
    pub fn document_index_create(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        name: &str,
        path: &str,
        unique: bool,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Document),
                mint_workspace_id()?,
            )?;
            let index = loom_core::document::DocumentIndexDef::new(
                name,
                loom_core::document::DocumentFieldPath::dotted(path)?,
                unique,
            )?;
            loom_core::document::doc_create_index(loom, ns, collection, index)?;
            save_loom(loom)
        })
    }

    pub fn document_index_create_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        declaration_json: &[u8],
    ) -> Result<(), LoomError> {
        let value = serde_json::from_slice::<serde_json::Value>(declaration_json)
            .map_err(|err| LoomError::invalid(err.to_string()))?;
        let declaration = loom_core::document_index_declaration_from_json(&value)?;
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Document),
                mint_workspace_id()?,
            )?;
            loom_core::document::doc_create_index_declaration(loom, ns, collection, declaration)?;
            save_loom(loom)
        })
    }

    /// Drop the document index `name` in `collection`; returns whether it was present. Persists on a
    /// change.
    pub fn document_index_drop(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        name: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => {
                    let dropped = loom_core::document::doc_drop_index(loom, ns, collection, name)?;
                    if dropped {
                        save_loom(loom)?;
                    }
                    Ok(dropped)
                }
                None => Ok(false),
            }
        })
    }

    /// Rebuild the document index `name` in `collection` from the stored documents, and persist.
    pub fn document_index_rebuild(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        name: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Document),
                mint_workspace_id()?,
            )?;
            loom_core::document::doc_rebuild_index(loom, ns, collection, name)?;
            save_loom(loom)
        })
    }

    /// The declared document indexes of `collection` as canonical JSON bytes; empty when the workspace
    /// is absent.
    pub fn document_index_list_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let indexes = match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => loom_core::document::doc_list_index_declarations(loom, ns, collection)?,
                None => Vec::new(),
            };
            document_json_to_bytes(&loom_core::document::document_index_declarations_json(
                indexes,
            ))
        })
    }

    /// The document index readiness statuses of `collection` as canonical JSON bytes; empty when the
    /// workspace is absent.
    pub fn document_index_status_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let statuses = match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => loom_core::document::doc_index_statuses(loom, ns, collection)?,
                None => Vec::new(),
            };
            document_json_to_bytes(&loom_core::document::document_index_statuses_json(statuses))
        })
    }

    /// The ids matching `value_json` on index `index` of `collection`, as a canonical JSON array of ids;
    /// empty when the workspace is absent.
    pub fn document_find_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        index: &str,
        value_json: &[u8],
    ) -> Result<Vec<u8>, LoomError> {
        let value: serde_json::Value = serde_json::from_slice(value_json)
            .map_err(|e| LoomError::invalid(format!("document find value must be json: {e}")))?;
        let value = loom_core::document::document_index_value_from_json(&value)?;
        self.with_session(session, |loom| {
            let ids = match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => loom_core::document::doc_find(loom, ns, collection, index, &value)?,
                None => Vec::new(),
            };
            document_json_to_bytes(&serde_json::Value::Array(
                ids.into_iter().map(serde_json::Value::String).collect(),
            ))
        })
    }

    /// The result of `query_json` over `collection`, as canonical JSON bytes.
    pub fn document_query_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        query_json: &[u8],
    ) -> Result<Vec<u8>, LoomError> {
        let query: serde_json::Value = serde_json::from_slice(query_json)
            .map_err(|e| LoomError::invalid(format!("document query must be json: {e}")))?;
        let query = loom_core::document::document_query_from_json(&query)?;
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => {
                    let result = loom_core::document::doc_query(loom, ns, collection, &query)?;
                    document_json_to_bytes(&loom_core::document::document_query_result_json(result))
                }
                None => Err(LoomError::new(
                    Code::NotFound,
                    "document collection not found",
                )),
            }
        })
    }

    /// Delete the document at `id` in `collection`; returns whether it was present. Persists on a
    /// change.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine or save failure.
    pub fn document_delete(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => {
                    let present = doc_delete(loom, ns, collection, id)?;
                    if present {
                        save_loom(loom)?;
                    }
                    Ok(present)
                }
                None => Ok(false),
            }
        })
    }

    /// Delete `collection` and its structured document roots; returns whether it existed. Persists on
    /// a change.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine or save failure.
    pub fn document_delete_collection(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => {
                    let present = doc_delete_collection(loom, ns, collection)?;
                    if present {
                        save_loom(loom)?;
                    }
                    Ok(present)
                }
                None => Ok(false),
            }
        })
    }

    /// The document collection names in `workspace`, sorted; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn document_list_collections(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Document)? {
                Some(ns) => doc_list_collections(loom, ns),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Write `content` to `path` in the working tree of `workspace` (create-or-replace), ensuring the
    /// workspace on first write, and persist. `mode` 0 uses the default file mode.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a missing parent, an rng failure, or a save
    /// failure.
    pub fn write_file(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
        content: &[u8],
        mode: u32,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Files),
                mint_workspace_id()?,
            )?;
            loom.write_file(ns, path, content, mode)?;
            save_loom(loom)
        })
    }

    /// Import the host filesystem tree at `src_path` into `workspace`'s Files facet, returning the
    /// canonical CBOR of the resulting [`loom_interchange::ImportReport`]. `commit` snapshots the import
    /// onto the workspace head; `dry_run` plans without writing (and does not save).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an import failure (I/O, authorization, invalid src).
    pub fn import_fs(
        &self,
        session: &LoomSession,
        workspace: &str,
        src_path: &str,
        author: Option<&str>,
        message: Option<&str>,
        commit: bool,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Files),
                mint_workspace_id()?,
            )?;
            let mut options = loom_interchange_io::FsImportOptions::new(src_path);
            if let Some(author) = author {
                options.author = author.to_string();
            }
            if let Some(message) = message {
                options.message = message.to_string();
            }
            options.commit = commit;
            options.dry_run = dry_run;
            let report =
                loom_interchange_io::import_fs(loom, ns, std::path::Path::new(src_path), &options)?;
            if !dry_run {
                save_loom(loom)?;
            }
            Ok(import_report_to_cbor(&report))
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn import_table_csv(
        &self,
        session: &LoomSession,
        workspace: &str,
        source_scope: &str,
        csv_payload: &[u8],
        database: &str,
        table: &str,
        schema: &str,
        primary_key: &str,
        mode: &str,
        commit: bool,
        author: Option<&str>,
        message: Option<&str>,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        let columns = parse_table_csv_schema(schema)?;
        let primary_key = parse_table_csv_primary_key(primary_key)?;
        let mode = parse_table_csv_import_mode(mode)?;
        self.with_session(session, |loom| {
            let ns = if dry_run {
                resolve_ns_read(loom, workspace)?
            } else {
                loom.registry_mut().ensure_for_write(
                    &ns_selector(workspace, FacetKind::Sql),
                    mint_workspace_id()?,
                )?
            };
            let mut options = loom_interchange_io::TableCsvImportOptions::new(
                source_scope,
                database,
                table,
                columns,
                primary_key,
            );
            options.mode = mode;
            options.commit = commit;
            if let Some(author) = author {
                options.author = author.to_string();
            }
            if let Some(message) = message {
                options.message = message.to_string();
            }
            options.dry_run = dry_run;
            let report =
                loom_interchange_io::import_table_csv_bytes(loom, ns, csv_payload, &options)?;
            if !dry_run {
                save_loom(loom)?;
            }
            Ok(import_report_to_cbor(&report))
        })
    }

    pub fn import_redmine(
        &self,
        session: &LoomSession,
        workspace: &str,
        profile: &str,
        source_scope: &str,
        snapshot_payload: &[u8],
        field_policy: &str,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        let field_policy = parse_ticket_import_field_policy(field_policy)?;
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let input = retained_import_input_from_bytes(loom, source_scope, snapshot_payload);
            let report = loom_interchange_io::import_redmine_bytes_with_field_policy(
                loom,
                ns,
                profile,
                source_scope,
                snapshot_payload,
                dry_run,
                field_policy,
            )?;
            let persisted = persist_profile_import_artifacts(loom, ns, profile, &input, &report)?;
            if !dry_run && (report.operations_applied > 0 || persisted) {
                save_loom(loom)?;
            }
            Ok(import_report_to_cbor(&report))
        })
    }

    pub fn import_asana(
        &self,
        session: &LoomSession,
        workspace: &str,
        profile: &str,
        source_scope: &str,
        snapshot_payload: &[u8],
        field_policy: &str,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        let field_policy = parse_ticket_import_field_policy(field_policy)?;
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let input = retained_import_input_from_bytes(loom, source_scope, snapshot_payload);
            let report = loom_interchange_io::import_asana_bytes_with_field_policy(
                loom,
                ns,
                profile,
                source_scope,
                snapshot_payload,
                dry_run,
                field_policy,
            )?;
            let persisted = persist_profile_import_artifacts(loom, ns, profile, &input, &report)?;
            if !dry_run && (report.operations_applied > 0 || persisted) {
                save_loom(loom)?;
            }
            Ok(import_report_to_cbor(&report))
        })
    }

    pub fn import_jira(
        &self,
        session: &LoomSession,
        workspace: &str,
        profile: &str,
        source_scope: &str,
        snapshot_payload: &[u8],
        field_policy: &str,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        let field_policy = parse_ticket_import_field_policy(field_policy)?;
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let input = retained_import_input_from_bytes(loom, source_scope, snapshot_payload);
            let report = loom_interchange_io::import_jira_bytes_with_field_policy(
                loom,
                ns,
                profile,
                source_scope,
                snapshot_payload,
                dry_run,
                field_policy,
            )?;
            let persisted = persist_profile_import_artifacts(loom, ns, profile, &input, &report)?;
            if !dry_run && (report.operations_applied > 0 || persisted) {
                save_loom(loom)?;
            }
            Ok(import_report_to_cbor(&report))
        })
    }

    pub fn import_confluence(
        &self,
        session: &LoomSession,
        workspace: &str,
        profile: &str,
        source_scope: &str,
        snapshot_payload: &[u8],
        default_space: &str,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let input = retained_import_input_from_bytes(loom, source_scope, snapshot_payload);
            let report = loom_interchange_io::import_confluence_bytes(
                loom,
                ns,
                profile,
                source_scope,
                default_space,
                snapshot_payload,
                dry_run,
            )?;
            let persisted = persist_profile_import_artifacts(loom, ns, profile, &input, &report)?;
            if !dry_run && (report.operations_applied > 0 || persisted) {
                save_loom(loom)?;
            }
            Ok(import_report_to_cbor(&report))
        })
    }

    pub fn import_slack(
        &self,
        session: &LoomSession,
        workspace: &str,
        profile: &str,
        source_scope: &str,
        snapshot_payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let input = retained_import_input_from_bytes(loom, source_scope, snapshot_payload);
            let report = loom_interchange_io::import_slack_bytes(
                loom,
                ns,
                profile,
                source_scope,
                snapshot_payload,
                dry_run,
            )?;
            let persisted = persist_profile_import_artifacts(loom, ns, profile, &input, &report)?;
            if !dry_run && (report.operations_applied > 0 || persisted) {
                save_loom(loom)?;
            }
            Ok(import_report_to_cbor(&report))
        })
    }

    pub fn import_drive(
        &self,
        session: &LoomSession,
        workspace: &str,
        profile: &str,
        source_scope: &str,
        archive_payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let input = retained_import_input_from_bytes(loom, source_scope, archive_payload);
            let report = loom_interchange_io::import_drive_archive_bytes(
                loom,
                ns,
                profile,
                source_scope,
                archive_payload,
                dry_run,
            )?;
            let persisted = persist_profile_import_artifacts(loom, ns, profile, &input, &report)?;
            if !dry_run && (report.operations_applied > 0 || persisted) {
                save_loom(loom)?;
            }
            Ok(import_report_to_cbor(&report))
        })
    }

    pub fn import_markdown(
        &self,
        session: &LoomSession,
        workspace: &str,
        profile: &str,
        source_scope: &str,
        archive_payload: &[u8],
        space: &str,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let input = retained_import_input_from_bytes(loom, source_scope, archive_payload);
            let report = loom_interchange_io::import_markdown_archive_bytes(
                loom,
                ns,
                profile,
                source_scope,
                archive_payload,
                space,
                dry_run,
            )?;
            let persisted = persist_profile_import_artifacts(loom, ns, profile, &input, &report)?;
            if !dry_run && (report.operations_applied > 0 || persisted) {
                save_loom(loom)?;
            }
            Ok(import_report_to_cbor(&report))
        })
    }

    pub fn import_notion(
        &self,
        session: &LoomSession,
        workspace: &str,
        profile: &str,
        source_scope: &str,
        snapshot_payload: &[u8],
        default_space: &str,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let input = retained_import_input_from_bytes(loom, source_scope, snapshot_payload);
            let report = loom_interchange_io::import_notion_bytes(
                loom,
                ns,
                profile,
                source_scope,
                default_space,
                snapshot_payload,
                dry_run,
            )?;
            let persisted = persist_profile_import_artifacts(loom, ns, profile, &input, &report)?;
            if !dry_run && (report.operations_applied > 0 || persisted) {
                save_loom(loom)?;
            }
            Ok(import_report_to_cbor(&report))
        })
    }

    /// Export `workspace`'s Files facet (optionally at `revision`) to the host directory `dst_path`,
    /// returning the canonical CBOR of the resulting [`loom_interchange::ExportReport`]. `dry_run` plans
    /// without writing to the host.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace or an export failure (I/O, authorization).
    pub fn export_fs(
        &self,
        session: &LoomSession,
        workspace: &str,
        dst_path: &str,
        revision: Option<&str>,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let mut options = loom_interchange_io::FsExportOptions::new(workspace);
            options.dry_run = dry_run;
            options.revision = revision.map(str::to_string);
            let report =
                loom_interchange_io::export_fs(loom, ns, std::path::Path::new(dst_path), &options)?;
            Ok(export_report_to_cbor(&report))
        })
    }

    /// Open a byte-transfer import staging area for `kind` (`specs/0067` §17.3) and return the opaque
    /// `TransferId` bytes. The client streams the payload with [`LocalLoomClient::transfer_import_write`]
    /// and applies it with [`LocalLoomClient::transfer_import_finish`]. v1 supports the archive family
    /// (`tar`/`tar-zstd`/`tar-gzip`/`zip`/`gzip`) and `car`; columnar `arrow-ipc`/`parquet` and `fs-tree`
    /// are not yet wired to a store-backed import codec and are rejected `Unsupported`.
    ///
    /// # Errors
    /// [`LoomError`] for an unknown session, an unknown/unsupported kind, or an RNG failure.
    pub fn transfer_import_open(
        &self,
        session: &LoomSession,
        workspace: &str,
        kind: &str,
        _opts: &[u8],
    ) -> Result<Vec<u8>, LoomError> {
        use loom_interchange_io::transfer::{StagingLimits, TransferKind, TransferStaging};
        let kind = TransferKind::parse(kind)?;
        // Reject a kind with no store-backed import codec before reserving any staging area.
        transfer_kind_import_supported(kind)?;
        let algo = self.with_session(session, |loom| Ok(loom.store().digest_algo()))?;
        let mut id = vec![0u8; 16];
        fill_random(&mut id)?;
        let entry = TransferEntry {
            staging: TransferStaging::open(kind, algo, StagingLimits::default()),
            workspace: workspace.to_string(),
            commit_report: None,
        };
        self.transfers
            .lock()
            .expect("transfers lock")
            .insert(id.clone(), entry);
        Ok(id)
    }

    /// Append one bounded chunk at monotonic `seq` to the transfer's staging area, returning the
    /// canonical `TransferAccept` (`[accepted_bytes, credit]`). Re-sending an accepted `seq` is an
    /// idempotent no-op; an optional per-chunk `digest` is verified early.
    ///
    /// # Errors
    /// [`LoomError`] for an unknown transfer, an out-of-order/oversized chunk, a staging-limit
    /// overflow, a chunk-digest mismatch, or a write after finalization.
    pub fn transfer_import_write(
        &self,
        _session: &LoomSession,
        transfer: &[u8],
        chunk: &[u8],
        seq: u64,
        digest: Option<&Digest>,
    ) -> Result<Vec<u8>, LoomError> {
        let mut transfers = self.transfers.lock().expect("transfers lock");
        let entry = transfers
            .get_mut(transfer)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown transfer handle"))?;
        let accept = entry
            .staging
            .write(seq, chunk, digest, std::time::Instant::now())?;
        Ok(loom_wire::transfer::transfer_accept_to_cbor(
            accept.accepted_bytes,
            accept.credit,
        ))
    }

    /// Validate the staged bytes against `final_digest`, apply the interchange under the write
    /// authority (honoring `commit`/`dry_run`), and return the canonical
    /// `loom.interchange.import-report.v1`. Finalize-once: a replayed `finish` returns the cached
    /// report without reapplying the import.
    ///
    /// # Errors
    /// [`LoomError`] for an unknown transfer, a final-digest mismatch, or an interchange failure.
    pub fn transfer_import_finish(
        &self,
        session: &LoomSession,
        transfer: &[u8],
        commit: bool,
        dry_run: bool,
        final_digest: &Digest,
    ) -> Result<Vec<u8>, LoomError> {
        // Validate and snapshot the staged bytes under the transfers lock, then release it before
        // taking the session lock to apply the interchange (avoids holding two locks at once).
        let (kind, workspace, bytes) = {
            let mut transfers = self.transfers.lock().expect("transfers lock");
            let entry = transfers
                .get_mut(transfer)
                .ok_or_else(|| LoomError::new(Code::NotFound, "unknown transfer handle"))?;
            if let Some(report) = &entry.commit_report {
                return Ok(report.clone());
            }
            entry.staging.validate_final(final_digest)?;
            (
                entry.staging.kind(),
                entry.workspace.clone(),
                entry.staging.bytes().to_vec(),
            )
        };
        let report = self.with_session(session, |loom| {
            apply_transfer_import(loom, &workspace, kind, &bytes, commit, dry_run)
        })?;
        // Cache the report for finalize-once (only on a real commit; a dry run may be re-attempted).
        if !dry_run
            && let Some(entry) = self
                .transfers
                .lock()
                .expect("transfers lock")
                .get_mut(transfer)
        {
            entry.commit_report = Some(report.clone());
        }
        Ok(report)
    }

    /// Discard a transfer's staging area and release its handle (`cancel`/lease expiry). Cancelling
    /// an unknown transfer is a no-op.
    pub fn transfer_import_cancel(
        &self,
        _session: &LoomSession,
        transfer: &[u8],
    ) -> Result<(), LoomError> {
        self.transfers
            .lock()
            .expect("transfers lock")
            .remove(transfer);
        Ok(())
    }

    /// Export `workspace`'s Files facet (optionally at `revision`) as a `kind` payload, returning the
    /// full byte stream. The caller chunks it into the `transfer_export` stream and writes the local
    /// destination path (`specs/0067` §17.4). v1 supports the archive family + `car`.
    ///
    /// # Errors
    /// [`LoomError`] for an unknown session/workspace, an unsupported kind, or an export failure.
    pub fn transfer_export_bytes(
        &self,
        session: &LoomSession,
        workspace: &str,
        kind: &str,
        revision: Option<&str>,
        _opts: &[u8],
    ) -> Result<Vec<u8>, LoomError> {
        use loom_interchange_io::transfer::TransferKind;
        let kind = TransferKind::parse(kind)?;
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            match kind {
                TransferKind::Car => {
                    if revision.is_some() {
                        return Err(LoomError::new(
                            Code::InvalidArgument,
                            "car export does not support a revision selector",
                        ));
                    }
                    let options = loom_interchange_io::CarExportOptions::new(workspace);
                    Ok(loom_interchange_io::export_car_bytes(loom, ns, &options)?.bytes)
                }
                _ => {
                    let archive_kind = transfer_kind_to_archive(kind)?;
                    let mut options = loom_interchange_io::ArchiveExportOptions::new(workspace);
                    options.revision = revision.map(str::to_string);
                    Ok(
                        loom_interchange_io::export_archive_bytes(
                            loom,
                            ns,
                            archive_kind,
                            &options,
                        )?
                        .bytes,
                    )
                }
            }
        })
    }

    /// Async form of [`LocalLoomClient::import_fs`] as an immediate-complete [`Task`]: the synchronous
    /// import runs on the first `task_poll` and its result is the same canonical-CBOR `ImportReport` bytes.
    /// There is no background execution, progress, or cancellation beyond what the task model provides.
    pub fn import_fs_async(
        &self,
        session: &LoomSession,
        workspace: &str,
        src_path: &str,
        author: Option<&str>,
        message: Option<&str>,
        commit: bool,
        dry_run: bool,
    ) -> Task {
        self.spawn_task(TaskWork::ImportFsAsync {
            session: session.clone(),
            workspace: workspace.to_string(),
            src_path: src_path.to_string(),
            author: author.map(str::to_string),
            message: message.map(str::to_string),
            commit,
            dry_run,
        })
    }

    /// Async form of [`LocalLoomClient::export_fs`] as an immediate-complete [`Task`]: the synchronous
    /// export runs on the first `task_poll` and its result is the same canonical-CBOR `ExportReport` bytes.
    pub fn export_fs_async(
        &self,
        session: &LoomSession,
        workspace: &str,
        dst_path: &str,
        revision: Option<&str>,
        dry_run: bool,
    ) -> Task {
        self.spawn_task(TaskWork::ExportFsAsync {
            session: session.clone(),
            workspace: workspace.to_string(),
            dst_path: dst_path.to_string(),
            revision: revision.map(str::to_string),
            dry_run,
        })
    }

    /// Import the archive at `src_path` (`kind` = zip/tar/tar-zstd/tar-gzip/gzip) into `workspace`'s Files
    /// facet, returning the canonical CBOR of the [`loom_interchange_io::ArchiveImportResult`]. `dry_run`
    /// plans without writing.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an unknown archive kind, or an import failure.
    #[expect(
        clippy::too_many_arguments,
        reason = "matches the generated Archive IDL signature"
    )]
    pub fn archive_import(
        &self,
        session: &LoomSession,
        workspace: &str,
        src_path: &str,
        kind: &str,
        gzip_output_path: Option<&str>,
        commit: bool,
        author: Option<&str>,
        message: Option<&str>,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        let kind = archive_kind_from_str(kind)?;
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Files),
                mint_workspace_id()?,
            )?;
            let mut options = loom_interchange_io::ArchiveImportOptions::new(src_path);
            options.gzip_output_path = gzip_output_path.map(str::to_string);
            options.commit = commit;
            if let Some(author) = author {
                options.author = author.to_string();
            }
            if let Some(message) = message {
                options.message = message.to_string();
            }
            options.dry_run = dry_run;
            let result = loom_interchange_io::import_archive(
                loom,
                ns,
                std::path::Path::new(src_path),
                kind,
                &options,
            )?;
            if !dry_run {
                save_loom(loom)?;
            }
            result.encode()
        })
    }

    /// Export `workspace`'s Files facet (optionally at `revision`) as an archive of `kind` to `dst_path`,
    /// returning the canonical CBOR of the [`loom_interchange_io::ArchiveExportResult`].
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace, an unknown archive kind, or an export failure.
    pub fn archive_export(
        &self,
        session: &LoomSession,
        workspace: &str,
        dst_path: &str,
        kind: &str,
        revision: Option<&str>,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        let kind = archive_kind_from_str(kind)?;
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let mut options = loom_interchange_io::ArchiveExportOptions::new(workspace);
            options.dry_run = dry_run;
            options.revision = revision.map(str::to_string);
            let result = loom_interchange_io::export_archive(
                loom,
                ns,
                std::path::Path::new(dst_path),
                kind,
                &options,
            )?;
            Ok(archive_export_result_to_cbor(&result))
        })
    }

    /// Immediate-complete [`Task`] form of [`LocalLoomClient::archive_import`] (runs on first `task_poll`).
    #[expect(
        clippy::too_many_arguments,
        reason = "matches the generated Archive IDL signature"
    )]
    pub fn archive_import_async(
        &self,
        session: &LoomSession,
        workspace: &str,
        src_path: &str,
        kind: &str,
        gzip_output_path: Option<&str>,
        commit: bool,
        author: Option<&str>,
        message: Option<&str>,
        dry_run: bool,
    ) -> Task {
        self.spawn_task(TaskWork::ArchiveImportAsync {
            session: session.clone(),
            workspace: workspace.to_string(),
            src_path: src_path.to_string(),
            kind: kind.to_string(),
            gzip_output_path: gzip_output_path.map(str::to_string),
            commit,
            author: author.map(str::to_string),
            message: message.map(str::to_string),
            dry_run,
        })
    }

    /// Immediate-complete [`Task`] form of [`LocalLoomClient::archive_export`] (runs on first `task_poll`).
    pub fn archive_export_async(
        &self,
        session: &LoomSession,
        workspace: &str,
        dst_path: &str,
        kind: &str,
        revision: Option<&str>,
        dry_run: bool,
    ) -> Task {
        self.spawn_task(TaskWork::ArchiveExportAsync {
            session: session.clone(),
            workspace: workspace.to_string(),
            dst_path: dst_path.to_string(),
            kind: kind.to_string(),
            revision: revision.map(str::to_string),
            dry_run,
        })
    }

    /// Import the CAR file at `src_path` into the store (store-wide, no workspace argument), returning the
    /// canonical CBOR of the [`loom_interchange_io::CarImportResult`]. `dry_run` plans without writing.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or a CAR import failure.
    pub fn car_import(
        &self,
        session: &LoomSession,
        src_path: &str,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let mut options = loom_interchange_io::CarImportOptions::new("car");
            options.dry_run = dry_run;
            let result =
                loom_interchange_io::import_car(loom, std::path::Path::new(src_path), &options)?;
            if !dry_run {
                save_loom(loom)?;
            }
            Ok(car_import_result_to_cbor(&result))
        })
    }

    /// Export `workspace` as a CAR file to `dst_path`, returning the canonical CBOR of the
    /// [`loom_interchange_io::CarExportResult`].
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace or a CAR export failure.
    pub fn car_export(
        &self,
        session: &LoomSession,
        workspace: &str,
        dst_path: &str,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let mut options = loom_interchange_io::CarExportOptions::new(workspace);
            options.dry_run = dry_run;
            let result = loom_interchange_io::export_car(
                loom,
                ns,
                std::path::Path::new(dst_path),
                &options,
            )?;
            Ok(car_export_result_to_cbor(&result))
        })
    }

    /// Immediate-complete [`Task`] form of [`LocalLoomClient::car_import`] (runs on first `task_poll`).
    pub fn car_import_async(&self, session: &LoomSession, src_path: &str, dry_run: bool) -> Task {
        self.spawn_task(TaskWork::CarImportAsync {
            session: session.clone(),
            src_path: src_path.to_string(),
            dry_run,
        })
    }

    /// Immediate-complete [`Task`] form of [`LocalLoomClient::car_export`] (runs on first `task_poll`).
    pub fn car_export_async(
        &self,
        session: &LoomSession,
        workspace: &str,
        dst_path: &str,
        dry_run: bool,
    ) -> Task {
        self.spawn_task(TaskWork::CarExportAsync {
            session: session.clone(),
            workspace: workspace.to_string(),
            dst_path: dst_path.to_string(),
            dry_run,
        })
    }

    /// Read the staged bytes of `path` in the working tree of `workspace`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND`) for an absent workspace or file, or for an unknown session.
    pub fn read_file(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Files))?;
            loom.read_file(ns, path)
        })
    }

    /// Stage a deletion of `path` in the working tree of `workspace`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND`) for an absent workspace, or for an unknown session or a
    /// save failure.
    pub fn remove_file(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Files))?;
            loom.remove_file(ns, path)?;
            save_loom(loom)
        })
    }

    /// Create directory `path` in `workspace`'s working tree, ensuring the workspace on first write, and
    /// persist. Idempotent if `path` is already a directory; `ALREADY_EXISTS` if it is a file; without
    /// `recursive` the parent must already exist (`NOT_FOUND`).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an invalid path, a conflict, or an engine/save failure.
    pub fn create_directory(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
        recursive: bool,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Files),
                mint_workspace_id()?,
            )?;
            loom.create_directory(ns, path, recursive)?;
            save_loom(loom)
        })
    }

    /// Remove directory `path` in `workspace`'s working tree, and persist. Without `recursive` a
    /// non-empty directory is `INVALID_ARGUMENT`; an absent directory is `NOT_FOUND`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/directory) or for an unknown session or
    /// a save failure.
    pub fn remove_directory(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
        recursive: bool,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Files))?;
            loom.remove_directory(ns, path, recursive)?;
            save_loom(loom)
        })
    }

    /// Metadata for `path` in `workspace`, as canonical CBOR `loom.fs.stat.v1` (`[path, kind, size,
    /// mode]`). A path resolving to neither a file nor a directory is `NOT_FOUND`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/path) or for an unknown session or an
    /// engine failure.
    pub fn stat(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Files))?;
            let stat = loom.stat(ns, path)?;
            loom_wire::fs::fs_stat_to_cbor(&stat)
        })
    }

    /// The immediate children of directory `path` in `workspace`, as canonical CBOR
    /// `loom.fs.dir-listing.v1` (an array of `[name, kind]`, sorted by name; root is `""` or `"/"`). A
    /// non-directory path is `NOT_FOUND`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/directory) or for an unknown session or
    /// an engine failure.
    pub fn list_directory(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Files))?;
            let entries = loom.list_directory(ns, path)?;
            loom_wire::fs::dir_listing_to_cbor(&entries)
        })
    }

    /// Append `content` to `path` in `workspace`, ensuring the workspace on first write, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an rng failure, or an engine or save failure.
    pub fn append_file(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
        content: &[u8],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Files),
                mint_workspace_id()?,
            )?;
            loom.append_file(ns, path, content)?;
            save_loom(loom)
        })
    }

    /// Read `len` bytes at `offset` from `path` in `workspace`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, or an engine failure.
    pub fn read_at(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
        offset: u64,
        len: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Files))?;
            loom.read_at(ns, path, offset, len)
        })
    }

    /// Write `content` at `offset` in `path` of `workspace`, ensuring the workspace on first write, and
    /// persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an rng failure, or an engine or save failure.
    pub fn write_at(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
        offset: u64,
        content: &[u8],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Files),
                mint_workspace_id()?,
            )?;
            loom.write_at(ns, path, offset, content)?;
            save_loom(loom)
        })
    }

    /// Resize `path` in `workspace` to `size`, ensuring the workspace on first write, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an rng failure, or an engine or save failure.
    pub fn truncate(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
        size: u64,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Files),
                mint_workspace_id()?,
            )?;
            loom.truncate_file(ns, path, size)?;
            save_loom(loom)
        })
    }

    /// Create a symlink at `link_path` pointing at `target` in `workspace`, ensuring the workspace on
    /// first write, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an rng failure, or an engine or save failure.
    pub fn symlink(
        &self,
        session: &LoomSession,
        workspace: &str,
        target: &str,
        link_path: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Files),
                mint_workspace_id()?,
            )?;
            loom.symlink(ns, target, link_path)?;
            save_loom(loom)
        })
    }

    /// Read the symlink target at `path` in `workspace`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, or an engine failure.
    pub fn read_link(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Files))?;
            loom.read_link(ns, path)
        })
    }

    /// Insert or replace node `id` with `props` in graph `name`, ensuring the workspace on first write,
    /// and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an rng failure, or an engine or save failure.
    pub fn graph_upsert_node(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
        props: Props,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Graph),
                mint_workspace_id()?,
            )?;
            graph_upsert_node(loom, ns, name, id, props)?;
            save_loom(loom)
        })
    }

    /// Read node `id`'s properties in graph `name`, or `None` when the node or graph is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn graph_get_node(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<Props>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Graph)? {
                Some(ns) => graph_get_node(loom, ns, name, id),
                None => Ok(None),
            }
        })
    }

    /// Insert or replace edge `id` from `src` to `dst` with `label` and `props` in graph `name` (both
    /// endpoints must exist), and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a missing endpoint, an rng failure, or a save
    /// failure.
    #[allow(clippy::too_many_arguments)]
    pub fn graph_upsert_edge(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
        src: &str,
        dst: &str,
        label: &str,
        props: Props,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Graph),
                mint_workspace_id()?,
            )?;
            graph_upsert_edge(loom, ns, name, id, src, dst, label, props)?;
            save_loom(loom)
        })
    }

    /// The neighbours of node `id` in graph `name`, or empty when the graph is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn graph_neighbors(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Graph)? {
                Some(ns) => graph_neighbors(loom, ns, name, id),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Remove node `id` from graph `name` and persist. `cascade` drops incident edges; without it, a
    /// node with incident edges is rejected. A no-op when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a conflict, or a save failure.
    pub fn graph_remove_node(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
        cascade: bool,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Graph)? {
                Some(ns) => {
                    graph_remove_node(loom, ns, name, id, cascade)?;
                    save_loom(loom)
                }
                None => Ok(()),
            }
        })
    }

    /// The edge `id` in graph `name`, or `None` when the edge or workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn graph_get_edge(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<Edge>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Graph)? {
                Some(ns) => graph_get_edge(loom, ns, name, id),
                None => Ok(None),
            }
        })
    }

    /// Remove edge `id` from graph `name`; returns whether it was present. Persists on a change; a
    /// no-op (`false`) when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine or save failure.
    pub fn graph_remove_edge(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Graph)? {
                Some(ns) => {
                    let present = graph_remove_edge(loom, ns, name, id)?;
                    if present {
                        save_loom(loom)?;
                    }
                    Ok(present)
                }
                None => Ok(false),
            }
        })
    }

    /// `graph.upsert_edge` plus the reference-index overlay, in one write/save unit (backs the remote MCP
    /// `graph_upsert_edge`).
    pub fn graph_upsert_edge_indexed(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
        src: &str,
        dst: &str,
        label: &str,
        props: Props,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Graph),
                mint_workspace_id()?,
            )?;
            loom_reference::upsert_graph_edge_indexed(loom, ns, name, id, src, dst, label, props)?;
            save_loom(loom)
        })
    }

    /// `graph.remove_edge` plus the reference-index overlay; returns whether the edge existed.
    pub fn graph_remove_edge_indexed(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Graph)? {
                Some(ns) => {
                    let present = loom_reference::remove_graph_edge_indexed(loom, ns, name, id)?;
                    if present {
                        save_loom(loom)?;
                    }
                    Ok(present)
                }
                None => Ok(false),
            }
        })
    }

    /// The outgoing edges of node `id` in graph `name`, or empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn graph_out_edges(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Vec<(String, Edge)>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Graph)? {
                Some(ns) => graph_out_edges(loom, ns, name, id),
                None => Ok(Vec::new()),
            }
        })
    }

    /// The incoming edges of node `id` in graph `name`, or empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn graph_in_edges(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Vec<(String, Edge)>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Graph)? {
                Some(ns) => graph_in_edges(loom, ns, name, id),
                None => Ok(Vec::new()),
            }
        })
    }

    /// The node ids reachable from `start` in graph `name` within `max_depth` (unbounded when `None`),
    /// optionally following only `via_label` edges; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn graph_reachable(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        start: &str,
        max_depth: Option<usize>,
        via_label: Option<&str>,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Graph)? {
                Some(ns) => graph_reachable(loom, ns, name, start, max_depth, via_label),
                None => Ok(Vec::new()),
            }
        })
    }

    /// A shortest directed path from `from` to `to` in graph `name` (optionally following only
    /// `via_label` edges), or `None` when there is no path or the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn graph_shortest_path(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        from: &str,
        to: &str,
        via_label: Option<&str>,
    ) -> Result<Option<Vec<String>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Graph)? {
                Some(ns) => graph_shortest_path(loom, ns, name, from, to, via_label),
                None => Ok(None),
            }
        })
    }

    /// Run a native graph query in graph `name`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, missing workspace, or engine failure.
    pub fn graph_query(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        query: &GraphQuery,
    ) -> Result<GraphQueryResult, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Graph))?;
            graph_query(loom, ns, name, query)
        })
    }

    /// Explain a native graph query in graph `name`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, missing workspace, or engine failure.
    pub fn graph_explain_query(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        query: &GraphQuery,
    ) -> Result<GraphQueryExplain, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Graph))?;
            graph_explain_query(loom, ns, name, query)
        })
    }

    /// Put `value` at the typed `key` (canonical CBOR cell bytes) in KV `collection`, ensuring the
    /// workspace on first write, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed key, an rng failure, or a save
    /// failure.
    pub fn kv_put(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let key = kv::key_from_cbor(key)?;
            let ns = loom
                .registry_mut()
                .ensure_for_write(&ns_selector(workspace, FacetKind::Kv), mint_workspace_id()?)?;
            kv::kv_put(loom, ns, collection, key, value.to_vec())?;
            save_loom(loom)
        })
    }

    /// Read the value at the typed `key` in KV `collection`, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed key, or an engine failure.
    pub fn kv_get(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            let key = kv::key_from_cbor(key)?;
            match read_ns(loom, workspace, FacetKind::Kv)? {
                Some(ns) => kv::kv_get(loom, ns, collection, &key),
                None => Ok(None),
            }
        })
    }

    /// Delete the typed `key` from KV `collection`; returns whether it was present. Persists on a
    /// change.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed key, or an engine or save failure.
    pub fn kv_delete(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        key: &[u8],
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            let key = kv::key_from_cbor(key)?;
            match read_ns(loom, workspace, FacetKind::Kv)? {
                Some(ns) => {
                    let present = kv::kv_delete(loom, ns, collection, &key)?;
                    if present {
                        save_loom(loom)?;
                    }
                    Ok(present)
                }
                None => Ok(false),
            }
        })
    }

    /// The KV map `collection` as canonical CBOR; an empty map when the collection or workspace is
    /// absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn kv_list(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Kv)? {
                Some(ns) => Ok(kv::kv_list(loom, ns, collection)?.encode()),
                None => Ok(kv::KvMap::new().encode()),
            }
        })
    }

    /// The half-open key range `[lo, hi)` of KV `collection` as canonical CBOR; an empty map when the
    /// collection or workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed bound, or an engine failure.
    pub fn kv_range(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        lo: &[u8],
        hi: &[u8],
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let lo = kv::key_from_cbor(lo)?;
            let hi = kv::key_from_cbor(hi)?;
            match read_ns(loom, workspace, FacetKind::Kv)? {
                Some(ns) => Ok(kv::kv_range(loom, ns, collection, &lo, &hi)?.encode()),
                None => Ok(kv::KvMap::new().encode()),
            }
        })
    }

    /// The KV map collection names in `workspace`, sorted; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn kv_list_collections(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Kv)? {
                Some(ns) => kv::kv_list_collections(loom, ns),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Set the durable config for KV `collection` (control plane), ensuring the workspace, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an rng failure, or an engine or save failure.
    pub fn set_config(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
        config: KvMapConfig,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry_mut()
                .ensure_for_write(&ns_selector(workspace, FacetKind::Kv), mint_workspace_id()?)?;
            kv::put_kv_config(loom, ns, collection, &config)?;
            save_loom(loom)
        })
    }

    /// The durable config for KV `collection`, or the default versioned config when none is stored.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session.
    pub fn get_config(
        &self,
        session: &LoomSession,
        workspace: &str,
        collection: &str,
    ) -> Result<KvMapConfig, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Kv)? {
                Some(ns) => Ok(kv::get_kv_config(loom, ns, collection)),
                None => Ok(KvMapConfig::VERSIONED),
            }
        })
    }

    /// Create a search collection with `mapping` (canonical CBOR), ensuring the workspace, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed mapping, an rng failure, or a save
    /// failure.
    pub fn search_create(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        mapping: &[u8],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let mapping = search::search_mapping_from_cbor(mapping)?;
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Search),
                mint_workspace_id()?,
            )?;
            search::search_create(loom, ns, name, mapping)?;
            save_loom(loom)
        })
    }

    /// Insert or replace document `doc` (canonical CBOR) at `id` in search collection `name`, and
    /// persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed document, or an engine or save failure.
    pub fn search_index(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &[u8],
        doc: &[u8],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let doc = search::search_document_from_cbor(doc)?;
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Search),
                mint_workspace_id()?,
            )?;
            search::search_index(loom, ns, name, id.to_vec(), doc)?;
            save_loom(loom)
        })
    }

    /// Read the document at `id` in search collection `name` as canonical CBOR, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn search_get(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &[u8],
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Search)? {
                Some(ns) => Ok(search::search_get(loom, ns, name, id)?
                    .map(|doc| search::search_document_cbor(&doc))),
                None => Ok(None),
            }
        })
    }

    /// Delete `id` from search collection `name`; returns whether it was present. Persists on a change.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine or save failure.
    pub fn search_delete(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &[u8],
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Search)? {
                Some(ns) => {
                    let present = search::search_delete(loom, ns, name, id)?;
                    if present {
                        save_loom(loom)?;
                    }
                    Ok(present)
                }
                None => Ok(false),
            }
        })
    }

    /// Run a search `request` (canonical CBOR) against collection `name`, returning the canonical-CBOR
    /// response.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent collection) or a malformed request.
    pub fn search_query(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        request: &[u8],
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let request = search::search_request_from_cbor(request)?;
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Search))?;
            Ok(search::search_response_cbor(&search::search_query(
                loom, ns, name, &request,
            )?))
        })
    }

    /// The document ids in search collection `name`, optionally restricted to a byte `prefix`; empty
    /// when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn search_ids(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        prefix: Option<&[u8]>,
    ) -> Result<Vec<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Search)? {
                Some(ns) => search::search_ids(loom, ns, name, prefix),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Replace the field mapping of search collection `name` and persist. The collection must exist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, a malformed mapping, an absent
    /// collection, or a save failure.
    pub fn search_remap(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        mapping: &[u8],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let mapping = search::search_mapping_from_cbor(mapping)?;
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Search))?;
            search::search_remap(loom, ns, name, mapping)?;
            save_loom(loom)
        })
    }

    /// Put a trigger binding (canonical CBOR) on the Program facet of `workspace`, ensuring the
    /// workspace, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed binding, an rng failure, or a save
    /// failure.
    pub fn trigger_put(
        &self,
        session: &LoomSession,
        workspace: &str,
        binding: &[u8],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let binding = trigger_binding_from_cbor(binding)?;
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Program),
                mint_workspace_id()?,
            )?;
            loom_core::trigger_put(loom, ns, &binding)?;
            save_loom(loom)
        })
    }

    pub fn program_put(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        manifest: &[u8],
        body: &[u8],
    ) -> Result<Vec<u8>, LoomError> {
        let manifest = Manifest::decode(manifest)
            .ok_or_else(|| LoomError::new(Code::InvalidArgument, "malformed program manifest"))?;
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Program),
                mint_workspace_id()?,
            )?;
            let stored = program_put(loom, ns, name, manifest, body)?;
            save_loom(loom)?;
            program_record_to_cbor(&stored)
        })
    }

    pub fn program_inspect(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Program)? {
                Some(ns) => program_inspect(loom, ns, name)?
                    .map(|stored| program_record_to_cbor(&stored))
                    .transpose(),
                None => Ok(None),
            }
        })
    }

    pub fn program_get(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Program)? {
                Some(ns) => program_get(loom, ns, name)?
                    .map(|body| program_body_to_cbor(&body))
                    .transpose(),
                None => Ok(None),
            }
        })
    }

    pub fn program_list(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let records = match read_ns(loom, workspace, FacetKind::Program)? {
                Some(ns) => program_list(loom, ns)?,
                None => Vec::new(),
            };
            program_list_to_cbor(&records)
        })
    }

    pub fn program_remove(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            let removed = match read_ns(loom, workspace, FacetKind::Program)? {
                Some(ns) => program_remove(loom, ns, name)?,
                None => false,
            };
            if removed {
                save_loom(loom)?;
            }
            Ok(removed)
        })
    }

    /// Read the trigger binding `id` as canonical CBOR, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn trigger_get(
        &self,
        session: &LoomSession,
        workspace: &str,
        id: TriggerId,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Program)? {
                Some(ns) => match loom_core::trigger_get(loom, ns, id) {
                    Ok(binding) => Ok(Some(trigger_binding_to_cbor(&binding)?)),
                    Err(err) if err.code == Code::NotFound => Ok(None),
                    Err(err) => Err(err),
                },
                None => Ok(None),
            }
        })
    }

    /// List the trigger bindings in `workspace`, each as canonical CBOR, in id order.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn trigger_list(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Program)? {
                Some(ns) => loom_core::trigger_list(loom, ns)?
                    .iter()
                    .map(trigger_binding_to_cbor)
                    .collect(),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Enable or disable trigger `id`, returning the updated binding as canonical CBOR, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent trigger) or a save failure.
    pub fn trigger_enable(
        &self,
        session: &LoomSession,
        workspace: &str,
        id: TriggerId,
        enabled: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Program))?;
            let binding = loom_core::trigger_enable(loom, ns, id, enabled)?;
            save_loom(loom)?;
            trigger_binding_to_cbor(&binding)
        })
    }

    /// Remove trigger `id`; returns whether it was present. Persists on a change.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine or save failure.
    pub fn trigger_remove(
        &self,
        session: &LoomSession,
        workspace: &str,
        id: TriggerId,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Program)? {
                Some(ns) => {
                    let present = loom_core::trigger_remove(loom, ns, id)?;
                    if present {
                        save_loom(loom)?;
                    }
                    Ok(present)
                }
                None => Ok(false),
            }
        })
    }

    /// The fire history of trigger `id` from `from_seq` (bounded by `limit`), each record as canonical
    /// CBOR, in fire-sequence order.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn trigger_history(
        &self,
        session: &LoomSession,
        workspace: &str,
        id: TriggerId,
        from_seq: u64,
        limit: u64,
    ) -> Result<Vec<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Program)? {
                Some(ns) => loom_core::trigger_history(loom, ns, id, from_seq, limit as usize)?
                    .iter()
                    .map(fire_record_to_cbor)
                    .collect(),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Create a columnar dataset `name` with typed `columns`, ensuring the workspace, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a duplicate dataset, an rng failure, or a save
    /// failure.
    pub fn columnar_create(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        columns: Vec<(String, ColumnType)>,
        target_segment_rows: u64,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Columnar),
                mint_workspace_id()?,
            )?;
            columnar_create(loom, ns, name, columns, target_segment_rows as usize)?;
            save_loom(loom)
        })
    }

    /// Append `row` to columnar dataset `name`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the dataset was never created, `INVALID_ARGUMENT` on an
    /// arity or type mismatch) or a save failure.
    pub fn columnar_append(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        row: Vec<Value>,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Columnar))?;
            columnar_append(loom, ns, name, row)?;
            save_loom(loom)
        })
    }

    /// Scan every row of columnar dataset `name`, or empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the dataset was never created) or for an unknown session.
    pub fn columnar_scan(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Vec<Vec<Value>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Columnar)? {
                Some(ns) => columnar_scan(loom, ns, name),
                None => Ok(Vec::new()),
            }
        })
    }

    /// The typed columns of dataset `name`, or empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the dataset was never created) or for an unknown session.
    pub fn columnar_columns(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Vec<(String, ColumnType)>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Columnar)? {
                Some(ns) => columnar_columns(loom, ns, name),
                None => Ok(Vec::new()),
            }
        })
    }

    /// The row count of dataset `name`, or `0` when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the dataset was never created) or for an unknown session.
    pub fn columnar_rows(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<u64, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Columnar)? {
                Some(ns) => Ok(columnar_rows(loom, ns, name)? as u64),
                None => Ok(0),
            }
        })
    }

    /// Compact the segments of columnar dataset `name` and persist. The dataset must exist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, an absent dataset, or a save
    /// failure.
    pub fn columnar_compact(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Columnar))?;
            columnar_compact(loom, ns, name)?;
            save_loom(loom)
        })
    }

    /// Inspect columnar dataset `name` (schema, row/segment counts, source digest). The dataset must
    /// exist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, or an absent dataset.
    pub fn columnar_inspect(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<ColumnarInspect, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Columnar))?;
            columnar_inspect(loom, ns, name)
        })
    }

    /// The source digest pinned in columnar dataset `name`. The dataset must exist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, or an absent dataset.
    pub fn columnar_source_digest(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Columnar))?;
            columnar_source_digest(loom, ns, name)
        })
    }

    /// Project `columns` from columnar dataset `name`'s rows matching `filter`. The dataset must
    /// exist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, or an absent dataset.
    pub fn columnar_select(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        columns: &[&str],
        filter: Option<(&str, CmpOp, &Value)>,
    ) -> Result<Vec<Vec<Value>>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Columnar))?;
            columnar_select(loom, ns, name, columns, filter)
        })
    }

    /// Evaluate `aggregates` over columnar dataset `name`'s rows matching `filter`. The dataset must
    /// exist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, or an absent dataset.
    pub fn columnar_aggregate(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        aggregates: &[ColumnarAggregate],
        filter: Option<(&str, CmpOp, &Value)>,
    ) -> Result<Vec<Value>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Columnar))?;
            columnar_aggregate(loom, ns, name, aggregates, filter)
        })
    }

    pub fn columnar_import_arrow(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        payload: &[u8],
        target_segment_rows: u64,
        replace: bool,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        let target_segment_rows = usize::try_from(target_segment_rows).map_err(|_| {
            LoomError::new(
                Code::InvalidArgument,
                "target_segment_rows does not fit in usize",
            )
        })?;
        let dataset = columnar_from_arrow_ipc(payload, target_segment_rows)?;
        self.columnar_import_dataset(
            session,
            workspace,
            name,
            "arrow-ipc",
            payload.len(),
            dataset,
            replace,
            dry_run,
        )
    }

    pub fn columnar_import_parquet(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        payload: &[u8],
        target_segment_rows: u64,
        replace: bool,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        let target_segment_rows = usize::try_from(target_segment_rows).map_err(|_| {
            LoomError::new(
                Code::InvalidArgument,
                "target_segment_rows does not fit in usize",
            )
        })?;
        let dataset = columnar_from_parquet(payload, target_segment_rows)?;
        self.columnar_import_dataset(
            session,
            workspace,
            name,
            "parquet",
            payload.len(),
            dataset,
            replace,
            dry_run,
        )
    }

    fn columnar_import_dataset(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        format: &str,
        bytes_in: usize,
        dataset: loom_core::ColumnarSet,
        replace: bool,
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let existing_ns = read_ns(loom, workspace, FacetKind::Columnar)?;
            let replaced = match existing_ns {
                Some(ns) => match get_columnar(loom, ns, name) {
                    Ok(_) => {
                        if !replace {
                            return Err(LoomError::new(
                                Code::Conflict,
                                format!(
                                    "columnar dataset {name:?} already exists; replace is required"
                                ),
                            ));
                        }
                        true
                    }
                    Err(error) if error.code == Code::NotFound => false,
                    Err(error) => return Err(error),
                },
                None => false,
            };
            let report = loom_wire::columnar::ColumnarImportReport {
                format: format.to_string(),
                columns: dataset.columns().to_vec(),
                rows: dataset.rows(),
                segment_count: dataset.segment_count(),
                target_segment_rows: dataset.target_segment_rows(),
                bytes_in,
                replaced,
                dry_run,
            };
            if !dry_run {
                let ns = loom.registry_mut().ensure_for_write(
                    &ns_selector(workspace, FacetKind::Columnar),
                    mint_workspace_id()?,
                )?;
                put_columnar(loom, ns, name, &dataset)?;
                save_loom(loom)?;
            }
            Ok(loom_wire::columnar::import_report_to_cbor(report))
        })
    }

    /// Create a vector set `name` of dimension `dim` under `metric`, ensuring the workspace, and
    /// persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a duplicate set, an rng failure, or a save
    /// failure.
    pub fn vector_create(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        dim: u64,
        metric: Metric,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Vector),
                mint_workspace_id()?,
            )?;
            vector_create(loom, ns, name, dim as usize, metric)?;
            save_loom(loom)
        })
    }

    /// Insert or replace the `vector` and `metadata` at `id` in set `name`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the set was never created, `DIMENSION_MISMATCH` on a bad
    /// width) or a save failure.
    pub fn vector_upsert(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
        vector: Vec<f32>,
        metadata: BTreeMap<String, Value>,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Vector))?;
            vector_upsert(loom, ns, name, id, vector, metadata)?;
            save_loom(loom)
        })
    }

    /// Read the vector entry at `id` in set `name`, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn vector_get(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<VectorEntry>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Vector)? {
                Some(ns) => vector_get(loom, ns, name, id),
                None => Ok(None),
            }
        })
    }

    /// The vector ids in set `name` (optionally prefix-filtered), or empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the set was never created) or for an unknown session.
    pub fn vector_ids(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Vector)? {
                Some(ns) => vector_ids(loom, ns, name, prefix),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Delete the vector at `id` in set `name`; returns whether it was present. Persists on a change.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the set was never created) or a save failure.
    pub fn vector_delete(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Vector))?;
            let present = vector_delete(loom, ns, name, id)?;
            if present {
                save_loom(loom)?;
            }
            Ok(present)
        })
    }

    /// Search set `name` for the `k` nearest hits to `query` under `filter`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the set was never created, `DIMENSION_MISMATCH` on a bad
    /// width) or for an unknown session.
    pub fn vector_search(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        query: &[f32],
        k: u64,
        filter: &MetaFilter,
    ) -> Result<Vec<Hit>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Vector))?;
            vector_search(loom, ns, name, query, k as usize, filter)
        })
    }

    /// Insert or replace the vector at `id` in set `name` with `source_text` and an optional embedding
    /// model profile, and persist. The set must exist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, an absent set, a dimension
    /// mismatch, or a save failure.
    #[allow(clippy::too_many_arguments)]
    pub fn vector_upsert_source(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
        vector: Vec<f32>,
        metadata: BTreeMap<String, Value>,
        source_text: &str,
        model: Option<EmbeddingModel>,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Vector))?;
            vector_upsert_with_source(loom, ns, name, id, vector, metadata, source_text, model)?;
            save_loom(loom)
        })
    }

    pub fn vector_text_upsert_generated(
        &self,
        session: &LoomSession,
        request: VectorTextUpsertRequest,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let bytes = apply_vector_text_upsert_generated(loom, request)?;
            save_loom(loom)?;
            Ok(bytes)
        })
    }

    pub fn vector_workspace_configure_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        request_json: &str,
    ) -> Result<String, LoomError> {
        let request = parse_vector_workspace_configure_request(request_json)?;
        self.with_session(session, |loom| {
            let json = apply_vector_workspace_configure_json(loom, workspace, request)?;
            save_loom(loom)?;
            Ok(json)
        })
    }

    pub fn inference_instance_create_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: String,
        model: String,
        kind: String,
        runtime: String,
        preset: Option<String>,
        settings_json: &str,
    ) -> Result<String, LoomError> {
        self.inference_instance_mutate_audited(
            session,
            workspace,
            name.clone(),
            "inference.instance.create",
            |state| {
                let kind = InferenceModelKind::parse(&kind)?;
                let runtime = loom_types::RuntimeKind::parse(&runtime)?;
                let settings = inference_instance_settings_from_json(settings_json)?;
                if state.find_instance(&name).is_some() {
                    return Err(LoomError::new(
                        Code::AlreadyExists,
                        format!("inference instance {name:?} already exists"),
                    ));
                }
                let model = loom_types::ModelRef::new(kind, model);
                let instance = loom_inference::build_instance_descriptor(
                    name, model.kind, model, runtime, preset, settings,
                )?;
                state.upsert_instance(instance.clone());
                inference_instance_view_json(state, &instance)
            },
        )
        .map(|receipt| receipt.result_json)
    }

    pub fn inference_instance_list_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        kind: Option<String>,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let workspace_id = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.authorize(workspace_id, FacetKind::Inference, AclRight::Read)?;
            let state = inference_instance_state(loom, workspace_id)?;
            let kind = kind.as_deref().map(InferenceModelKind::parse).transpose()?;
            let instances = state
                .instances
                .iter()
                .filter(|instance| kind.is_none_or(|kind| instance.kind == kind))
                .map(|instance| InferenceInstanceJsonView {
                    instance,
                    refs: state.instance_ref_count(&instance.name),
                })
                .collect::<Vec<_>>();
            serde_json::to_string(&instances)
                .map_err(|error| LoomError::new(Code::CorruptObject, format!("json: {error}")))
        })
    }

    pub fn inference_instance_get_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let workspace_id = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.authorize(workspace_id, FacetKind::Inference, AclRight::Read)?;
            let state = inference_instance_state(loom, workspace_id)?;
            let instance = state.find_instance(name).ok_or_else(|| {
                LoomError::new(
                    Code::NotFound,
                    format!("inference instance {name:?} not found"),
                )
            })?;
            inference_instance_view_json(&state, instance)
        })
    }

    pub fn inference_instance_update_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: String,
        preset: Option<String>,
        settings_json: &str,
    ) -> Result<String, LoomError> {
        self.inference_instance_mutate_audited(
            session,
            workspace,
            name.clone(),
            "inference.instance.update",
            |state| {
                let settings = inference_instance_settings_from_json(settings_json)?;
                let instance = state.find_instance(&name).cloned().ok_or_else(|| {
                    LoomError::new(
                        Code::NotFound,
                        format!("inference instance {name:?} not found"),
                    )
                })?;
                let instance =
                    loom_inference::update_instance_descriptor(instance, preset, settings)?;
                state.upsert_instance(instance.clone());
                inference_instance_view_json(state, &instance)
            },
        )
        .map(|receipt| receipt.result_json)
    }

    pub fn inference_instance_delete_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: String,
    ) -> Result<String, LoomError> {
        self.inference_instance_mutate_audited(
            session,
            workspace,
            name.clone(),
            "inference.instance.delete",
            |state| {
                let refs = state.instance_ref_count(&name);
                if refs != 0 {
                    return Err(LoomError::new(
                        Code::Conflict,
                        format!(
                            "inference instance {name:?} is still referenced by {refs} binding(s)"
                        ),
                    ));
                }
                state.remove_instance(&name).ok_or_else(|| {
                    LoomError::new(
                        Code::NotFound,
                        format!("inference instance {name:?} not found"),
                    )
                })?;
                inference_instance_delete_result_json(&name)
            },
        )
        .map(|receipt| receipt.result_json)
    }

    fn inference_instance_mutate_audited(
        &self,
        session: &LoomSession,
        workspace: &str,
        instance_name: String,
        action: &'static str,
        mutate: impl FnOnce(&mut loom_inference::InferenceInstanceState) -> Result<String, LoomError>,
    ) -> Result<InferenceInstanceMutationReceipt, LoomError> {
        self.inference_instance_mutate_audited_with_commit(
            session,
            workspace,
            &instance_name,
            action,
            mutate,
            loom_store::put_saved_state_and_audit,
        )
    }

    fn inference_instance_mutate_audited_with_commit(
        &self,
        session: &LoomSession,
        workspace: &str,
        instance_name: &str,
        action: &'static str,
        mutate: impl FnOnce(&mut loom_inference::InferenceInstanceState) -> Result<String, LoomError>,
        commit: impl FnOnce(
            &FileStore,
            loom_core::vcs::SavedStateObjects,
            Vec<WorkflowAuditWrite>,
        ) -> Result<loom_store::SavedStateAndAuditReceipt, LoomError>,
    ) -> Result<InferenceInstanceMutationReceipt, LoomError> {
        self.with_session(session, |loom| {
            let state_before = loom.export_state();
            let result = (|| {
                let workspace_id = loom
                    .registry()
                    .open(&ns_selector(workspace, FacetKind::Inference))?;
                loom.authorize(workspace_id, FacetKind::Inference, AclRight::Write)?;
                let principal = loom.effective_principal()?;
                let mut state = inference_instance_state(loom, workspace_id)?;
                let result_json = mutate(&mut state)?;
                put_inference_instance_state(loom, workspace_id, &state)?;
                let saved = loom.save_state_objects()?;
                let receipt = commit(
                    loom.store(),
                    saved,
                    vec![WorkflowAuditWrite {
                        principal,
                        action: action.to_string(),
                        target: Some(inference_instance_audit_target(workspace_id, instance_name)),
                    }],
                )?;
                let audit_sequence = receipt
                    .audit_sequences
                    .first()
                    .copied()
                    .ok_or_else(|| LoomError::corrupt("inference audit sequence missing"))?;
                if receipt.audit_sequences.len() != 1 {
                    return Err(LoomError::corrupt("inference audit sequence count"));
                }
                Ok(InferenceInstanceMutationReceipt {
                    result_json: inference_instance_result_with_audit_sequence(
                        &result_json,
                        audit_sequence,
                    )?,
                    audit_sequence,
                })
            })();
            match result {
                Ok(receipt) => Ok(receipt),
                Err(error) => {
                    loom.import_state(&state_before)?;
                    Err(error)
                }
            }
        })
    }

    pub fn studio_reindex_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        profile: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let report = studio_reindex(loom, ns, profile)?;
            save_loom(loom)?;
            serde_json::to_string(&report)
                .map_err(|error| LoomError::new(Code::CorruptObject, format!("json: {error}")))
        })
    }

    pub fn studio_revisions_rebuild_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        profile: &str,
        dry_run: bool,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let report = rebuild_studio_revision_index(loom, ns, profile, dry_run)?;
            if !dry_run && report.inserted > 0 {
                save_loom(loom)?;
            }
            serde_json::to_string(&report)
                .map_err(|error| LoomError::new(Code::CorruptObject, format!("json: {error}")))
        })
    }

    /// The source text stored for vector `id` in set `name`, or `None` when absent (or the workspace
    /// is absent).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn vector_source_text(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Vector)? {
                Some(ns) => vector_source_text(loom, ns, name, id),
                None => Ok(None),
            }
        })
    }

    /// The embedding-model profile of set `name`, or `None` when unset (or the workspace is absent).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn vector_embedding_model(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Option<EmbeddingModel>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Vector)? {
                Some(ns) => vector_embedding_model(loom, ns, name),
                None => Ok(None),
            }
        })
    }

    /// The metadata index keys of set `name`, or empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn vector_metadata_index_keys(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Vector)? {
                Some(ns) => vector_metadata_index_keys(loom, ns, name),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Create a metadata index on `key` for set `name`; returns whether it was newly created. Persists
    /// on a change. The set must exist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, an absent set, or a save failure.
    pub fn vector_create_metadata_index(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        key: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Vector))?;
            let created = vector_create_metadata_index(loom, ns, name, key)?;
            if created {
                save_loom(loom)?;
            }
            Ok(created)
        })
    }

    /// Drop the metadata index on `key` for set `name`; returns whether it was present. Persists on a
    /// change. The set must exist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, an absent set, or a save failure.
    pub fn vector_drop_metadata_index(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        key: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Vector))?;
            let dropped = vector_drop_metadata_index(loom, ns, name, key)?;
            if dropped {
                save_loom(loom)?;
            }
            Ok(dropped)
        })
    }

    /// Top-`k` nearest neighbours of `query` in set `name` under an explicit accelerator `policy` and
    /// PQ parameters. The set must exist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace, an absent set, or a dimension
    /// mismatch.
    #[allow(clippy::too_many_arguments)]
    pub fn vector_search_policy(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        query: &[f32],
        k: u64,
        filter: &MetaFilter,
        policy: AcceleratorPolicy,
        ef: u64,
        pq_m: u64,
        pq_k: u64,
        pq_iters: u64,
    ) -> Result<Vec<Hit>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Vector))?;
            vector_search_with_pq_policy(
                loom,
                ns,
                name,
                query,
                k as usize,
                filter,
                policy,
                ef as usize,
                pq_m as usize,
                pq_k as usize,
                pq_iters as usize,
            )
        })
    }

    /// Stage `path` in `workspace`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or a save failure.
    pub fn stage(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.stage(ns, &[path])?;
            save_loom(loom)
        })
    }

    /// Stage the whole working tree of `workspace`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or a save failure.
    pub fn stage_all(&self, session: &LoomSession, workspace: &str) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.stage_all(ns)?;
            save_loom(loom)
        })
    }

    /// Record the working tree of `workspace` as a commit and persist, returning the new commit
    /// address.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or a save failure.
    pub fn commit(
        &self,
        session: &LoomSession,
        workspace: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            let commit = loom.commit(ns, author, message, timestamp_ms)?;
            save_loom(loom)?;
            Ok(commit)
        })
    }

    /// Create branch `name` in `workspace` at the current head, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or a save failure.
    pub fn branch(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.branch(ns, name)?;
            save_loom(loom)
        })
    }

    /// Check out branch `branch` in `workspace`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace or branch) or a save failure.
    pub fn checkout(
        &self,
        session: &LoomSession,
        workspace: &str,
        branch: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.checkout_branch(ns, branch)?;
            save_loom(loom)
        })
    }

    /// The commit log of `branch` in `workspace`, newest first.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace or branch) or for an unknown session.
    pub fn log(
        &self,
        session: &LoomSession,
        workspace: &str,
        branch: &str,
    ) -> Result<Vec<Digest>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.log(ns, branch)
        })
    }

    /// The working-tree status of `workspace`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or for an unknown session.
    pub fn status(&self, session: &LoomSession, workspace: &str) -> Result<Status, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.status(ns)
        })
    }

    /// The tag names in `workspace`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or for an unknown session.
    pub fn tag_list(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.tag_list(ns)
        })
    }

    // ---- VersionControl: merge, history, tags, replay ------------------------------

    /// Merge `from_branch` into the current branch of `workspace` (row-level, or cell-level when
    /// `cell_level`), and persist. Returns the engine [`MergeOutcome`].
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/branch, `CONFLICT` on unresolved
    /// conflicts is reported in the outcome not as an error) or a save failure.
    pub fn vcs_merge(
        &self,
        session: &LoomSession,
        workspace: &str,
        from_branch: &str,
        author: &str,
        cell_level: bool,
        timestamp_ms: u64,
    ) -> Result<MergeOutcome, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            let outcome = if cell_level {
                loom.merge_cell_level(ns, from_branch, author, timestamp_ms)?
            } else {
                loom.merge(ns, from_branch, author, timestamp_ms)?
            };
            save_loom(loom)?;
            Ok(outcome)
        })
    }

    /// Whether `workspace` has an in-progress (conflicted) merge.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or for an unknown session.
    pub fn merge_in_progress(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.merge_in_progress(ns)
        })
    }

    /// The unresolved paths of `workspace`'s in-progress merge (empty when none).
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or for an unknown session.
    pub fn merge_conflicts(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.merge_conflicts(ns)
        })
    }

    /// Resolve one conflicted `path` of the in-progress merge, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace, `INVALID_ARGUMENT` for no merge or an
    /// unknown path) or a save failure.
    pub fn merge_resolve(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
        resolution: ConflictResolution,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.merge_resolve(ns, path, resolution)?;
            save_loom(loom)
        })
    }

    /// Abort the in-progress merge in `workspace`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace, `INVALID_ARGUMENT` for no merge) or a
    /// save failure.
    pub fn merge_abort(&self, session: &LoomSession, workspace: &str) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.merge_abort(ns)?;
            save_loom(loom)
        })
    }

    /// Finish the in-progress merge in `workspace` with a merge commit by `author`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace, `INVALID_ARGUMENT` for no merge or
    /// unresolved conflicts) or a save failure.
    pub fn merge_continue(
        &self,
        session: &LoomSession,
        workspace: &str,
        author: &str,
        timestamp_ms: u64,
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            let commit = loom.merge_continue(ns, author, timestamp_ms)?;
            save_loom(loom)?;
            Ok(commit)
        })
    }

    /// The canonical `LMDIFF` envelope between two commits of `workspace`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace, `INVALID_ARGUMENT` for a malformed
    /// commit) or for an unknown session.
    pub fn vcs_diff(
        &self,
        session: &LoomSession,
        workspace: &str,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            let from = Digest::parse(from_commit)?;
            let to = Digest::parse(to_commit)?;
            loom.diff_commits(ns, from, to)
        })
    }

    /// The blame table (path -> owning commit) for `branch` in `workspace`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/branch) or for an unknown session.
    pub fn vcs_blame(
        &self,
        session: &LoomSession,
        workspace: &str,
        branch: &str,
    ) -> Result<Vec<(String, Digest)>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.blame(ns, branch)
        })
    }

    /// Unstage `path` in `workspace` (reset the index entry to `HEAD`), and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or a save failure.
    pub fn unstage(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.unstage(ns, &[path])?;
            save_loom(loom)
        })
    }

    /// Commit only the staging index of `workspace`, and persist. Returns the new commit.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or a save failure.
    pub fn commit_staged(
        &self,
        session: &LoomSession,
        workspace: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            let commit = loom.commit_staged(ns, author, message, timestamp_ms)?;
            save_loom(loom)?;
            Ok(commit)
        })
    }

    /// Create tag `name` at `rev` in `workspace` (annotated when `message` is non-empty), and persist.
    /// Returns the tag's target digest.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/rev, `ALREADY_EXISTS` on a name clash)
    /// or a save failure.
    pub fn tag_create(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        rev: &str,
        tagger: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            let target = loom.tag_create(ns, name, rev, tagger, message, timestamp_ms)?;
            save_loom(loom)?;
            Ok(target)
        })
    }

    /// The raw ref target of tag `name` in `workspace`, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or for an unknown session.
    pub fn tag_target(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Option<Digest>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.tag_target(ns, name)
        })
    }

    /// Delete tag `name` in `workspace`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace or tag) or a save failure.
    pub fn tag_delete(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.tag_delete(ns, name)?;
            save_loom(loom)
        })
    }

    /// Rename tag `old_name` to `new_name` in `workspace`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace or tag, `ALREADY_EXISTS` on a clash) or
    /// a save failure.
    pub fn tag_rename(
        &self,
        session: &LoomSession,
        workspace: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.tag_rename(ns, old_name, new_name)?;
            save_loom(loom)
        })
    }

    /// Restore file `path` in `workspace`'s working tree from `rev`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/rev/path) or a save failure.
    pub fn restore_file(
        &self,
        session: &LoomSession,
        workspace: &str,
        rev: &str,
        path: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.restore_file(ns, rev, path)?;
            save_loom(loom)
        })
    }

    /// Restore all paths under `prefix` in `workspace`'s working tree from `rev`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/rev) or a save failure.
    pub fn restore_path(
        &self,
        session: &LoomSession,
        workspace: &str,
        rev: &str,
        prefix: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.restore_path(ns, rev, prefix)?;
            save_loom(loom)
        })
    }

    /// Cherry-pick `commits` onto the current branch of `workspace`. Persists only on a real (non
    /// `dry_run`) apply that changes state. Returns the engine [`ReplayOutcome`].
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/commit) or a save failure.
    pub fn vcs_cherry_pick(
        &self,
        session: &LoomSession,
        workspace: &str,
        commits: &[Digest],
        dry_run: bool,
        timestamp_ms: u64,
    ) -> Result<ReplayOutcome, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            let outcome = loom.cherry_pick(ns, commits, timestamp_ms, dry_run)?;
            if !dry_run {
                save_loom(loom)?;
            }
            Ok(outcome)
        })
    }

    /// Revert `commits` on the current branch of `workspace`. Persists only on a real apply.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/commit) or a save failure.
    pub fn vcs_revert(
        &self,
        session: &LoomSession,
        workspace: &str,
        commits: &[Digest],
        author: &str,
        dry_run: bool,
        timestamp_ms: u64,
    ) -> Result<ReplayOutcome, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            let outcome = loom.revert(ns, commits, author, timestamp_ms, dry_run)?;
            if !dry_run {
                save_loom(loom)?;
            }
            Ok(outcome)
        })
    }

    /// Rebase the current branch of `workspace` onto `onto`. Persists only on a real apply.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/target) or a save failure.
    pub fn vcs_rebase(
        &self,
        session: &LoomSession,
        workspace: &str,
        onto: &str,
        dry_run: bool,
        timestamp_ms: u64,
    ) -> Result<ReplayOutcome, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            let outcome = loom.rebase(ns, onto, timestamp_ms, dry_run)?;
            if !dry_run {
                save_loom(loom)?;
            }
            Ok(outcome)
        })
    }

    /// Squash the current branch of `workspace` down onto `onto` as a single commit, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/target, `CONFLICT` for an in-progress
    /// merge) or a save failure.
    pub fn squash(
        &self,
        session: &LoomSession,
        workspace: &str,
        onto: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            let commit = loom.squash(ns, onto, author, message, timestamp_ms)?;
            save_loom(loom)?;
            Ok(commit)
        })
    }

    /// Start an async commit-log read of `branch` in `workspace` as a pending [`Task`]; the log runs on
    /// the first `task_poll` and its result is the canonical CBOR digest list.
    pub fn log_async(&self, session: &LoomSession, workspace: &str, branch: &str) -> Task {
        self.spawn_task(TaskWork::LogAsync {
            session: session.clone(),
            workspace: workspace.to_string(),
            branch: branch.to_string(),
        })
    }

    /// Start an async merge of `from_branch` into `workspace` as a pending [`Task`]; the merge runs (and
    /// persists) on the first `task_poll` and its result is the canonical CBOR `MergeResult`.
    pub fn merge_async(
        &self,
        session: &LoomSession,
        workspace: &str,
        from_branch: &str,
        author: &str,
        cell_level: bool,
    ) -> Task {
        self.spawn_task(TaskWork::MergeAsync {
            session: session.clone(),
            workspace: workspace.to_string(),
            from_branch: from_branch.to_string(),
            author: author.to_string(),
            cell_level,
        })
    }

    /// Create a calendar collection with `meta` (canonical CBOR) under `principal`, ensuring the
    /// workspace, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, malformed meta, an rng failure, or a save failure.
    pub fn calendar_create_collection(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        collection: &str,
        meta: &[u8],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let meta = calendar::CollectionMeta::decode(meta)?;
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Calendar),
                mint_workspace_id()?,
            )?;
            calendar::create_collection(loom, ns, principal, collection, &meta)?;
            save_loom(loom)
        })
    }

    /// Put a calendar `entry` (canonical CBOR) into a collection, and persist. Returns the entry ETag.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for a missing collection, `INVALID_ARGUMENT` on a bad entry)
    /// or a save failure.
    pub fn calendar_put_entry(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        collection: &str,
        entry: &[u8],
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let entry = calendar::CalendarEntry::decode(entry)?;
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Calendar),
                mint_workspace_id()?,
            )?;
            let etag = calendar::put_entry(loom, ns, principal, collection, &entry)?;
            save_loom(loom)?;
            Ok(etag)
        })
    }

    /// Import an iCalendar (.ics) document into `collection`, ensuring the workspace on first write, and
    /// persist. Returns the etag [`Digest`].
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an invalid iCalendar payload, or an engine/save failure.
    pub fn calendar_put_ics(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        collection: &str,
        ics: &str,
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Calendar),
                mint_workspace_id()?,
            )?;
            let etag = calendar::put_ics(loom, ns, principal, collection, ics)?;
            save_loom(loom)?;
            Ok(etag)
        })
    }

    /// Import a vCard (.vcf) document into `book`, ensuring the workspace on first write, and persist.
    /// Returns the etag [`Digest`].
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, an invalid vCard payload, or an engine/save failure.
    pub fn contacts_put_vcard(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        book: &str,
        vcard: &str,
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Contacts),
                mint_workspace_id()?,
            )?;
            let etag = contacts::put_vcard(loom, ns, principal, book, vcard)?;
            save_loom(loom)?;
            Ok(etag)
        })
    }

    /// The derived source digest of search index `name` (status/introspection).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace or an engine failure.
    pub fn search_source_digest(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Search))?;
            search::search_source_digest(loom, ns, name)
        })
    }

    /// Full search-index status for `name` at `engine_version`, as canonical bytes of
    /// `[source_digest, DerivedArtifactStatus]` about the served store's derived tantivy artifact.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace or an engine failure.
    pub fn search_status(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        engine_version: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Search))?;
            let source_digest = search::search_source_digest(loom, ns, name)?;
            let status =
                loom.store()
                    .search_tantivy_status(ns, name, source_digest, engine_version)?;
            loom_store::encode_search_status_result(&source_digest, &status)
        })
    }

    /// The current HEAD branch name for `workspace` (what `vcs log` resolves locally).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace or an engine failure.
    pub fn vcs_head_branch(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.registry().head_branch(ns)
        })
    }

    /// Read the calendar entry `uid` as canonical CBOR, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn calendar_get_entry(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Calendar)? {
                Some(ns) => {
                    Ok(calendar::get_entry(loom, ns, principal, collection, uid)?
                        .map(|e| e.encode()))
                }
                None => Ok(None),
            }
        })
    }

    /// Delete the calendar entry `uid`; returns whether it was present. Persists on a change.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine or save failure.
    pub fn calendar_delete_entry(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Calendar)? {
                Some(ns) => {
                    let present = calendar::delete_entry(loom, ns, principal, collection, uid)?;
                    if present {
                        save_loom(loom)?;
                    }
                    Ok(present)
                }
                None => Ok(false),
            }
        })
    }

    /// List the calendar entries of a collection, each as canonical CBOR, in UID order.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn calendar_list_entries(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> Result<Vec<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Calendar)? {
                Some(ns) => Ok(calendar::list_entries(loom, ns, principal, collection)?
                    .iter()
                    .map(|e| e.encode())
                    .collect()),
                None => Ok(Vec::new()),
            }
        })
    }

    /// The metadata of calendar collection `collection` under `principal`, or `None` when absent (or
    /// the workspace is absent).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn calendar_get_collection(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Calendar)? {
                Some(ns) => {
                    Ok(calendar::get_collection(loom, ns, principal, collection)?
                        .map(|m| m.encode()))
                }
                None => Ok(None),
            }
        })
    }

    /// The calendar collection ids under `principal`, sorted; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn calendar_list_collections(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Calendar)? {
                Some(ns) => calendar::list_collections(loom, ns, principal),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Delete calendar collection `collection` under `principal` and persist; returns whether it
    /// existed. A no-op when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine or save failure.
    pub fn calendar_delete_collection(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Calendar)? {
                Some(ns) => {
                    let existed = calendar::delete_collection(loom, ns, principal, collection)?;
                    if existed {
                        save_loom(loom)?;
                    }
                    Ok(existed)
                }
                None => Ok(false),
            }
        })
    }

    /// The occurrences of `collection` within the `[from, to)` window (each bound a `YYYYMMDDTHHMMSS`
    /// wall-clock string) as canonical CBOR; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed window bound, or an engine failure.
    pub fn calendar_range(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        collection: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let from = loom_wire::calendar::parse_window_bound(from, "from")?;
            let to = loom_wire::calendar::parse_window_bound(to, "to")?;
            match read_ns(loom, workspace, FacetKind::Calendar)? {
                Some(ns) => loom_wire::calendar::occurrences_to_cbor(calendar::range(
                    loom, ns, principal, collection, from, to,
                )?),
                None => loom_wire::calendar::occurrences_to_cbor(Vec::new()),
            }
        })
    }

    /// The entries of `collection` matching the optional `component` filter (`""`/`"event"`/`"todo"`)
    /// and `text`; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a bad component filter, or an engine failure.
    pub fn calendar_search(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        collection: &str,
        component: &str,
        text: &str,
    ) -> Result<Vec<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            let component = loom_wire::calendar::parse_component_filter(component)?;
            let text = if text.is_empty() { None } else { Some(text) };
            match read_ns(loom, workspace, FacetKind::Calendar)? {
                Some(ns) => Ok(
                    calendar::search(loom, ns, principal, collection, component, text)?
                        .iter()
                        .map(|e| e.encode())
                        .collect(),
                ),
                None => Ok(Vec::new()),
            }
        })
    }

    /// The iCalendar serialization of entry `uid` in `collection`, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn calendar_to_ics(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Calendar)? {
                Some(ns) => Ok(calendar::entry_ics(loom, ns, principal, collection, uid)?
                    .map(String::into_bytes)),
                None => Ok(None),
            }
        })
    }

    /// Create an address book with `meta` (canonical CBOR) under `principal`, ensuring the workspace,
    /// and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, malformed meta, an rng failure, or a save failure.
    pub fn contacts_create_book(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        book: &str,
        meta: &[u8],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let meta = contacts::BookMeta::decode(meta)?;
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Contacts),
                mint_workspace_id()?,
            )?;
            contacts::create_book(loom, ns, principal, book, &meta)?;
            save_loom(loom)
        })
    }

    /// Put a contact `entry` (canonical CBOR) into a book, and persist. Returns the entry ETag.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for a missing book, `INVALID_ARGUMENT` on a bad entry) or a
    /// save failure.
    pub fn contacts_put_entry(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        book: &str,
        entry: &[u8],
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let entry = contacts::ContactEntry::decode(entry)?;
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Contacts),
                mint_workspace_id()?,
            )?;
            let etag = contacts::put_entry(loom, ns, principal, book, &entry)?;
            save_loom(loom)?;
            Ok(etag)
        })
    }

    /// Read the contact `uid` as canonical CBOR, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn contacts_get_entry(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Contacts)? {
                Some(ns) => {
                    Ok(contacts::get_entry(loom, ns, principal, book, uid)?.map(|e| e.encode()))
                }
                None => Ok(None),
            }
        })
    }

    /// Delete the contact `uid`; returns whether it was present. Persists on a change.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine or save failure.
    pub fn contacts_delete_entry(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Contacts)? {
                Some(ns) => {
                    let present = contacts::delete_entry(loom, ns, principal, book, uid)?;
                    if present {
                        save_loom(loom)?;
                    }
                    Ok(present)
                }
                None => Ok(false),
            }
        })
    }

    /// List the contacts of a book, each as canonical CBOR, in UID order.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn contacts_list_entries(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> Result<Vec<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Contacts)? {
                Some(ns) => Ok(contacts::list_entries(loom, ns, principal, book)?
                    .iter()
                    .map(|e| e.encode())
                    .collect()),
                None => Ok(Vec::new()),
            }
        })
    }

    /// The metadata of contact book `book` under `principal`, or `None` when absent (or the workspace
    /// is absent).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn contacts_get_book(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Contacts)? {
                Some(ns) => Ok(contacts::get_book(loom, ns, principal, book)?.map(|m| m.encode())),
                None => Ok(None),
            }
        })
    }

    /// The contact book ids under `principal`, sorted; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn contacts_list_books(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Contacts)? {
                Some(ns) => contacts::list_books(loom, ns, principal),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Delete contact book `book` under `principal` and persist; returns whether it existed. A no-op
    /// when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine or save failure.
    pub fn contacts_delete_book(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Contacts)? {
                Some(ns) => {
                    let existed = contacts::delete_book(loom, ns, principal, book)?;
                    if existed {
                        save_loom(loom)?;
                    }
                    Ok(existed)
                }
                None => Ok(false),
            }
        })
    }

    /// The entries of `book` matching `text`; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn contacts_search(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        book: &str,
        text: &str,
    ) -> Result<Vec<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Contacts)? {
                Some(ns) => Ok(contacts::search(loom, ns, principal, book, text)?
                    .iter()
                    .map(|e| e.encode())
                    .collect()),
                None => Ok(Vec::new()),
            }
        })
    }

    /// The vCard serialization of entry `uid` in `book`, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn contacts_to_vcard(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Contacts)? {
                Some(ns) => {
                    Ok(contacts::entry_vcard(loom, ns, principal, book, uid)?
                        .map(String::into_bytes))
                }
                None => Ok(None),
            }
        })
    }

    /// Create a mailbox with `meta` (canonical CBOR) under `principal`, ensuring the workspace, and
    /// persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, malformed meta, an rng failure, or a save failure.
    pub fn mail_create_mailbox(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        meta: &[u8],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let meta = mail::MailboxMeta::decode(meta)?;
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Mail),
                mint_workspace_id()?,
            )?;
            mail::create_mailbox(loom, ns, principal, mailbox, &meta)?;
            save_loom(loom)
        })
    }

    /// Ingest a raw RFC 5322 message at `uid` into a mailbox, and persist. Returns the body content
    /// address.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for a missing mailbox) or a save failure.
    pub fn mail_ingest_message(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
        raw: &[u8],
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Mail),
                mint_workspace_id()?,
            )?;
            let digest = mail::ingest_message(loom, ns, principal, mailbox, uid, raw)?;
            save_loom(loom)?;
            Ok(digest)
        })
    }

    /// Read the parsed index record for message `uid` as canonical CBOR, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn mail_get_message(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Mail)? {
                Some(ns) => {
                    Ok(mail::get_message(loom, ns, principal, mailbox, uid)?.map(|m| m.encode()))
                }
                None => Ok(None),
            }
        })
    }

    /// The raw RFC 5322 bytes of message `uid`, or `None` when absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn mail_to_eml(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Mail)? {
                Some(ns) => mail::to_eml(loom, ns, principal, mailbox, uid),
                None => Ok(None),
            }
        })
    }

    /// Delete message `uid`; returns whether it was present. Persists on a change.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine or save failure.
    pub fn mail_delete_message(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Mail)? {
                Some(ns) => {
                    let present = mail::delete_message(loom, ns, principal, mailbox, uid)?;
                    if present {
                        save_loom(loom)?;
                    }
                    Ok(present)
                }
                None => Ok(false),
            }
        })
    }

    /// List the parsed message records in a mailbox, each as canonical CBOR.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn mail_list_messages(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> Result<Vec<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Mail)? {
                Some(ns) => Ok(mail::list_messages(loom, ns, principal, mailbox)?
                    .iter()
                    .map(|m| m.encode())
                    .collect()),
                None => Ok(Vec::new()),
            }
        })
    }

    /// The metadata of mailbox `mailbox` under `principal`, or `None` when absent (or the workspace is
    /// absent).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn mail_get_mailbox(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Mail)? {
                Some(ns) => {
                    Ok(mail::get_mailbox(loom, ns, principal, mailbox)?.map(|m| m.encode()))
                }
                None => Ok(None),
            }
        })
    }

    /// The mailbox ids under `principal`, sorted; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn mail_list_mailboxes(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Mail)? {
                Some(ns) => mail::list_mailboxes(loom, ns, principal),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Delete mailbox `mailbox` under `principal` and persist; returns whether it existed. A no-op when
    /// the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine or save failure.
    pub fn mail_delete_mailbox(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Mail)? {
                Some(ns) => {
                    let existed = mail::delete_mailbox(loom, ns, principal, mailbox)?;
                    if existed {
                        save_loom(loom)?;
                    }
                    Ok(existed)
                }
                None => Ok(false),
            }
        })
    }

    /// The flags on message `uid` in `mailbox`; empty when the message or workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn mail_get_flags(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Vec<String>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Mail)? {
                Some(ns) => mail::get_flags(loom, ns, principal, mailbox, uid),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Replace the flags on message `uid` in `mailbox` and persist. A no-op when the workspace is
    /// absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine or save failure.
    pub fn mail_set_flags(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
        flags: &[String],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Mail)? {
                Some(ns) => {
                    mail::set_flags(loom, ns, principal, mailbox, uid, flags)?;
                    save_loom(loom)
                }
                None => Ok(()),
            }
        })
    }

    /// The messages of `mailbox` matching `text`; empty when the workspace is absent.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or an engine failure.
    pub fn mail_search(
        &self,
        session: &LoomSession,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        text: &str,
    ) -> Result<Vec<Vec<u8>>, LoomError> {
        self.with_session(session, |loom| {
            match read_ns(loom, workspace, FacetKind::Mail)? {
                Some(ns) => Ok(mail::search(loom, ns, principal, mailbox, text)?
                    .iter()
                    .map(|m| m.encode())
                    .collect()),
                None => Ok(Vec::new()),
            }
        })
    }

    /// Create a dataframe `name` from a `plan` (canonical CBOR), ensuring the workspace, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed plan, a duplicate frame, an rng
    /// failure, or a save failure.
    pub fn dataframe_create(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        plan: &[u8],
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let plan = DataframePlan::decode(plan)?;
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Dataframe),
                mint_workspace_id()?,
            )?;
            dataframe_create(loom, ns, name, &plan)?;
            save_loom(loom)
        })
    }

    /// Collect dataframe `name`, returning `[columns, rows]` as canonical CBOR.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the frame does not exist) or for an unknown session.
    pub fn dataframe_collect(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Dataframe))?;
            Ok(dataframe_collect(loom, ns, name)?.encode())
        })
    }

    /// Preview up to `rows` of dataframe `name`, returning `[columns, rows]` as canonical CBOR.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the frame does not exist) or for an unknown session.
    pub fn dataframe_preview(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
        rows: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Dataframe))?;
            Ok(dataframe_preview(loom, ns, name, rows)?.encode())
        })
    }

    /// Materialize dataframe `name`, and persist. Returns the materialized digest, if any.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the frame does not exist) or a save failure.
    pub fn dataframe_materialize(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Option<Digest>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Dataframe))?;
            let digest = dataframe_materialize(loom, ns, name)?;
            save_loom(loom)?;
            Ok(digest)
        })
    }

    /// The plan digest of dataframe `name`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the frame does not exist) or for an unknown session.
    pub fn dataframe_plan_digest(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Digest, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Dataframe))?;
            dataframe_plan_digest(loom, ns, name)
        })
    }

    /// The source digests pinned in dataframe `name`'s plan.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` if the frame does not exist) or for an unknown session.
    pub fn dataframe_source_digests(
        &self,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> Result<Vec<Digest>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Dataframe))?;
            dataframe_source_digests(loom, ns, name)
        })
    }

    /// Execute a canonical `loom.exec.request.v1` program `request` (canonical CBOR) and persist any
    /// committed changes, returning the canonical `loom.exec.result.v1` response.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a malformed request, or an execution failure.
    pub fn exec_cbor(&self, session: &LoomSession, request: &[u8]) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let result = execute_cbor(loom, request)
                .map_err(|err| LoomError::new(Code::Internal, err.to_string()))?;
            save_loom(loom)?;
            Ok(result)
        })
    }

    /// Apply a gated proposal fork described by canonical `loom.exec.apply.request.v1` CBOR, persist
    /// successful outcomes, and return canonical `loom.exec.apply.result.v1` CBOR.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, malformed request, absent workspace or branch,
    /// merge failure, or save failure.
    pub fn apply_cbor(&self, session: &LoomSession, request: &[u8]) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let request = decode_exec_apply_request(request)?;
            let mut candidate =
                loom.fork_state_into(loom_core::provider::PlanningObjectStore::new(loom.store()))?;
            let ns = candidate
                .registry()
                .open(&ns_selector_by_name(&request.workspace))?;
            let outcome = apply(
                &mut candidate,
                ns,
                &request.base,
                &request.fork,
                &request.author,
                request.timestamp_ms,
            )
            .map_err(exec_error_to_loom)?;
            let result = encode_exec_apply_result(&outcome)?;
            let candidate_state =
                save_exec_apply_candidate(&self.path, loom.store(), &mut candidate)?;
            drop(candidate);
            loom.import_engine_state_preserving_mutable_overlay(&candidate_state)?;
            Ok(result)
        })
    }

    /// Import a normalized Meetings snapshot and return canonical import-report JSON.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, missing write authorization, malformed profile
    /// or snapshot input, import failure, or save failure.
    pub fn meetings_import_snapshot(
        &self,
        session: &LoomSession,
        workspace: &str,
        input_profile: &str,
        snapshot: &[u8],
        dry_run: bool,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let mut candidate =
                loom.fork_state_into(loom_core::provider::PlanningObjectStore::new(loom.store()))?;
            let workspace_id =
                ensure_generated_facet_workspace(&mut candidate, workspace, FacetKind::Vcs)?;
            meetings_import_test_hook(
                &self.path,
                MeetingsImportTestHookPoint::AfterWorkspaceCreation,
            )?;
            candidate.authorize(workspace_id, FacetKind::Vcs, AclRight::Write)?;
            let input_profile = loom_interchange_io::parse_meetings_input_profile(input_profile)?;
            let previous = load_meetings_snapshot(loom, &workspace_id.to_string())?;
            let mut plan = loom_interchange_io::plan_meetings_import(
                &mut candidate,
                workspace_id,
                input_profile,
                snapshot,
                previous,
                dry_run,
            )?;
            meetings_import_test_hook(
                &self.path,
                MeetingsImportTestHookPoint::AfterPayloadRevisionPlanning,
            )?;
            let report = plan.report_json.clone();
            if !dry_run && (!plan.writes.is_empty() || !plan.owner_state.is_empty()) {
                for (digest, canonical) in candidate.store().objects()? {
                    if !plan
                        .owner_state
                        .objects
                        .iter()
                        .any(|(existing, _)| existing == &digest)
                    {
                        plan.owner_state.objects.push((digest, canonical));
                    }
                }
                let expected_generation = if loom.store().uses_mutable_overlay_current_records() {
                    loom.store().mutable_overlay_generation()?
                } else {
                    loom.mutable_overlay_snapshot().generation()
                };
                meetings_import_test_hook(
                    &self.path,
                    MeetingsImportTestHookPoint::BeforePublication,
                )?;
                let receipt = loom
                    .store()
                    .commit_workflow_transaction(WorkflowTransaction {
                        workspace: plan.workspace_id,
                        actor: loom.effective_principal()?.unwrap_or(plan.workspace_id),
                        expected_generation: Some(expected_generation),
                        writes: plan.writes,
                        prepared_operations: Vec::new(),
                        revision_metadata: Vec::new(),
                        delivery_intents: Vec::new(),
                        durability: OverlayDurabilityPolicy::Normal,
                        boundary: AtomicityBoundary::Single,
                        idempotency: None,
                        owner_state: plan.owner_state,
                        post_commit_delta: None,
                    })?;
                for outcome in receipt.writes {
                    let current = loom
                        .store()
                        .mutable_overlay_current_entry(&outcome.target)?
                        .ok_or_else(|| {
                            LoomError::corrupt(
                                "workflow transaction omitted committed current record",
                            )
                        })?;
                    loom.mutable_overlay_mut()
                        .synchronize_current_entry(current)?;
                }
                loom.import_engine_state_preserving_mutable_overlay(&plan.engine_state)?;
            }
            Ok(report)
        })
    }

    pub fn drive_create_folder_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        parent_folder_id: &str,
        folder_id: &str,
        name: &str,
        expected_root: &str,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::create_folder(
                loom,
                workspace,
                drive_workspace_id,
                parent_folder_id,
                folder_id,
                name,
                expected_root,
            )
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "matches the generated Drive IDL signature"
    )]
    pub fn drive_create_upload_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        upload_id: &str,
        parent_folder_id: &str,
        name: &str,
        file_id: &str,
        expected_root: &str,
        created_at_ms: u64,
        replace_file: bool,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::create_upload(
                loom,
                workspace,
                loom_drive::HostedDriveCreateUpload {
                    workspace_id: drive_workspace_id,
                    upload_id,
                    parent_folder_id,
                    name,
                    file_id,
                    expected_root,
                    created_at_ms,
                    replace_file,
                },
            )
        })
    }

    pub fn drive_upload_chunk_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        upload_id: &str,
        chunk: &[u8],
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::upload_chunk(loom, workspace, drive_workspace_id, upload_id, chunk)
        })
    }

    pub fn drive_commit_upload_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        upload_id: &str,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::commit_upload(loom, workspace, drive_workspace_id, upload_id)
        })
    }

    pub fn drive_rename_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        folder_id: &str,
        node_id: &str,
        new_name: &str,
        expected_root: &str,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::rename_node(
                loom,
                workspace,
                drive_workspace_id,
                folder_id,
                node_id,
                new_name,
                expected_root,
            )
        })
    }

    pub fn drive_move_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        source_folder_id: &str,
        target_folder_id: &str,
        node_id: &str,
        expected_root: &str,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::move_node(
                loom,
                workspace,
                drive_workspace_id,
                source_folder_id,
                target_folder_id,
                node_id,
                expected_root,
            )
        })
    }

    pub fn drive_delete_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        folder_id: &str,
        node_id: &str,
        expected_root: &str,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::delete_node(
                loom,
                workspace,
                drive_workspace_id,
                folder_id,
                node_id,
                expected_root,
            )
        })
    }

    pub fn drive_resolve_conflict_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        conflict_id: &str,
        resolution: &str,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            let resolution = parse_drive_conflict_resolution(resolution)?;
            loom_drive::resolve_conflict(
                loom,
                workspace,
                drive_workspace_id,
                conflict_id,
                resolution,
            )
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "matches the generated Drive IDL signature"
    )]
    pub fn drive_pin_retention_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        pin_id: &str,
        kind: &str,
        root: &str,
        target_entity_id: Option<&str>,
        added_at_ms: u64,
        expires_at_ms: Option<u64>,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::pin_retention(
                loom,
                workspace,
                loom_drive::HostedDrivePinRetention {
                    workspace_id: drive_workspace_id,
                    pin_id,
                    kind,
                    root,
                    target_entity_id,
                    added_at_ms,
                    expires_at_ms,
                },
            )
        })
    }

    pub fn drive_unpin_retention_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        pin_id: &str,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::unpin_retention(loom, workspace, drive_workspace_id, pin_id)
        })
    }

    pub fn drive_apply_retention_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        now_ms: u64,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::apply_retention(loom, workspace, drive_workspace_id, now_ms)
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "matches the generated Drive IDL signature"
    )]
    pub fn drive_grant_share_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        grant_id: &str,
        target_kind: &str,
        target_id: &str,
        principal: &str,
        role: &str,
        granted_at_ms: u64,
        expires_at_ms: Option<u64>,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::grant_share(
                loom,
                workspace,
                loom_drive::HostedDriveGrantShare {
                    workspace_id: drive_workspace_id,
                    grant_id,
                    target_kind,
                    target_id,
                    principal,
                    role,
                    granted_at_ms,
                    expires_at_ms,
                },
            )
        })
    }

    pub fn drive_revoke_share_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        grant_id: &str,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::revoke_share(loom, workspace, drive_workspace_id, grant_id)
        })
    }

    pub fn drive_apply_share_expiry_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        now_ms: u64,
    ) -> Result<String, LoomError> {
        self.drive_write_json(session, workspace, drive_workspace_id, |loom, workspace| {
            loom_drive::apply_share_expiry(loom, workspace, drive_workspace_id, now_ms)
        })
    }

    fn drive_write_json<T: serde::Serialize>(
        &self,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        operation: impl FnOnce(
            &mut Loom<loom_core::provider::PlanningObjectStore<'_, FileStore>>,
            WorkspaceId,
        ) -> Result<T, LoomError>,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let mut candidate =
                loom.fork_state_into(loom_core::provider::PlanningObjectStore::new(loom.store()))?;
            let workspace = candidate.registry().open(&ns_selector_by_name(workspace))?;
            authorize_generated_drive_collection(&candidate, workspace, drive_workspace_id)?;
            let write = operation(&mut candidate, workspace)?;
            let published =
                save_generated_planning_candidate(&self.path, loom.store(), &mut candidate)?;
            drop(candidate);
            for receipt in &published.workflow_receipts {
                for outcome in &receipt.writes {
                    let current = loom
                        .store()
                        .mutable_overlay_current_entry(&outcome.target)?
                        .ok_or_else(|| {
                            LoomError::corrupt(
                                "workflow transaction omitted committed current record",
                            )
                        })?;
                    loom.mutable_overlay_mut()
                        .synchronize_current_entry(current)?;
                }
            }
            loom.import_engine_state_preserving_mutable_overlay(&published.engine_state)?;
            json_string(&write)
        })
    }

    /// Subscribe to `branch` in `workspace` from `from` (or the current head), returning an opaque
    /// resume cursor string.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace, `UNSUPPORTED` for a narrowed
    /// selector) or for an unknown session.
    pub fn watch_subscribe(
        &self,
        session: &LoomSession,
        workspace: &str,
        branch: &str,
        from: Option<Digest>,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            let selector = WatchSelector {
                workspace: ns,
                branch: branch.to_string(),
                facet: None,
                path_prefix: None,
                change_kinds: Vec::new(),
            };
            Ok(loom.watch_subscribe(&selector, from)?.encode())
        })
    }

    /// Poll up to `max` change events from `cursor`, returning the batch (events and the next cursor).
    ///
    /// # Errors
    /// Returns [`LoomError`] (`CURSOR_INVALID` for a malformed cursor) or for an unknown session.
    pub fn watch_poll(
        &self,
        session: &LoomSession,
        cursor: &str,
        max: u32,
    ) -> Result<WatchBatch, LoomError> {
        self.with_session(session, |loom| {
            let cursor = WatchCursor::decode(cursor)?;
            loom.watch_poll(&cursor, max as usize)
        })
    }

    /// Subscribe to a fully specified `selector` from `from` (or the current head), returning an opaque
    /// resume cursor string. Unlike [`Self::watch_subscribe`], the caller supplies the whole selector
    /// (facet, path prefix, and change-kind narrowing), so this backs the generated `Watch.subscribe`
    /// and `Watch.stream` wire methods.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace/branch, `UNSUPPORTED` for a narrowed
    /// selector the engine cannot serve) or for an unknown session.
    pub fn watch_subscribe_selector(
        &self,
        session: &LoomSession,
        selector: WatchSelector,
        from: Option<Digest>,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            Ok(loom.watch_subscribe(&selector, from)?.encode())
        })
    }

    /// Open a file handle over `path` in `workspace`, ensuring the workspace, returning the handle id.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` when opening a missing file read-only) or an rng/engine
    /// failure.
    pub fn file_open(
        &self,
        session: &LoomSession,
        workspace: &str,
        path: &str,
        mode: OpenMode,
    ) -> Result<u64, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Files),
                mint_workspace_id()?,
            )?;
            loom.file_open(ns, path, mode)
        })
    }

    /// Read up to `len` bytes from the file handle at its cursor, advancing it.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or handle, or a read on a write-only handle.
    pub fn file_read(
        &self,
        session: &LoomSession,
        file: u64,
        len: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| loom.file_read(file, len))
    }

    /// Write `content` to the file handle at its cursor, advancing it, and persist. Returns the new
    /// cursor position.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or handle, a write on a read-only handle, or a save
    /// failure.
    pub fn file_write(
        &self,
        session: &LoomSession,
        file: u64,
        content: &[u8],
    ) -> Result<u64, LoomError> {
        self.with_session(session, |loom| {
            let next = loom.file_write(file, content)?;
            save_loom(loom)?;
            Ok(next)
        })
    }

    /// The live size and mode of the open file handle.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or handle.
    pub fn file_stat(&self, session: &LoomSession, file: u64) -> Result<FileStat, LoomError> {
        self.with_session(session, |loom| loom.file_stat(file))
    }

    /// Close the file handle, and persist (reclaiming bytes when the last handle on an unlinked inode
    /// closes).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or handle, or a save failure.
    pub fn file_close(&self, session: &LoomSession, file: u64) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            loom.file_close(file)?;
            save_loom(loom)
        })
    }

    /// Read `len` bytes at `offset` from open file handle `file` (no cursor movement).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or handle, or an engine failure.
    pub fn file_read_at(
        &self,
        session: &LoomSession,
        file: u64,
        offset: u64,
        len: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| loom.file_read_at(file, offset, len))
    }

    /// Write `content` at `offset` in open file handle `file` and persist; returns the new size.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or handle, a non-writable mode, or a save failure.
    pub fn file_write_at(
        &self,
        session: &LoomSession,
        file: u64,
        offset: u64,
        content: &[u8],
    ) -> Result<u64, LoomError> {
        self.with_session(session, |loom| {
            let size = loom.file_write_at(file, offset, content)?;
            save_loom(loom)?;
            Ok(size)
        })
    }

    /// Resize open file handle `file` to `size` and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or handle, a non-writable mode, or a save failure.
    pub fn file_truncate(
        &self,
        session: &LoomSession,
        file: u64,
        size: u64,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            loom.file_truncate(file, size)?;
            save_loom(loom)
        })
    }

    /// Flush buffered writes on open file handle `file`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or handle, or an engine failure.
    pub fn file_flush(&self, session: &LoomSession, file: u64) -> Result<(), LoomError> {
        self.with_session(session, |loom| loom.file_flush(file))
    }

    /// The engine version string (a build property).
    pub fn store_version(&self) -> String {
        loom_core::VERSION.to_string()
    }

    pub fn lifecycle_define_standard_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        kind: &str,
        version: &str,
        completion_predicate_digest: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let workspace_id = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Vcs),
                mint_workspace_id()?,
            )?;
            let profile_id = workspace_id.to_string();
            let definition = loom_lifecycle::define_standard_lifecycle(
                loom,
                workspace_id,
                loom_lifecycle::StandardLifecycleRequest {
                    workspace_id: &profile_id,
                    kind,
                    version,
                    completion_predicate_digest,
                },
            )?;
            save_loom(loom)?;
            json_string(&definition)
        })
    }

    pub fn lifecycle_define_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        definition: &[u8],
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let workspace_id = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Vcs),
                mint_workspace_id()?,
            )?;
            let profile_id = workspace_id.to_string();
            let definition =
                loom_lifecycle::define_lifecycle(loom, workspace_id, &profile_id, definition)?;
            save_loom(loom)?;
            json_string(&definition)
        })
    }

    pub fn lifecycle_instantiate_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        instance_id: &str,
        definition_id: &str,
        subject_refs: Vec<String>,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let workspace_id = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Vcs))?;
            let profile_id = workspace_id.to_string();
            let instance = loom_lifecycle::instantiate(
                loom,
                workspace_id,
                &profile_id,
                instance_id,
                definition_id,
                subject_refs,
            )?;
            save_loom(loom)?;
            json_string(&instance)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn lifecycle_transition_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        instance_id: &str,
        transition_id: &str,
        to_stage_id: &str,
        actor_principal_id: Option<&str>,
        gate_evaluations_json: &str,
        snapshot_digest: Option<&str>,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let workspace_id = loom
                .registry()
                .open(&ns_selector(workspace, FacetKind::Vcs))?;
            let profile_id = workspace_id.to_string();
            let now = now_ms();
            let gate_evaluations = parse_lifecycle_gate_evaluations(gate_evaluations_json, now)?;
            let actor = actor_principal_id.unwrap_or(&profile_id);
            let result = loom_lifecycle::transition(
                loom,
                workspace_id,
                loom_lifecycle::LifecycleTransitionRequest {
                    workspace_id: &profile_id,
                    instance_id,
                    transition_id,
                    to_stage_id,
                    actor_principal_id: actor,
                    gate_evaluations,
                    snapshot_digest,
                    recorded_at_ms: now,
                },
            )?;
            save_loom(loom)?;
            json_string(&result)
        })
    }

    pub fn refs_reconcile_json(
        &self,
        session: &LoomSession,
        workspace: &str,
        max: usize,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            let workspace_id = loom.registry().open(&ns_selector_by_name(workspace))?;
            let profile_id = workspace_id.to_string();
            let principal = loom.effective_principal()?.unwrap_or(workspace_id);
            let state_before = loom.export_state();
            let result = (|| {
                let ticket =
                    reconcile_ticket_references(loom, workspace_id, &profile_id, now_ms(), max)?;
                let chat = reconcile_chat_references(
                    loom,
                    workspace_id,
                    &profile_id,
                    now_ms(),
                    max.saturating_sub(ticket.processed as usize),
                )?;
                Ok((ticket, chat))
            })();
            let (ticket, chat) = match result {
                Ok(summary) => summary,
                Err(error) => {
                    loom.import_state(&state_before)?;
                    return Err(error);
                }
            };
            let summary = loom_reference::ReconciliationSummary {
                pending: chat.pending,
                resolved: chat.resolved,
                failed: chat.failed,
                processed: ticket.processed.saturating_add(chat.processed),
            };
            if summary.processed > 0 {
                let target = format!(
                    "workspace={profile_id};processed={};resolved={};failed={};pending={}",
                    summary.processed, summary.resolved, summary.failed, summary.pending
                );
                let result = (|| {
                    let saved = loom.save_state_objects()?;
                    loom_store::put_saved_state_and_audit(
                        loom.store(),
                        saved,
                        vec![WorkflowAuditWrite {
                            principal: Some(principal),
                            action: "refs.reconcile".to_string(),
                            target: Some(target),
                        }],
                    )
                })();
                if let Err(error) = result {
                    loom.import_state(&state_before)?;
                    return Err(error);
                }
            } else {
                save_loom(loom)?;
            }
            json_string(&summary)
        })
    }

    /// The protected-ref policies in `workspace`, as `(ref_name, policy)` pairs.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or for an unknown session.
    pub fn protected_ref_list(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<(String, ProtectedRefPolicy)>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.protected_ref_policies(ns)
        })
    }

    /// The protected-ref policy for exact `ref_name`, or `None` when unset.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or for an unknown session.
    pub fn protected_ref_get(
        &self,
        session: &LoomSession,
        workspace: &str,
        ref_name: &str,
    ) -> Result<Option<ProtectedRefPolicy>, LoomError> {
        self.with_session(session, |loom| {
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.protected_ref_policy(ns, ref_name)
        })
    }

    /// Set the protected-ref `policy` for exact `ref_name`, and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace, `INVALID_ARGUMENT` for a bad ref
    /// name) or a save failure.
    pub fn protected_ref_set(
        &self,
        session: &LoomSession,
        workspace: &str,
        ref_name: &str,
        policy: ProtectedRefPolicy,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            loom.set_protected_ref_policy(ns, ref_name, policy)?;
            save_loom(loom)
        })
    }

    /// Remove the protected-ref policy for `ref_name`; returns whether one was set. Persists on a
    /// change.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`NOT_FOUND` for an absent workspace) or a save failure.
    pub fn protected_ref_remove(
        &self,
        session: &LoomSession,
        workspace: &str,
        ref_name: &str,
    ) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let ns = loom.registry().open(&ns_selector_by_name(workspace))?;
            let removed = loom.remove_protected_ref_policy(ns, ref_name)?;
            if removed {
                save_loom(loom)?;
            }
            Ok(removed)
        })
    }

    /// The access-control grants configured on the store.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or a decode failure.
    pub fn acl_list(&self, session: &LoomSession) -> Result<Vec<AclGrant>, LoomError> {
        self.with_session(session, |loom| {
            Ok(loom
                .store()
                .acl_store()?
                .map(|acl| acl.grants().to_vec())
                .unwrap_or_default())
        })
    }

    /// Add an ACL `grant` to the store's control plane.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`INVALID_ARGUMENT` for an empty rights set) or for an unknown session.
    pub fn acl_grant(&self, session: &LoomSession, grant: AclGrant) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let mut acl = loom.store().acl_store()?.unwrap_or_default();
            acl.grant(grant)?;
            loom.store().save_acl_store(&acl)?;
            loom.set_acl_store(acl);
            Ok(())
        })
    }

    /// Revoke a matching ACL grant; returns whether one was removed.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or a store failure.
    pub fn acl_revoke(&self, session: &LoomSession, grant: &AclGrant) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let mut acl = loom.store().acl_store()?.unwrap_or_default();
            let removed = acl.revoke(grant);
            if removed {
                loom.store().save_acl_store(&acl)?;
                loom.set_acl_store(acl);
            }
            Ok(removed)
        })
    }

    /// Acquire a lock for the authenticated store session.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`LOCKED` on contention, `INVALID_ARGUMENT` for a zero lease).
    pub fn lock_acquire(
        &self,
        session: &LoomSession,
        key: &[u8],
        mode: LockMode,
        lease_ms: u64,
        wait_ms: u64,
    ) -> Result<LockToken, LoomError> {
        self.locks.acquire(
            self.lock_owner_for_session(session)?,
            key.to_vec(),
            mode,
            lease_ms,
            wait_ms,
        )
    }

    /// Refresh a held lock's lease, returning the updated token.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`LOCK_LEASE_EXPIRED`, `LOCK_NOT_HELD`) or a zero lease.
    pub fn lock_refresh(
        &self,
        session: &LoomSession,
        token: &LockToken,
        lease_ms: u64,
    ) -> Result<LockToken, LoomError> {
        self.locks
            .refresh(&self.lock_owner_for_session(session)?, token, lease_ms)
    }

    /// Release a held lock.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`LOCK_LEASE_EXPIRED`, `LOCK_NOT_HELD`).
    pub fn lock_release(&self, session: &LoomSession, token: &LockToken) -> Result<(), LoomError> {
        self.locks
            .release(&self.lock_owner_for_session(session)?, token)
    }

    pub fn lock_owner_for_session(&self, session: &LoomSession) -> Result<LockOwner, LoomError> {
        let key = handle_key(session)?;
        let (auth_view, logical_session) = self
            .sessions
            .lock()
            .expect("session lock")
            .get(&key)
            .map(|slot| (slot.auth_view.clone(), slot.lock_owner_session.clone()))
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown session handle"))?;
        match auth_view {
            LocalSessionAuthView::Authenticated {
                principal,
                session_id,
            } => Ok(LockOwner {
                principal: principal.to_string(),
                session: session_id,
            }),
            LocalSessionAuthView::Clear => Ok(LockOwner {
                principal: "unauthenticated-root".to_string(),
                session: logical_session
                    .unwrap_or_else(|| format!("{}:{key}", self.lock_session_namespace)),
            }),
            LocalSessionAuthView::Preserve => {
                let principal = self.with_session(session, |loom| {
                    Ok(loom
                        .effective_principal()?
                        .map(|principal| principal.to_string())
                        .unwrap_or_else(|| "unauthenticated-root".to_string()))
                })?;
                Ok(LockOwner {
                    principal,
                    session: format!("{}:{key}", self.lock_session_namespace),
                })
            }
        }
    }

    /// Bind an unauthenticated engine handle to a stable runtime-owned logical lock session.
    pub fn bind_clear_logical_lock_session(
        &self,
        session: &LoomSession,
        logical_session: String,
    ) -> Result<(), LoomError> {
        let key = handle_key(session)?;
        let auth_view = self
            .sessions
            .lock()
            .expect("session lock")
            .get(&key)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown session handle"))?
            .auth_view
            .clone();
        if matches!(auth_view, LocalSessionAuthView::Preserve) {
            let principal = self.with_session(session, |loom| loom.effective_principal())?;
            if principal.is_some() {
                return Err(LoomError::new(
                    Code::PermissionDenied,
                    "logical clear-session binding requires unauthenticated-root mode",
                ));
            }
        } else if !matches!(auth_view, LocalSessionAuthView::Clear) {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "logical clear-session binding requires unauthenticated-root mode",
            ));
        }
        let mut sessions = self.sessions.lock().expect("session lock");
        let slot = sessions
            .get_mut(&key)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown session handle"))?;
        slot.auth_view = LocalSessionAuthView::Clear;
        slot.lock_owner_session = Some(logical_session);
        Ok(())
    }

    /// Authenticate and bind an engine handle to a runtime-owned logical session identity.
    pub fn authenticate_passphrase_for_logical_session(
        &self,
        session: &LoomSession,
        principal: PrincipalId,
        passphrase: &[u8],
        logical_session: String,
    ) -> Result<(), LoomError> {
        let key = handle_key(session)?;
        let passphrase = std::str::from_utf8(passphrase)
            .map_err(|_| LoomError::new(Code::InvalidArgument, "passphrase is not valid utf-8"))?;
        self.with_session(session, |loom| {
            let mut identity = loom.store().identity_store()?.ok_or_else(|| {
                LoomError::new(
                    Code::Unsupported,
                    "store is in unauthenticated-root mode; no identity is configured",
                )
            })?;
            let bound =
                identity.authenticate_passphrase(principal, passphrase, logical_session.clone())?;
            loom.set_session(bound.id);
            loom.set_identity_store(identity);
            Ok(())
        })?;
        let mut sessions = self.sessions.lock().expect("session lock");
        let slot = sessions
            .get_mut(&key)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown session handle"))?;
        slot.auth_view = LocalSessionAuthView::Authenticated {
            principal,
            session_id: logical_session,
        };
        Ok(())
    }

    /// List the principals recorded in the store's identity control plane. An unauthenticated-root
    /// store carries no identity and yields an empty list.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or a control-plane decode failure.
    pub fn identity_list(&self, session: &LoomSession) -> Result<Vec<Principal>, LoomError> {
        self.with_session(session, |loom| {
            Ok(loom
                .store()
                .identity_store()?
                .map(|identity| identity.principals().cloned().collect())
                .unwrap_or_default())
        })
    }

    /// Report whether the store's identity control plane is in authenticated mode. An
    /// unauthenticated-root store reports `false`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or a control-plane decode failure.
    pub fn identity_authenticated(&self, session: &LoomSession) -> Result<bool, LoomError> {
        self.with_session(session, |loom| {
            Ok(loom
                .store()
                .identity_store()?
                .map(|identity| identity.authenticated_mode())
                .unwrap_or(false))
        })
    }

    /// Add a principal to the store's identity control plane and persist it, returning the minted
    /// principal id. The change is also installed on the in-process engine so later authorization sees
    /// it.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when the store has no identity configured, `ALREADY_EXISTS`
    /// on id collision, `INVALID_ARGUMENT` for an empty name) or for an unknown session.
    pub fn identity_add_principal(
        &self,
        session: &LoomSession,
        principal_handle: &str,
        name: &str,
        kind: PrincipalKind,
    ) -> Result<PrincipalId, LoomError> {
        let id = mint_workspace_id()?;
        self.with_session(session, |loom| {
            let mut identity = loom.store().identity_store()?.ok_or_else(|| {
                LoomError::new(
                    Code::Unsupported,
                    "store is in unauthenticated-root mode; no identity is configured",
                )
            })?;
            identity.add_principal_with_handle(id, principal_handle, name, kind)?;
            loom.store().save_identity_store(&identity)?;
            loom.set_identity_store(identity);
            Ok(id)
        })
    }

    /// Rename a principal's durable handle in the store's identity control plane and persist it, also
    /// installing the change on the in-process engine.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when no identity is configured, or an engine error for an
    /// unknown principal or a reserved handle) or for an unknown session.
    pub fn identity_rename_principal_handle(
        &self,
        session: &LoomSession,
        principal: PrincipalId,
        handle: &str,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            let mut identity = loom.store().identity_store()?.ok_or_else(|| {
                LoomError::new(
                    Code::Unsupported,
                    "store is in unauthenticated-root mode; no identity is configured",
                )
            })?;
            identity.rename_principal_handle(principal, handle)?;
            loom.store().save_identity_store(&identity)?;
            loom.set_identity_store(identity);
            Ok(())
        })
    }

    /// Set a principal's passphrase verifier in the store's identity control plane and persist it. The
    /// change is also installed on the in-process engine.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when the store has no identity configured, `NOT_FOUND` for an
    /// unknown principal, `INVALID_ARGUMENT` for an empty passphrase) or for an unknown session.
    pub fn identity_set_passphrase(
        &self,
        session: &LoomSession,
        principal: PrincipalId,
        passphrase: &[u8],
        salt: &[u8],
    ) -> Result<(), LoomError> {
        let passphrase = std::str::from_utf8(passphrase)
            .map_err(|_| LoomError::new(Code::InvalidArgument, "passphrase is not valid utf-8"))?;
        self.with_session(session, |loom| {
            let mut identity = loom.store().identity_store()?.ok_or_else(|| {
                LoomError::new(
                    Code::Unsupported,
                    "store is in unauthenticated-root mode; no identity is configured",
                )
            })?;
            identity.set_passphrase(principal, passphrase, salt)?;
            loom.store().save_identity_store(&identity)?;
            loom.set_identity_store(identity);
            Ok(())
        })
    }

    /// A clone of the store's identity control plane for read-only projection (the `identity_list`
    /// snapshot). Errors `UNSUPPORTED` when the store has no identity configured.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when no identity is configured) or for an unknown session.
    pub fn identity_snapshot_store(
        &self,
        session: &LoomSession,
    ) -> Result<IdentityStore, LoomError> {
        self.with_session(session, |loom| {
            loom.store().identity_store()?.ok_or_else(|| {
                LoomError::new(
                    Code::Unsupported,
                    "store is in unauthenticated-root mode; no identity is configured",
                )
            })
        })
    }

    /// Load the identity control plane, apply `mutate`, persist it, and install it on the engine,
    /// mirroring the `identity_add_principal` save path.
    fn identity_mutate<T>(
        &self,
        session: &LoomSession,
        mutate: impl FnOnce(&mut IdentityStore) -> Result<T, LoomError>,
    ) -> Result<T, LoomError> {
        self.with_session(session, |loom| {
            let mut identity = loom.store().identity_store()?.ok_or_else(|| {
                LoomError::new(
                    Code::Unsupported,
                    "store is in unauthenticated-root mode; no identity is configured",
                )
            })?;
            let out = mutate(&mut identity)?;
            loom.store().save_identity_store(&identity)?;
            loom.set_identity_store(identity);
            Ok(out)
        })
    }

    /// Assign `role` to `principal` in the identity control plane and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when no identity is configured, `NOT_FOUND` for an unknown
    /// principal/role) or for an unknown session.
    pub fn identity_assign_role(
        &self,
        session: &LoomSession,
        principal: PrincipalId,
        role: RoleId,
    ) -> Result<(), LoomError> {
        self.identity_mutate(session, |identity| identity.assign_role(principal, role))
    }

    /// Revoke `role` from `principal`; returns whether it was assigned. Persists.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when no identity is configured, `NOT_FOUND` for an unknown
    /// principal) or for an unknown session.
    pub fn identity_revoke_role(
        &self,
        session: &LoomSession,
        principal: PrincipalId,
        role: RoleId,
    ) -> Result<bool, LoomError> {
        self.identity_mutate(session, |identity| identity.revoke_role(principal, role))
    }

    /// Remove `principal` from the identity control plane and persist.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when no identity is configured, `NOT_FOUND` for an unknown
    /// principal) or for an unknown session.
    pub fn identity_remove_principal(
        &self,
        session: &LoomSession,
        principal: PrincipalId,
    ) -> Result<(), LoomError> {
        self.identity_mutate(session, |identity| {
            identity.remove_principal(principal).map(|_| ())
        })
    }

    /// Create an external credential for `principal` and persist, returning the credential id.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when no identity is configured, `ALREADY_EXISTS` on id
    /// collision, `NOT_FOUND`/`PERMISSION_DENIED` for the principal) or for an unknown session.
    /// Mutate the identity store under `session`, persist with an appended audit record, and return the
    /// audit sequence with the echoed action and redacted target. The closure returns the minted id (when
    /// the mutation creates one) and the audit target string. The acting principal is the session's
    /// effective principal.
    fn identity_mutate_audited(
        &self,
        session: &LoomSession,
        action: &str,
        mutate: impl FnOnce(&mut IdentityStore) -> Result<(Option<WorkspaceId>, String), LoomError>,
    ) -> Result<loom_wire::identity::IdentityAuditResult, LoomError> {
        self.with_session(session, |loom| {
            let actor = loom.effective_principal()?;
            let (snapshot, id, target) = {
                let identity = loom.identity_store_mut().ok_or_else(|| {
                    LoomError::new(
                        Code::Unsupported,
                        "store is in unauthenticated-root mode; no identity is configured",
                    )
                })?;
                let (id, target) = mutate(identity)?;
                (identity.clone(), id, target)
            };
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                actor,
                action,
                Some(&target),
            )?;
            Ok(loom_wire::identity::IdentityAuditResult {
                audit_seq: seq,
                id,
                action: action.to_string(),
                target: Some(target),
            })
        })
    }

    pub fn identity_create_external_credential(
        &self,
        session: &LoomSession,
        principal: PrincipalId,
        spec: ExternalCredentialSpec,
    ) -> Result<loom_wire::identity::IdentityAuditResult, LoomError> {
        self.identity_mutate_audited(session, "identity.external_credential.create", |identity| {
            let id = identity.create_external_credential(principal, spec)?.id;
            Ok((Some(id), format!("principal={principal};credential={id}")))
        })
    }

    /// Revoke external credential `id`, persist with an audit record, and return the audit result.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when no identity is configured, `NOT_FOUND` for an unknown
    /// credential) or for an unknown session.
    pub fn identity_revoke_external_credential(
        &self,
        session: &LoomSession,
        id: WorkspaceId,
    ) -> Result<loom_wire::identity::IdentityAuditResult, LoomError> {
        self.identity_mutate_audited(session, "identity.external_credential.revoke", |identity| {
            let credential = identity.revoke_external_credential(id)?;
            Ok((
                Some(credential.id),
                format!(
                    "principal={};credential={}",
                    credential.principal, credential.id
                ),
            ))
        })
    }

    /// Add a public key for `principal`, persist with an audit record, and return the audit result.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when no identity is configured, `ALREADY_EXISTS` on id
    /// collision, `NOT_FOUND`/`PERMISSION_DENIED`/`INVALID_ARGUMENT`) or for an unknown session.
    pub fn identity_add_public_key(
        &self,
        session: &LoomSession,
        principal: PrincipalId,
        spec: IdentityPublicKeySpec,
    ) -> Result<loom_wire::identity::IdentityAuditResult, LoomError> {
        self.identity_mutate_audited(session, "identity.public_key.add", |identity| {
            let id = identity.add_public_key(principal, spec)?.id;
            Ok((Some(id), format!("principal={principal};key={id}")))
        })
    }

    /// Revoke public key `id`, persist with an audit record, and return the audit result.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when no identity is configured, `NOT_FOUND` for an unknown
    /// key) or for an unknown session.
    pub fn identity_revoke_public_key(
        &self,
        session: &LoomSession,
        id: WorkspaceId,
    ) -> Result<loom_wire::identity::IdentityAuditResult, LoomError> {
        self.identity_mutate_audited(session, "identity.public_key.revoke", |identity| {
            let key = identity.revoke_public_key(id)?;
            Ok((
                Some(key.id),
                format!("principal={};key={}", key.principal, key.id),
            ))
        })
    }

    /// Create an app credential with a server-minted secret and persist with an audit record. Returns the
    /// stored (secret-free) record fields plus the one-time bearer token.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when no identity is configured, `NOT_FOUND`/`PERMISSION_DENIED`
    /// for the principal, `INVALID_ARGUMENT` for an empty label) or for an unknown session.
    pub fn identity_create_app_credential(
        &self,
        session: &LoomSession,
        principal: PrincipalId,
        label: &str,
    ) -> Result<loom_wire::identity::AppCredentialCreateResult, LoomError> {
        self.with_session(session, |loom| {
            let actor = loom.effective_principal()?;
            let (snapshot, credential, token) = {
                let identity = loom.identity_store_mut().ok_or_else(|| {
                    LoomError::new(
                        Code::Unsupported,
                        "store is in unauthenticated-root mode; no identity is configured",
                    )
                })?;
                let (credential, token) = mint_app_credential(identity, principal, label)?;
                (identity.clone(), credential, token)
            };
            let target = format!("principal={principal};credential={}", credential.id);
            let seq = loom.store().save_identity_store_audited(
                &snapshot,
                actor,
                "identity.app_credential.create",
                Some(&target),
            )?;
            Ok(loom_wire::identity::AppCredentialCreateResult {
                audit_seq: seq,
                id: credential.id,
                principal: credential.principal,
                label: credential.label,
                enabled: credential.enabled,
                secret_token: token,
            })
        })
    }

    /// Revoke an app credential and persist with an audit record; returns the audit result (no secret).
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when no identity is configured, `NOT_FOUND` for an unknown
    /// credential) or for an unknown session.
    pub fn identity_revoke_app_credential(
        &self,
        session: &LoomSession,
        id: WorkspaceId,
    ) -> Result<loom_wire::identity::IdentityAuditResult, LoomError> {
        self.identity_mutate_audited(session, "identity.app_credential.revoke", |identity| {
            let credential = identity.revoke_app_credential(id)?;
            Ok((
                Some(credential.id),
                format!(
                    "principal={};credential={}",
                    credential.principal, credential.id
                ),
            ))
        })
    }

    pub fn identity_force_detach_authority_json(
        &self,
        session: &LoomSession,
        principal: PrincipalId,
        generation: u64,
        reason: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            IdentityAuthorityPolicyService::force_detach_authority_json(
                loom, principal, generation, reason,
            )
        })
    }

    pub fn identity_authority_witness(
        &self,
        session: &LoomSession,
    ) -> Result<loom_core::IdentityAuthorityWitness, LoomError> {
        self.with_session(session, IdentityAuthorityPolicyService::authority_witness)
    }

    pub fn identity_list_authority_replication(
        &self,
        session: &LoomSession,
    ) -> Result<Vec<loom_store::AuthorityReplicationPolicy>, LoomError> {
        self.with_session(
            session,
            IdentityAuthorityPolicyService::authority_replication_policies,
        )
    }

    pub fn identity_replicate_authority_json(
        &self,
        session: &LoomSession,
        source: &str,
        become_authority: bool,
    ) -> Result<String, LoomError> {
        let source_loom = open_loom_read(source)?;
        source_loom.authorize_global_admin()?;
        let source_identity = source_loom.store().identity_store()?.ok_or_else(|| {
            LoomError::new(Code::Unsupported, "source identity store not initialized")
        })?;
        self.with_session(session, |loom| {
            IdentityAuthorityPolicyService::replicate_authority_json(
                loom,
                &source_identity,
                source,
                become_authority,
            )
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "matches the generated Identity IDL signature"
    )]
    pub fn identity_configure_authority_replication_json(
        &self,
        session: &LoomSession,
        id: &str,
        source: &str,
        disabled: bool,
        pull_on_start: bool,
        interval_ms: Option<u64>,
        jitter_ms: u64,
        backoff_ms: u64,
        publish_witness: bool,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            IdentityAuthorityPolicyService::configure_authority_replication_json(
                loom,
                id,
                source,
                disabled,
                pull_on_start,
                interval_ms,
                jitter_ms,
                backoff_ms,
                publish_witness,
            )
        })
    }

    pub fn identity_remove_authority_replication_json(
        &self,
        session: &LoomSession,
        id: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            IdentityAuthorityPolicyService::remove_authority_replication_json(loom, id)
        })
    }

    /// Authenticate a principal by passphrase and bind the resulting session to the engine, mirroring
    /// the local-open auth wiring (`attach_local_auth`). The session id is derived from the handle.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` when the store has no identity configured,
    /// `AUTHENTICATION_FAILED` on a bad passphrase, `NOT_FOUND` for an unknown principal,
    /// `INVALID_ARGUMENT` for a non-utf-8 passphrase) or for an unknown session.
    pub fn authenticate_passphrase(
        &self,
        session: &LoomSession,
        principal: PrincipalId,
        passphrase: &[u8],
    ) -> Result<(), LoomError> {
        let session_id = handle_key(session)?.to_string();
        let passphrase = std::str::from_utf8(passphrase)
            .map_err(|_| LoomError::new(Code::InvalidArgument, "passphrase is not valid utf-8"))?;
        self.with_session(session, |loom| {
            let mut identity = loom.store().identity_store()?.ok_or_else(|| {
                LoomError::new(
                    Code::Unsupported,
                    "store is in unauthenticated-root mode; no identity is configured",
                )
            })?;
            let bound = identity.authenticate_passphrase(principal, passphrase, session_id)?;
            loom.set_session(bound.id);
            loom.set_identity_store(identity);
            Ok(())
        })
    }

    /// Clear the engine's authenticated session binding. In authenticated mode this makes protected
    /// operations fail closed until a principal is authenticated again.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session.
    pub fn clear_authentication(&self, session: &LoomSession) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            loom.clear_session();
            Ok(())
        })
    }

    /// Add a passphrase-derived unlock credential (an at-rest DEK wrap) to the bound store. The store
    /// must already be encrypted and unlocked; an unencrypted store reports `UNSUPPORTED`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` for an unencrypted store, `E2E_LOCKED` when locked,
    /// `INVALID_ARGUMENT` for a non-utf-8 passphrase) or for an unknown session.
    pub fn key_add_wrap_keyed(
        &self,
        session: &LoomSession,
        passphrase: &[u8],
        salt: Vec<u8>,
        wrap_nonce: Vec<u8>,
        allow_no_recovery: bool,
    ) -> Result<(), LoomError> {
        let passphrase = std::str::from_utf8(passphrase)
            .map_err(|_| LoomError::new(Code::InvalidArgument, "passphrase is not valid utf-8"))?;
        let spec = KeySpec::passphrase(passphrase);
        self.with_session(session, |loom| {
            loom.store()
                .add_wrap(&spec, salt, wrap_nonce, allow_no_recovery)
        })
    }

    /// Add a raw-KEK unlock credential (an at-rest DEK wrap) to the bound store. The store must already
    /// be encrypted and unlocked; an unencrypted store reports `UNSUPPORTED`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` for an unencrypted store, `E2E_LOCKED` when locked) or for an
    /// unknown session.
    pub fn key_add_wrap_with_kek(
        &self,
        session: &LoomSession,
        kek: [u8; KEY_LEN],
        salt: Vec<u8>,
        wrap_nonce: Vec<u8>,
        allow_no_recovery: bool,
    ) -> Result<(), LoomError> {
        let spec = KeySpec::raw_kek(kek);
        self.with_session(session, |loom| {
            loom.store()
                .add_wrap(&spec, salt, wrap_nonce, allow_no_recovery)
        })
    }

    /// Remove an unlock credential (DEK wrap) by zero-based index. The store must already be encrypted
    /// and unlocked; an unencrypted store reports `UNSUPPORTED`.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`UNSUPPORTED` for an unencrypted store, `E2E_LOCKED` when locked) or for an
    /// unknown session.
    pub fn key_remove_wrap(
        &self,
        session: &LoomSession,
        index: usize,
        allow_no_recovery: bool,
    ) -> Result<(), LoomError> {
        self.with_session(session, |loom| {
            loom.store().remove_wrap(index, allow_no_recovery)
        })
    }

    // ---- Audit, certificate, and network-access administration ----

    pub fn audit_config_show_json(&self, session: &LoomSession) -> Result<String, LoomError> {
        self.with_session(session, SecurityAdminService::audit_config_show_json)
    }

    pub fn audit_config_set_json(
        &self,
        session: &LoomSession,
        retention_days: Option<u32>,
        legal_hold: Option<bool>,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            SecurityAdminService::audit_config_set_json(loom, retention_days, legal_hold)
        })
    }

    pub fn audit_list_json(&self, session: &LoomSession) -> Result<String, LoomError> {
        self.with_session(session, SecurityAdminService::audit_list_json)
    }

    pub fn audit_view_json(
        &self,
        session: &LoomSession,
        record: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            SecurityAdminService::audit_view_json(loom, record)
        })
    }

    pub fn certificate_list_json(&self, session: &LoomSession) -> Result<String, LoomError> {
        self.with_session(session, SecurityAdminService::certificate_list_json)
    }

    pub fn certificate_import_json(
        &self,
        session: &LoomSession,
        name: &str,
        cert_chain_pem: Vec<u8>,
        private_key_pem: Vec<u8>,
        trust_bundle_pem: Option<Vec<u8>>,
        force: bool,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            SecurityAdminService::certificate_import_json(
                loom,
                name,
                cert_chain_pem,
                private_key_pem,
                trust_bundle_pem,
                force,
            )
        })
    }

    pub fn certificate_export(
        &self,
        session: &LoomSession,
        name: &str,
        include_cert_chain: bool,
        include_private_key: bool,
        include_trust_bundle: bool,
        force: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            SecurityAdminService::certificate_export(
                loom,
                name,
                include_cert_chain,
                include_private_key,
                include_trust_bundle,
                force,
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn certificate_generate_self_signed_json(
        &self,
        session: &LoomSession,
        name: &str,
        dns_names: Vec<String>,
        ip_addresses: Vec<String>,
        cn: Option<&str>,
        days: u32,
        algorithm: &str,
        force: bool,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            SecurityAdminService::certificate_generate_self_signed_json(
                loom,
                name,
                dns_names,
                ip_addresses,
                cn,
                days,
                algorithm,
                force,
            )
        })
    }

    pub fn certificate_remove_json(
        &self,
        session: &LoomSession,
        name: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            SecurityAdminService::certificate_remove_json(loom, name)
        })
    }

    pub fn certificate_audit_json(
        &self,
        session: &LoomSession,
        name: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            SecurityAdminService::certificate_audit_json(loom, name)
        })
    }

    pub fn network_access_list_json(&self, session: &LoomSession) -> Result<String, LoomError> {
        self.with_session(session, SecurityAdminService::network_access_list_json)
    }

    pub fn network_access_set_json(
        &self,
        session: &LoomSession,
        name: &str,
        description: Option<&str>,
        default_action: &str,
        rules_json: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            SecurityAdminService::network_access_set_json(
                loom,
                name,
                description,
                default_action,
                rules_json,
            )
        })
    }

    pub fn network_access_remove_json(
        &self,
        session: &LoomSession,
        name: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            SecurityAdminService::network_access_remove_json(loom, name)
        })
    }

    pub fn network_access_audit_json(
        &self,
        session: &LoomSession,
        name: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            SecurityAdminService::network_access_audit_json(loom, name)
        })
    }

    pub fn serve_listener_configure_json(
        &self,
        session: &LoomSession,
        request_json: &str,
    ) -> Result<String, LoomError> {
        self.serve_listener_configure_json_with_commit(
            session,
            request_json,
            loom_store::put_saved_state_served_listener_controls_audited,
        )
    }

    fn serve_listener_configure_json_with_commit(
        &self,
        session: &LoomSession,
        request_json: &str,
        commit: impl FnOnce(
            &FileStore,
            loom_core::vcs::SavedStateObjects,
            &loom_store::ServedListenerRecord,
            Vec<WorkflowControlWrite>,
            Vec<WorkflowAuditWrite>,
            Option<WorkspaceId>,
            &str,
            Option<&str>,
        ) -> Result<loom_store::SavedStateAndAuditReceipt, LoomError>,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let actor = loom.effective_principal()?;
            let request: crate::serve_config::ServeListenerConfigureRequest =
                serde_json::from_str(request_json).map_err(|err| {
                    LoomError::new(
                        Code::InvalidArgument,
                        format!("serve listener request: {err}"),
                    )
                })?;
            crate::serve_config::validate_bind(&request.bind)?;
            let surface = crate::serve_config::normalize_surface(&request.surface)?;
            crate::serve_config::validate_selector_shape(surface, &request.selectors)?;
            let transport =
                crate::serve_config::normalize_transport(surface, request.transport.as_deref())?;
            crate::serve_config::validate_transport(surface, transport)?;
            let profile = crate::serve_config::normalize_profile(
                surface,
                transport,
                request.profile.as_deref(),
                request.mode.as_deref(),
            )?;
            let mut record = FileStore::served_listener_record_with_profile(
                surface,
                request.selectors.clone(),
                transport,
                profile,
                &request.bind,
                request.enabled,
            )?;
            crate::serve_config::apply_listener_policy(&mut record, request);
            crate::serve_config::validate_listener_references(loom, &record)?;
            let mut candidate =
                loom.fork_state_into(loom_core::provider::PlanningObjectStore::new(loom.store()))?;
            crate::serve_config::configure_memcached_cache_mode(
                &mut candidate,
                |candidate, workspace| {
                    candidate.registry_mut().ensure_for_write(
                        &ns_selector(workspace, FacetKind::Kv),
                        mint_workspace_id()?,
                    )
                },
                surface,
                record.profile.as_deref(),
                &record.selectors,
            )?;
            let drive_policy = if surface == "drive" {
                let workspace = candidate
                    .registry()
                    .open(&ns_selector_by_name(&record.selectors[0]))?;
                Some(serve_drive_policy_registry_write(
                    loom.store(),
                    actor,
                    workspace,
                )?)
            } else {
                None
            };
            let mut objects = candidate.store().objects()?;
            let (root, state_objects) = candidate.save_state_objects()?;
            objects.extend(state_objects);
            let candidate_state = candidate.export_state();
            let saved = (root, objects);
            let target = crate::serve_config::listener_target(&record);
            let (controls, audits) = drive_policy
                .map(|policy| (vec![policy.0], vec![policy.1]))
                .unwrap_or_else(|| (Vec::new(), Vec::new()));
            let receipt = commit(
                loom.store(),
                saved,
                &record,
                controls,
                audits,
                actor,
                "serve.listener.configure",
                Some(&target),
            )?;
            drop(candidate);
            loom.import_engine_state_preserving_mutable_overlay(&candidate_state)?;
            let seq = receipt
                .audit_sequences
                .first()
                .copied()
                .ok_or_else(|| LoomError::corrupt("served listener audit sequence missing"))?;
            record.last_modified_audit_seq = Some(seq);
            crate::serve_config::listener_json(&record, seq)
        })
    }

    pub fn serve_listener_list_json(&self, session: &LoomSession) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let actor = loom.effective_principal()?;
            let listeners = loom.store().served_listeners()?;
            let seq = loom
                .store()
                .audit_append(actor, "serve.listener.list", Some("listeners"))?;
            let mut out = format!("{{\"seq\":{seq},\"listeners\":[");
            for (idx, listener) in listeners.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                out.push_str(&crate::serve_config::listener_record_json(listener)?);
            }
            out.push_str("]}");
            Ok(out)
        })
    }

    pub fn serve_listener_set_enabled_json(
        &self,
        session: &LoomSession,
        listener_id: &str,
        enabled: bool,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let actor = loom.effective_principal()?;
            let mut record = loom
                .store()
                .served_listener(listener_id)?
                .ok_or_else(|| LoomError::not_found("served listener not found"))?;
            record.enabled = enabled;
            let target = crate::serve_config::listener_target(&record);
            let action = if enabled {
                "serve.listener.enable"
            } else {
                "serve.listener.disable"
            };
            let seq =
                loom.store()
                    .save_served_listener_audited(&record, actor, action, Some(&target))?;
            record.last_modified_audit_seq = Some(seq);
            crate::serve_config::listener_json(&record, seq)
        })
    }

    pub fn serve_listener_remove_json(
        &self,
        session: &LoomSession,
        listener_id: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let actor = loom.effective_principal()?;
            let record = loom
                .store()
                .served_listener(listener_id)?
                .ok_or_else(|| LoomError::not_found("served listener not found"))?;
            let target = crate::serve_config::listener_target(&record);
            let seq = loom.store().remove_served_listener_audited(
                listener_id,
                actor,
                "serve.listener.remove",
                Some(&target),
            )?;
            Ok(format!(
                "{{\"seq\":{seq},\"id\":{}}}",
                json_string(&listener_id)?
            ))
        })
    }

    pub fn serve_web_route_list_json(
        &self,
        session: &LoomSession,
        listener_id: &str,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let actor = loom.effective_principal()?;
            let record = crate::serve_config::require_web_listener_record(loom, listener_id)?;
            let web_listener =
                crate::serve_config::web_listener_from_record(loom, &record, |loom, workspace| {
                    loom.registry().open(&ns_selector_by_name(workspace))
                })?;
            let seq = loom.store().audit_append(
                actor,
                "serve.web.route.list",
                Some(&format!("listener={listener_id}")),
            )?;
            crate::serve_config::web_listener_json(&web_listener, Some(seq))
        })
    }

    pub fn serve_web_route_set_json(
        &self,
        session: &LoomSession,
        request_json: &str,
    ) -> Result<String, LoomError> {
        self.serve_web_route_set_json_with_commit(
            session,
            request_json,
            |store, key, value, actor, action, target| {
                store.control_set_audited(&key, value, actor, action, Some(target))
            },
        )
    }

    fn serve_web_route_set_json_with_commit(
        &self,
        session: &LoomSession,
        request_json: &str,
        commit: impl FnOnce(
            &FileStore,
            Vec<u8>,
            Vec<u8>,
            Option<WorkspaceId>,
            &str,
            &str,
        ) -> Result<u64, LoomError>,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let actor = loom.effective_principal()?;
            let request: crate::serve_config::ServeWebRouteSetRequest =
                serde_json::from_str(request_json).map_err(|err| {
                    LoomError::new(
                        Code::InvalidArgument,
                        format!("serve web route request: {err}"),
                    )
                })?;
            let record = crate::serve_config::require_web_listener_record(loom, &request.listener)?;
            let mut web_listener =
                crate::serve_config::web_listener_from_record(loom, &record, |loom, workspace| {
                    loom.registry().open(&ns_selector_by_name(workspace))
                })?;
            let workspace = request
                .workspace
                .as_deref()
                .map(|workspace| loom.registry().open(&ns_selector_by_name(workspace)))
                .transpose()?;
            let mut route = loom_substrate::web::WebRoute::new(
                request.route.clone(),
                vec![
                    loom_substrate::web::WebMethod::Get,
                    loom_substrate::web::WebMethod::Head,
                ],
                request.host.clone(),
                &request.prefix,
                &request.root,
                loom_substrate::web::WebRouteMode::StaticFile,
            )?;
            route.workspace = workspace;
            web_listener
                .routes
                .routes
                .retain(|existing| existing.route_id != route.route_id);
            web_listener.routes.routes.push(route);
            web_listener
                .routes
                .routes
                .sort_by(|left, right| left.route_id.cmp(&right.route_id));
            web_listener.routes =
                loom_substrate::web::WebRouteTable::new(web_listener.routes.routes)?;
            let key = crate::serve_config::web_listener_control_key(&web_listener.listener_id)?;
            let value = web_listener.encode()?;
            let target = format!("listener={};route={}", request.listener, request.route);
            let seq = commit(
                loom.store(),
                key,
                value,
                actor,
                "serve.web.route.set",
                &target,
            )?;
            crate::serve_config::web_listener_json(&web_listener, Some(seq))
        })
    }

    pub fn serve_web_route_remove_json(
        &self,
        session: &LoomSession,
        listener_id: &str,
        route_id: &str,
    ) -> Result<String, LoomError> {
        self.serve_web_route_remove_json_with_commit(
            session,
            listener_id,
            route_id,
            |store, key, value, actor, action, target| {
                store.control_set_audited(&key, value, actor, action, Some(target))
            },
        )
    }

    fn serve_web_route_remove_json_with_commit(
        &self,
        session: &LoomSession,
        listener_id: &str,
        route_id: &str,
        commit: impl FnOnce(
            &FileStore,
            Vec<u8>,
            Vec<u8>,
            Option<WorkspaceId>,
            &str,
            &str,
        ) -> Result<u64, LoomError>,
    ) -> Result<String, LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let actor = loom.effective_principal()?;
            let record = crate::serve_config::require_web_listener_record(loom, listener_id)?;
            let mut web_listener =
                crate::serve_config::web_listener_from_record(loom, &record, |loom, workspace| {
                    loom.registry().open(&ns_selector_by_name(workspace))
                })?;
            let before = web_listener.routes.routes.len();
            web_listener
                .routes
                .routes
                .retain(|existing| existing.route_id != route_id);
            if web_listener.routes.routes.len() == before {
                return Err(LoomError::not_found("web route not found"));
            }
            web_listener
                .routes
                .routes
                .sort_by(|left, right| left.route_id.cmp(&right.route_id));
            web_listener.routes =
                loom_substrate::web::WebRouteTable::new(web_listener.routes.routes)?;
            let key = crate::serve_config::web_listener_control_key(&web_listener.listener_id)?;
            let value = web_listener.encode()?;
            let target = format!("listener={listener_id};route={route_id}");
            let seq = commit(
                loom.store(),
                key,
                value,
                actor,
                "serve.web.route.remove",
                &target,
            )?;
            crate::serve_config::web_listener_json(&web_listener, Some(seq))
        })
    }

    // ---- StoreAdmin (server-owned store administration, specs/0067 §13, task 640) ----

    fn store_policy_result(
        policy: StorePolicy,
        audit_seq: Option<u64>,
    ) -> loom_wire::store_admin::StorePolicyResult {
        let facet_durability_overrides = FacetKind::ALL
            .into_iter()
            .filter_map(|facet| {
                policy.facet_durability_overrides[facet.stable_tag() as usize].map(|durability| {
                    loom_wire::store_admin::StoreFacetDurabilityAssignment { facet, durability }
                })
            })
            .collect();
        loom_wire::store_admin::StorePolicyResult {
            fips_required: policy.fips_required,
            default_durability: policy.default_durability,
            facet_durability_overrides,
            audit_seq,
        }
    }

    fn store_policy_audit_target(policy: &StorePolicy) -> String {
        let mut target = format!(
            "fips_required={},default_durability={}",
            policy.fips_required,
            policy.default_durability.as_str()
        );
        for facet in FacetKind::ALL {
            if let Some(durability) = policy.facet_durability_overrides[facet.stable_tag() as usize]
            {
                target.push(',');
                target.push_str(facet.as_str());
                target.push('=');
                target.push_str(durability.as_str());
            }
        }
        target
    }

    /// StoreAdmin: the store maintenance/size snapshot, as canonical `loom.store.stat.v1` CBOR. Requires
    /// global-admin authorization (fail-closed in authenticated mode).
    pub fn store_stat(&self, session: &LoomSession) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let store = loom.store();
            let status = store.maintenance_status()?;
            Ok(loom_wire::store_admin::store_stat_to_cbor(
                &loom_wire::store_admin::StoreStat {
                    object_count: status.object_count,
                    generation: status.generation,
                    physical_page_count: status.physical_page_count,
                    physical_bytes: status.physical_bytes,
                    reusable_free_pages: status.reusable_free_pages,
                    candidate_dead_pages: status.candidate_dead_pages,
                    last_validated_mark_epoch: status.last_validated_mark_epoch,
                    touched_segments: status.touched_segments.len() as u64,
                    candidate_segments: status.candidate_segments.len() as u64,
                    segment_overflow: u64::from(status.segment_overflow),
                },
            ))
        })
    }

    /// StoreAdmin: read the durable store policy (`loom.store.policy.v1`). Global-admin authorized.
    pub fn store_policy_get(&self, session: &LoomSession) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let policy = loom.store().store_policy()?;
            Ok(loom_wire::store_admin::store_policy_result_to_cbor(
                &Self::store_policy_result(policy, None),
            ))
        })
    }

    /// StoreAdmin: set the durable store policy, audited under the authenticated actor. Global-admin
    /// authorized; returns `loom.store.policy.v1` carrying the assigned audit seq.
    pub fn store_policy_set(
        &self,
        session: &LoomSession,
        update: &[u8],
    ) -> Result<Vec<u8>, LoomError> {
        let update = loom_wire::store_admin::store_policy_update_from_cbor(update)?;
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let actor = loom.effective_principal()?;
            let mut policy = loom.store().store_policy()?;
            if let Some(value) = update.fips_required {
                policy.fips_required = value;
            }
            if let Some(value) = update.default_durability {
                policy.set_default_durability(value)?;
            }
            for assignment in update.facet_durability_assignments {
                policy.set_facet_durability(assignment.facet, Some(assignment.durability))?;
            }
            for facet in update.clear_facet_durability {
                policy.set_facet_durability(facet, None)?;
            }
            let target = Self::store_policy_audit_target(&policy);
            let seq = loom.store().save_store_policy_audited(
                policy,
                actor,
                "store.policy.set",
                Some(&target),
            )?;
            Ok(loom_wire::store_admin::store_policy_result_to_cbor(
                &Self::store_policy_result(policy, Some(seq)),
            ))
        })
    }

    /// StoreAdmin: rekey the served store. The caller (server dispatch) supplies the server-generated
    /// `salt`/`wrap_nonce` (and, for `reseal`, a fresh `new_dek`); the plaintext DEK never leaves the
    /// server and is never returned to the client. Global-admin authorized and audited.
    ///
    /// Fast path (`reseal = false`) re-wraps the existing DEK under the new credential; a suite change
    /// requires `reseal`. `reseal = true` re-seals every object under a fresh DEK (and optional new
    /// AEAD suite) via the existing `rekey_reseal` store pass.
    #[allow(clippy::too_many_arguments)]
    pub fn store_rekey(
        &self,
        session: &LoomSession,
        request: &[u8],
        salt: Vec<u8>,
        wrap_nonce: Vec<u8>,
        new_dek: Option<[u8; KEY_LEN]>,
    ) -> Result<Vec<u8>, LoomError> {
        let request = loom_wire::store_admin::store_rekey_request_from_cbor(request)?;
        let spec = match request.credential {
            loom_wire::store_admin::StoreRekeyCredential::Passphrase(secret) => {
                let passphrase = std::str::from_utf8(&secret).map_err(|_| {
                    LoomError::new(Code::InvalidArgument, "new passphrase is not valid utf-8")
                })?;
                KeySpec::passphrase(passphrase)
            }
            loom_wire::store_admin::StoreRekeyCredential::RawKek(secret) => {
                KeySpec::raw_kek(secret)
            }
        };
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let actor = loom.effective_principal()?;
            let meta = loom.store().encryption_meta()?.ok_or_else(|| {
                LoomError::new(
                    Code::Unsupported,
                    "store is not encrypted; rekey requires an encrypted store",
                )
            })?;
            let (resealed, suite_str, bytes_before, bytes_after) = if request.reseal {
                let target_suite = match request.suite.as_deref() {
                    Some(s) => Suite::parse(s)?,
                    None => meta.active_suite,
                };
                let new_dek = new_dek.ok_or_else(|| {
                    LoomError::new(Code::Internal, "reseal requires a server-generated DEK")
                })?;
                let (new_meta, new_session) =
                    EncryptionMeta::create(&spec, target_suite, salt, new_dek, wrap_nonce)?;
                let stats = loom
                    .store_mut()
                    .rekey_reseal(new_meta.encode(), new_session)?;
                (
                    true,
                    target_suite.as_str().to_string(),
                    Some(stats.before),
                    Some(stats.after),
                )
            } else {
                if let Some(s) = request.suite.as_deref() {
                    let want = Suite::parse(s)?;
                    if want != meta.active_suite {
                        return Err(LoomError::new(
                            Code::InvalidArgument,
                            "changing the AEAD suite requires re-sealing every object; set reseal=true",
                        ));
                    }
                }
                loom.store().rekey(&spec, salt, wrap_nonce)?;
                (false, meta.active_suite.as_str().to_string(), None, None)
            };
            let target = format!("resealed={resealed} suite={suite_str}");
            let seq = loom.store().control_set_audited(
                b"store.rekey.last",
                target.clone().into_bytes(),
                actor,
                "store.rekey",
                Some(&target),
            )?;
            Ok(loom_wire::store_admin::store_rekey_result_to_cbor(
                &loom_wire::store_admin::StoreRekeyResult {
                    audit_seq: seq,
                    resealed,
                    suite: suite_str,
                    bytes_before,
                    bytes_after,
                },
            ))
        })
    }

    pub fn store_bundle_import(
        &self,
        session: &LoomSession,
        bundle_bytes: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let (bytes, persist) =
                apply_store_bundle_import_generated(loom, bundle_bytes, dry_run)?;
            if persist {
                save_loom(loom)?;
            }
            Ok(bytes)
        })
    }

    fn maintenance_policy_record(
        policy: &loom_store::StoreMaintenancePolicy,
    ) -> loom_wire::store_admin::StoreMaintenancePolicyRecord {
        loom_wire::store_admin::StoreMaintenancePolicyRecord {
            min_candidate_pages: policy.min_candidate_pages,
            min_reusable_pages: policy.min_reusable_pages,
            interval_ms: policy.interval_ms,
            backoff_ms: policy.backoff_ms,
            max_segments: policy.max_segments,
            max_pages: policy.max_pages,
            full_compaction_enabled: policy.full_compaction_enabled,
            tail_trim_enabled: policy.tail_trim_enabled,
            tail_compaction_enabled: policy.tail_compaction_enabled,
            tail_compaction_max_pages: policy.tail_compaction_max_pages,
            tail_compaction_max_objects: policy.tail_compaction_max_objects,
            tail_compaction_max_bytes: policy.tail_compaction_max_bytes,
            tail_compaction_interval_ms: policy.tail_compaction_interval_ms,
            tail_compaction_backoff_ms: policy.tail_compaction_backoff_ms,
        }
    }

    fn maintenance_run_state_record(
        state: &loom_store::StoreMaintenanceRunState,
    ) -> loom_wire::store_admin::StoreMaintenanceRunStateRecord {
        loom_wire::store_admin::StoreMaintenanceRunStateRecord {
            last_run_ms: state.last_run_ms,
            next_eligible_ms: state.next_eligible_ms,
            last_skip_reason: state.last_skip_reason.clone(),
            last_error: state.last_error.clone(),
            last_tail_trim_attempted: state.last_tail_trim_attempted,
            last_tail_trim_pages: state.last_tail_trim_pages,
            last_tail_trim_bytes: state.last_tail_trim_bytes,
            last_tail_compaction_attempted: state.last_tail_compaction_attempted,
            last_tail_compaction_relocated_objects: state.last_tail_compaction_relocated_objects,
            last_tail_compaction_relocated_pages: state.last_tail_compaction_relocated_pages,
            last_tail_compaction_relocated_bytes: state.last_tail_compaction_relocated_bytes,
            last_tail_compaction_truncated_pages: state.last_tail_compaction_truncated_pages,
            last_tail_compaction_conflicts: state.last_tail_compaction_conflicts,
            last_shrink_skip_reason: state.last_shrink_skip_reason.clone(),
            last_progress_steps: state.last_progress_steps,
            last_yield_count: state.last_yield_count,
            last_overrun_count: state.last_overrun_count,
        }
    }

    fn maintenance_report_record(
        report: &loom_store::StoreMaintenanceReport,
    ) -> loom_wire::store_admin::StoreMaintenanceReportRecord {
        let status = &report.status;
        let group = &status.group_commit;
        let overlay = &report.overlay_health;
        let mvcc = &report.mvcc_snapshots;
        loom_wire::store_admin::StoreMaintenanceReportRecord {
            status: loom_wire::store_admin::StoreMaintenanceStatusRecord {
                generation: status.generation,
                object_count: status.object_count,
                physical_page_count: status.physical_page_count,
                physical_bytes: status.physical_bytes,
                reusable_free_pages: status.reusable_free_pages,
                candidate_dead_pages: status.candidate_dead_pages,
                tail_free_pages: status.tail_free_pages,
                tail_free_bytes: status.tail_free_bytes,
                last_validated_mark_epoch: status.last_validated_mark_epoch,
                touched_segments: status.touched_segments.clone(),
                candidate_segments: status.candidate_segments.clone(),
                segment_overflow: status.segment_overflow,
                group_commit: loom_wire::store_admin::StoreGroupCommitDiagnosticsRecord {
                    group_commit_batches_total: group.group_commit_batches_total,
                    group_commit_transactions_total: group.group_commit_transactions_total,
                    group_commit_records_total: group.group_commit_records_total,
                    fsync_total_micros: group.fsync_total_micros,
                    fsync_count: group.fsync_count,
                    write_lock_wait_total_micros: group.write_lock_wait_total_micros,
                    write_lock_wait_count: group.write_lock_wait_count,
                    pending_durable_window_transactions: group.pending_durable_window_transactions,
                    pending_durable_window_records: group.pending_durable_window_records,
                    pinned_reader_blockers: group.pinned_reader_blockers,
                },
            },
            overlay_health: loom_wire::store_admin::StoreMutableOverlayHealthRecord {
                current_generation: overlay.current_generation,
                current_record_count: overlay.current_record_count,
                tombstone_count: overlay.tombstone_count,
                live_checkpoint_references: overlay.live_checkpoint_references,
                reclaimable_overlay_pages: overlay.reclaimable_overlay_pages,
                blocked_reclamation_reasons: overlay.blocked_reclamation_reasons.clone(),
                hot_write_count: overlay.hot_write_count,
                active_writer_contention_indicators: overlay.active_writer_contention_indicators,
            },
            mvcc_snapshots: loom_wire::store_admin::StoreMvccSnapshotDiagnosticsRecord {
                active_snapshot_count: mvcc.active_snapshot_count,
                oldest_pinned_overlay_generation: mvcc
                    .oldest_pinned_overlay_generation
                    .map(|generation| generation.as_u64()),
                pins: mvcc
                    .pins
                    .iter()
                    .map(|pin| loom_wire::store_admin::StoreMvccSnapshotPinRecord {
                        pin_id: pin.pin_id,
                        identity: loom_wire::store_admin::StoreMvccSnapshotIdentityRecord {
                            overlay_generation: pin.identity.overlay_generation.as_u64(),
                            immutable_base_root: pin
                                .identity
                                .immutable_base_root
                                .map(|digest| digest.to_string()),
                        },
                        owner: pin.owner.clone(),
                    })
                    .collect(),
            },
            policy: Self::maintenance_policy_record(&report.policy),
            run_state: Self::maintenance_run_state_record(&report.run_state),
            mark_epoch: report.mark_epoch,
            mark_completed: report.mark_completed,
            marked_live_objects: report.marked_live_objects,
            marked_live_bytes: report.marked_live_bytes,
            live_bytes: report.live_bytes,
            candidate_reclaimable_bytes: report.candidate_reclaimable_bytes,
            reusable_free_bytes: report.reusable_free_bytes,
            overlay_obsolete_record_count: report.overlay_obsolete_record_count,
            overlay_obsolete_page_count: report.overlay_obsolete_page_count,
            tail_free_pages: report.tail_free_pages,
            tail_free_bytes: report.tail_free_bytes,
            tail_trim_eligible: report.tail_trim_eligible,
            tail_blocked_by_live_objects: report.tail_blocked_by_live_objects,
            tail_compaction_eligible: report.tail_compaction_eligible,
            full_compaction_required_for_shrink: report.full_compaction_required_for_shrink,
            tail_trim_attempted: report.tail_trim_attempted,
            tail_trim_pages: report.tail_trim_pages,
            tail_trim_bytes: report.tail_trim_bytes,
            tail_compaction_attempted: report.tail_compaction_attempted,
            tail_compaction_relocated_objects: report.tail_compaction_relocated_objects,
            tail_compaction_relocated_pages: report.tail_compaction_relocated_pages,
            tail_compaction_relocated_bytes: report.tail_compaction_relocated_bytes,
            tail_compaction_truncated_pages: report.tail_compaction_truncated_pages,
            tail_compaction_conflicts: report.tail_compaction_conflicts,
            last_shrink_skip_reason: report.last_shrink_skip_reason.clone(),
            retained_control_roots: report.retained_control_roots,
            derived_payload_count: report.derived_payload_count,
            growth_domains: report
                .growth_domains
                .iter()
                .map(|domain| loom_wire::store_admin::StoreGrowthDomainRecord {
                    domain: domain.domain.clone(),
                    current_records: domain.current_records,
                    obsolete_records: domain.obsolete_records,
                    payload_bytes: domain.payload_bytes,
                })
                .collect(),
            eligible: report.eligible,
            reason: report.reason.clone(),
        }
    }

    fn live_root_diagnostics_record(
        diagnostics: &loom_core::LiveRootDiagnostics,
    ) -> loom_wire::store_admin::StoreLiveRootDiagnosticsRecord {
        loom_wire::store_admin::StoreLiveRootDiagnosticsRecord {
            sample_limit: u64::try_from(diagnostics.sample_limit).unwrap_or(u64::MAX),
            classes: diagnostics
                .classes
                .iter()
                .map(
                    |class| loom_wire::store_admin::StoreLiveRootClassDiagnosticsRecord {
                        class: class.class.to_string(),
                        count: class.count,
                        examples: class
                            .examples
                            .iter()
                            .map(
                                |example| loom_wire::store_admin::StoreLiveRootExampleRecord {
                                    id: example.id.clone(),
                                    digest: example.digest.to_string(),
                                },
                            )
                            .collect(),
                        truncated: class.truncated,
                    },
                )
                .collect(),
        }
    }

    pub fn audit_compact(
        &self,
        session: &LoomSession,
        through_seq: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let actor = loom.effective_principal()?;
            let stats = loom.store().audit_prune_through(actor, through_seq)?;
            Ok(loom_wire::audit::audit_compact_result_to_cbor(
                &loom_wire::audit::AuditCompactResult {
                    pruned: stats.pruned,
                    checkpoint_seq: stats.checkpoint_seq,
                    checkpoint_hash: stats.checkpoint_hash,
                    audit_seq: stats.audit_seq,
                },
            ))
        })
    }

    pub fn store_maintenance_status(
        &self,
        session: &LoomSession,
        request_bytes: &[u8],
    ) -> Result<Vec<u8>, LoomError> {
        let clock = Arc::clone(&self.store_maintenance_clock);
        self.with_session(session, |loom| {
            let request =
                loom_wire::store_admin::store_maintenance_status_request_from_cbor(request_bytes)?;
            let report = loom.store().store_maintenance_report(clock.now_ms())?;
            let live_root_diagnostics = if request.include_live_root_diagnostics {
                Some(Self::live_root_diagnostics_record(
                    &loom_store::maintenance_live_root_diagnostics(loom)?,
                ))
            } else {
                None
            };
            Ok(
                loom_wire::store_admin::store_maintenance_status_result_to_cbor(
                    &loom_wire::store_admin::StoreMaintenanceStatusResult {
                        report: Self::maintenance_report_record(&report),
                        live_root_diagnostics,
                    },
                ),
            )
        })
    }

    pub fn store_maintenance_policy_set(
        &self,
        session: &LoomSession,
        update_bytes: &[u8],
    ) -> Result<Vec<u8>, LoomError> {
        let clock = Arc::clone(&self.store_maintenance_clock);
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let update =
                loom_wire::store_admin::store_maintenance_policy_update_from_cbor(update_bytes)?;
            let mut policy = loom.store().store_maintenance_policy()?;
            if let Some(value) = update.min_candidate_pages {
                policy.min_candidate_pages = value;
            }
            if let Some(value) = update.min_reusable_pages {
                policy.min_reusable_pages = value;
            }
            if let Some(value) = update.interval_ms {
                policy.interval_ms = value;
            }
            if let Some(value) = update.backoff_ms {
                policy.backoff_ms = value;
            }
            if let Some(value) = update.max_segments {
                policy.max_segments = value;
            }
            if let Some(value) = update.max_pages {
                policy.max_pages = value;
            }
            if let Some(value) = update.full_compaction_enabled {
                policy.full_compaction_enabled = value;
            }
            if let Some(value) = update.tail_trim_enabled {
                policy.tail_trim_enabled = value;
            }
            if let Some(value) = update.tail_compaction_enabled {
                policy.tail_compaction_enabled = value;
            }
            if let Some(value) = update.tail_compaction_max_pages {
                policy.tail_compaction_max_pages = value;
            }
            if let Some(value) = update.tail_compaction_max_objects {
                policy.tail_compaction_max_objects = value;
            }
            if let Some(value) = update.tail_compaction_max_bytes {
                policy.tail_compaction_max_bytes = value;
            }
            if let Some(value) = update.tail_compaction_interval_ms {
                policy.tail_compaction_interval_ms = value;
            }
            if let Some(value) = update.tail_compaction_backoff_ms {
                policy.tail_compaction_backoff_ms = value;
            }
            let actor = loom.effective_principal()?;
            loom.store().save_store_maintenance_policy_audited(
                policy,
                actor,
                "store.maintenance.policy.set",
                Some("updated"),
            )?;
            let report = loom.store().store_maintenance_report(clock.now_ms())?;
            Ok(
                loom_wire::store_admin::store_maintenance_status_result_to_cbor(
                    &loom_wire::store_admin::StoreMaintenanceStatusResult {
                        report: Self::maintenance_report_record(&report),
                        live_root_diagnostics: None,
                    },
                ),
            )
        })
    }

    pub fn store_maintenance_run(
        &self,
        session: &LoomSession,
        request_bytes: &[u8],
    ) -> Result<Vec<u8>, LoomError> {
        let clock = Arc::clone(&self.store_maintenance_clock);
        self.with_session(session, |loom| {
            loom.authorize_global_admin()?;
            let request =
                loom_wire::store_admin::store_maintenance_run_request_from_cbor(request_bytes)?;
            let outcome = run_store_maintenance_once_with_clock(
                loom,
                true,
                request.max_segments,
                request.max_pages,
                None,
                None,
                clock.as_ref(),
            )?;
            let actor = loom.effective_principal()?;
            loom.store().control_set_audited(
                b"store.maintenance.run.last",
                format!("{:?}", outcome.kind).into_bytes(),
                actor,
                "store.maintenance.run",
                Some("manual=true"),
            )?;
            let kind = match outcome.kind {
                loom_store::StoreMaintenanceRunKind::Skipped => {
                    loom_wire::store_admin::StoreMaintenanceRunKind::Skipped
                }
                loom_store::StoreMaintenanceRunKind::Marked => {
                    loom_wire::store_admin::StoreMaintenanceRunKind::Marked
                }
                loom_store::StoreMaintenanceRunKind::Compacted => {
                    loom_wire::store_admin::StoreMaintenanceRunKind::Compacted
                }
                loom_store::StoreMaintenanceRunKind::Reclaimed => {
                    loom_wire::store_admin::StoreMaintenanceRunKind::Reclaimed
                }
            };
            Ok(
                loom_wire::store_admin::store_maintenance_run_result_to_cbor(
                    &loom_wire::store_admin::StoreMaintenanceRunResult {
                        kind,
                        reason: outcome.reason,
                        visited: outcome.visited,
                        pending: outcome.pending,
                        before: outcome.before,
                        after: outcome.after,
                        reclaimed: outcome.reclaimed,
                        required_temp_bytes: outcome.required_temp_bytes,
                        available_temp_bytes: outcome.available_temp_bytes,
                        segments_reclaimed: outcome.segments_reclaimed,
                        pages_freed: outcome.pages_freed,
                        tail_trim_pages: outcome.tail_trim_pages,
                        tail_trim_bytes: outcome.tail_trim_bytes,
                        tail_compaction_attempted: outcome.tail_compaction_attempted,
                        tail_compaction_relocated_objects: outcome
                            .tail_compaction_relocated_objects,
                        tail_compaction_relocated_pages: outcome.tail_compaction_relocated_pages,
                        tail_compaction_truncated_pages: outcome.tail_compaction_truncated_pages,
                        objects_relocated: outcome.objects_relocated,
                        objects_dropped: outcome.objects_dropped,
                        elapsed_ms: outcome.elapsed_ms,
                        run_state: Self::maintenance_run_state_record(&outcome.run_state),
                        report: Self::maintenance_report_record(&outcome.report),
                    },
                ),
            )
        })
    }

    /// The build capabilities advertised by the bound engine. This is a static build report and takes no
    /// session.
    pub fn store_capabilities(&self) -> Vec<String> {
        loom_store::provided_capabilities()
            .iter()
            .map(|cap| (*cap).to_string())
            .collect()
    }

    /// The runtime profile (channel, policy, crypto and TLS providers, FIPS posture) of the bound
    /// engine. This is a static runtime report and takes no session.
    pub fn store_runtime_profile(&self) -> RuntimeProfile {
        loom_core::runtime_profile()
    }

    /// The bound store's digest algorithm as a stable lowercase name (`"blake3"`/`"sha256"`). Read from
    /// the superblock via a lock-free reader open (no session, no writer exclusion), so a caller can
    /// reproduce `Digest::hash(algo, bytes)` against this store's actual profile.
    ///
    /// # Errors
    /// Returns [`LoomError`] if the store cannot be opened for reading.
    pub fn store_digest_algo(&self) -> Result<String, LoomError> {
        let store = open_store_metadata_checked(&self.path)?;
        Ok(store.digest_algo().as_str().to_string())
    }

    /// Open a path-bound SQL session over database `db` in `workspace`: the
    /// client is already bound to one store, so no parent `LoomSession` is taken. The SQL workspace id is
    /// derived deterministically from the name (matching the CLI and FFI), its SQL facet is ensured, and a
    /// cheap reopenable handle is registered. No loom or store is held between calls.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a facet/authorization or store failure.
    pub fn sql_open(&self, workspace: &str, db: &str) -> Result<SqlSession, LoomError> {
        self.sql_open_inner(workspace, db, LocalOpenAuth::default())
    }

    fn sql_open_inner(
        &self,
        workspace: &str,
        db: &str,
        auth: LocalOpenAuth,
    ) -> Result<SqlSession, LoomError> {
        let ns = derive_sql_ns_id(workspace);
        // Fail-fast and create the workspace + SQL facet eagerly, then release the write lock.
        let mut loom = self.open_sql_write_loom(workspace, ns, &auth)?;
        save_loom(&mut loom)?;
        drop(loom);
        let id = {
            let mut next = self.next_id.lock().expect("session id lock");
            *next += 1;
            *next
        };
        self.sql_sessions.lock().expect("sql session lock").insert(
            id,
            SqlSessionState {
                ns_name: workspace.to_string(),
                ns,
                db: db.to_string(),
                auth,
            },
        );
        Ok(SqlSession(HandleId {
            kind: "sql_session".to_string(),
            id: id.to_be_bytes().to_vec(),
            generation: 1,
            owner_session: Vec::new(),
        }))
    }

    /// Open a SQL session over an **encrypted** store unlocked with `passphrase`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a non-utf-8 passphrase, a wrong key, or a facet failure.
    pub fn sql_open_keyed(
        &self,
        workspace: &str,
        db: &str,
        passphrase: &[u8],
    ) -> Result<SqlSession, LoomError> {
        self.sql_open_inner(workspace, db, sql_auth_keyed(passphrase)?)
    }

    /// Open a SQL session over an **encrypted** store unlocked with a raw 256-bit KEK.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a wrong key or a facet failure.
    pub fn sql_open_with_kek(
        &self,
        workspace: &str,
        db: &str,
        kek: [u8; KEY_LEN],
    ) -> Result<SqlSession, LoomError> {
        self.sql_open_inner(workspace, db, sql_auth_kek(kek))
    }

    /// Open a SQL session that binds `principal` (authenticated with `auth_passphrase`) for ACL checks.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a non-utf-8 passphrase, an auth failure, or a facet failure.
    pub fn sql_open_authenticated(
        &self,
        workspace: &str,
        db: &str,
        auth_principal: PrincipalId,
        auth_passphrase: &[u8],
    ) -> Result<SqlSession, LoomError> {
        let auth =
            sql_auth_with_principal(LocalOpenAuth::default(), auth_principal, auth_passphrase)?;
        self.sql_open_inner(workspace, db, auth)
    }

    /// Open a SQL session over an encrypted store (`passphrase`) that also binds `principal`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a non-utf-8 passphrase, a wrong key, an auth failure, or a facet failure.
    pub fn sql_open_keyed_authenticated(
        &self,
        workspace: &str,
        db: &str,
        passphrase: &[u8],
        auth_principal: PrincipalId,
        auth_passphrase: &[u8],
    ) -> Result<SqlSession, LoomError> {
        let auth =
            sql_auth_with_principal(sql_auth_keyed(passphrase)?, auth_principal, auth_passphrase)?;
        self.sql_open_inner(workspace, db, auth)
    }

    /// Open a SQL session over an encrypted store (raw KEK) that also binds `principal`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a non-utf-8 passphrase, a wrong key, an auth failure, or a facet failure.
    pub fn sql_open_with_kek_authenticated(
        &self,
        workspace: &str,
        db: &str,
        kek: [u8; KEY_LEN],
        auth_principal: PrincipalId,
        auth_passphrase: &[u8],
    ) -> Result<SqlSession, LoomError> {
        let auth = sql_auth_with_principal(sql_auth_kek(kek), auth_principal, auth_passphrase)?;
        self.sql_open_inner(workspace, db, auth)
    }

    /// Bind and authenticate `principal` (with `passphrase`) on an existing SQL session, so subsequent
    /// operations run under that principal. Verified immediately against the store's identity.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, a non-utf-8 passphrase, or an auth failure.
    pub fn sql_authenticate_passphrase(
        &self,
        session: &SqlSession,
        principal: PrincipalId,
        passphrase: &[u8],
    ) -> Result<(), LoomError> {
        let key = sql_handle_key(session)?;
        let candidate = {
            let map = self.sql_sessions.lock().expect("sql session lock");
            let state = map
                .get(&key)
                .ok_or_else(|| LoomError::new(Code::NotFound, "unknown sql session handle"))?;
            sql_auth_with_principal(state.auth.clone(), principal, passphrase)?
        };
        // Verify the credential now by attaching it to a read snapshot (fail fast on a bad passphrase).
        self.open_sql_read_loom(&candidate)?;
        let mut map = self.sql_sessions.lock().expect("sql session lock");
        let state = map
            .get_mut(&key)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown sql session handle"))?;
        state.auth = candidate;
        Ok(())
    }

    /// Run one or more `;`-separated statements against the SQL session, returning the canonical-CBOR
    /// result payloads. Each call is one atomic operation: it opens a lock-free read snapshot, executes,
    /// and - only when the statement actually mutated state - takes the write lock to persist and save.
    /// A `BEGIN` without a matching `COMMIT`/`ROLLBACK` in one call is rejected; run cross-statement
    /// transactions through a `SqlBatch`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown SQL session, an unresolved transaction, or a SQL failure.
    pub fn sql_exec(&self, session: &SqlSession, sql: &str) -> Result<Vec<u8>, LoomError> {
        let state = self.sql_state(session)?;
        let read = self.open_sql_read_loom(&state.auth)?;
        let mut store = LoomSqlStore::open_write(read, state.ns, &state.db)?;
        let payload = store.exec_cbor(sql)?;
        if store.in_transaction() {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "BEGIN without a matching COMMIT/ROLLBACK in one exec: open a SqlBatch to run a transaction across statements",
            ));
        }
        if store.is_dirty() {
            let mut write = self.open_sql_write_loom(&state.ns_name, state.ns, &state.auth)?;
            store.persist(&mut write, state.ns, &state.db)?;
            save_loom(&mut write)?;
        }
        Ok(payload)
    }

    /// Run a query and return the rows of its first `SELECT`, each row as a canonical-CBOR cell array,
    /// over a lock-free read snapshot (no writer is blocked).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown SQL session or a SQL execution failure.
    pub fn sql_query(&self, session: &SqlSession, sql: &str) -> Result<Vec<Vec<u8>>, LoomError> {
        let state = self.sql_state(session)?;
        let read = self.open_sql_read_loom(&state.auth)?;
        let mut store = LoomSqlStore::open_read(read, state.ns, &state.db)?;
        store.select_rows_cbor(sql)
    }

    /// Run a read-only query against database `db` in `workspace` and return the full canonical
    /// `exec_cbor` result payload without persisting state.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace/db, a malformed query, an in-flight
    /// transaction (`INVALID_ARGUMENT`), or a mutating statement (`PERMISSION_DENIED`).
    pub fn sql_query_result(
        &self,
        session: &LoomSession,
        workspace: &str,
        db: &str,
        sql: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let mut store = LoomSqlStore::load_eager_read(loom, ns, db)?;
            let bytes = store.exec_cbor(sql)?;
            if store.in_transaction() {
                return Err(LoomError::new(
                    Code::InvalidArgument,
                    "BEGIN without a matching COMMIT/ROLLBACK in one query: use sql.exec for statements that mutate state",
                ));
            }
            if store.is_dirty() {
                return Err(LoomError::new(
                    Code::PermissionDenied,
                    "sql.query is read-only; use sql.exec for statements that mutate state",
                ));
            }
            Ok(bytes)
        })
    }

    pub fn sql_exec_result(
        &self,
        session: &LoomSession,
        workspace: &str,
        db: &str,
        sql: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            execute_generated_sql_result(&self.path, loom, workspace, db, sql)
        })
    }

    /// Record a VCS commit over the SQL workspace's persisted working tree,
    /// returning the commit's content address. Prior `sql_exec` calls have already staged their changes,
    /// so this snapshots them onto the workspace's head branch.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown SQL session or a commit/save failure.
    pub fn sql_commit(
        &self,
        session: &SqlSession,
        message: &str,
        author: &str,
        timestamp_ms: u64,
    ) -> Result<Digest, LoomError> {
        let state = self.sql_state(session)?;
        let mut loom = self.open_sql_write_loom(&state.ns_name, state.ns, &state.auth)?;
        let digest = loom.commit(state.ns, author, message, timestamp_ms)?;
        save_loom(&mut loom)?;
        Ok(digest)
    }

    /// Close a SQL session, releasing its handle state. Returns whether a session was open.
    pub fn sql_close(&self, session: &SqlSession) -> bool {
        match sql_handle_key(session) {
            Ok(key) => self
                .sql_sessions
                .lock()
                .expect("sql session lock")
                .remove(&key)
                .is_some(),
            Err(_) => false,
        }
    }

    /// Snapshot the path-bound state for a SQL session handle (workspace name/id, database, unlock key),
    /// dropping the registry lock before any I/O.
    fn sql_state(&self, session: &SqlSession) -> Result<SqlSessionState, LoomError> {
        let key = sql_handle_key(session)?;
        let map = self.sql_sessions.lock().expect("sql session lock");
        map.get(&key)
            .cloned()
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown sql session handle"))
    }

    /// Open the bound store for write, apply `auth` (unlock key + principal auth), and ensure the SQL
    /// facet of `ns` (named `ns_name`) exists.
    fn open_sql_write_loom(
        &self,
        ns_name: &str,
        ns: WorkspaceId,
        auth: &LocalOpenAuth,
    ) -> Result<Loom<FileStore>, LoomError> {
        let loom = match auth.unlock_key.as_ref() {
            Some(_) => open_loom_unlocked(&self.path, auth.unlock_key.as_ref())?,
            None => open_loom(&self.path)?,
        };
        let mut loom = attach_local_auth(loom, auth)?;
        loom.registry_mut().ensure_for_write(
            &WsSelector::Typed {
                ty: FacetKind::Sql,
                name: ns_name.to_string(),
            },
            ns,
        )?;
        Ok(loom)
    }

    /// Open a lock-free read snapshot of the bound store and apply `auth`.
    fn open_sql_read_loom(&self, auth: &LocalOpenAuth) -> Result<Loom<FileStore>, LoomError> {
        let loom = match auth.unlock_key.as_ref() {
            Some(_) => open_loom_read_unlocked(&self.path, auth.unlock_key.as_ref())?,
            None => open_loom_read(&self.path)?,
        };
        attach_local_auth(loom, auth)
    }

    // ---- SqlBatch: cross-statement transactions --------------------------------

    /// Begin a SQL transaction batch over `(workspace, db)`: hold the exclusive write lock for the batch's
    /// lifetime and load a mutation-capable store over a read snapshot. Statements accumulate until
    /// `sql_batch_commit`/`sql_batch_commit_vcs`; closing without a commit discards them.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a facet/authorization or store failure.
    pub fn sql_batch_begin(&self, workspace: &str, db: &str) -> Result<SqlBatch, LoomError> {
        self.sql_batch_begin_inner(workspace, db, LocalOpenAuth::default())
    }

    fn sql_batch_begin_inner(
        &self,
        workspace: &str,
        db: &str,
        auth: LocalOpenAuth,
    ) -> Result<SqlBatch, LoomError> {
        let ns = derive_sql_ns_id(workspace);
        let loom = self.open_sql_write_loom(workspace, ns, &auth)?;
        let read = self.open_sql_read_loom(&auth)?;
        let store = LoomSqlStore::open_write(read, ns, db)?;
        let id = {
            let mut next = self.next_id.lock().expect("session id lock");
            *next += 1;
            *next
        };
        self.sql_batches.lock().expect("sql batch lock").insert(
            id,
            SqlBatchState {
                loom,
                store,
                ns,
                db: db.to_string(),
                auth,
            },
        );
        Ok(SqlBatch(HandleId {
            kind: "sql_batch".to_string(),
            id: id.to_be_bytes().to_vec(),
            generation: 1,
            owner_session: Vec::new(),
        }))
    }

    /// Begin a batch over an **encrypted** store unlocked with `passphrase`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a non-utf-8 passphrase, a wrong key, or a facet/store failure.
    pub fn sql_batch_begin_keyed(
        &self,
        workspace: &str,
        db: &str,
        passphrase: &[u8],
    ) -> Result<SqlBatch, LoomError> {
        self.sql_batch_begin_inner(workspace, db, sql_auth_keyed(passphrase)?)
    }

    /// Begin a batch over an **encrypted** store unlocked with a raw 256-bit KEK.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a wrong key or a facet/store failure.
    pub fn sql_batch_begin_with_kek(
        &self,
        workspace: &str,
        db: &str,
        kek: [u8; KEY_LEN],
    ) -> Result<SqlBatch, LoomError> {
        self.sql_batch_begin_inner(workspace, db, sql_auth_kek(kek))
    }

    /// Begin a batch that binds `principal` (authenticated with `auth_passphrase`) for ACL checks.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a non-utf-8 passphrase, an auth failure, or a facet/store failure.
    pub fn sql_batch_begin_authenticated(
        &self,
        workspace: &str,
        db: &str,
        auth_principal: PrincipalId,
        auth_passphrase: &[u8],
    ) -> Result<SqlBatch, LoomError> {
        let auth =
            sql_auth_with_principal(LocalOpenAuth::default(), auth_principal, auth_passphrase)?;
        self.sql_batch_begin_inner(workspace, db, auth)
    }

    /// Begin a batch over an encrypted store (`passphrase`) that also binds `principal`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a non-utf-8 passphrase, a wrong key, an auth failure, or a store failure.
    pub fn sql_batch_begin_keyed_authenticated(
        &self,
        workspace: &str,
        db: &str,
        passphrase: &[u8],
        auth_principal: PrincipalId,
        auth_passphrase: &[u8],
    ) -> Result<SqlBatch, LoomError> {
        let auth =
            sql_auth_with_principal(sql_auth_keyed(passphrase)?, auth_principal, auth_passphrase)?;
        self.sql_batch_begin_inner(workspace, db, auth)
    }

    /// Begin a batch over an encrypted store (raw KEK) that also binds `principal`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a non-utf-8 passphrase, a wrong key, an auth failure, or a store failure.
    pub fn sql_batch_begin_with_kek_authenticated(
        &self,
        workspace: &str,
        db: &str,
        kek: [u8; KEY_LEN],
        auth_principal: PrincipalId,
        auth_passphrase: &[u8],
    ) -> Result<SqlBatch, LoomError> {
        let auth = sql_auth_with_principal(sql_auth_kek(kek), auth_principal, auth_passphrase)?;
        self.sql_batch_begin_inner(workspace, db, auth)
    }

    /// Execute one or more `;`-separated statements inside the batch, returning the canonical-CBOR result
    /// payload. Changes accumulate in the batch's store and are not durable until a batch commit.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown batch or a SQL failure.
    pub fn sql_batch_exec(&self, batch: &SqlBatch, sql: &str) -> Result<Vec<u8>, LoomError> {
        let key = batch_handle_key(batch)?;
        let mut map = self.sql_batches.lock().expect("sql batch lock");
        let state = map
            .get_mut(&key)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown sql batch handle"))?;
        state.store.exec_cbor(sql)
    }

    /// Persist the batch's accumulated changes and save (the atomic persistence boundary). Rejected while
    /// a SQL transaction is still open; the batch stays open for further statements.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown batch, an open transaction, or a persist/save failure.
    pub fn sql_batch_commit(&self, batch: &SqlBatch) -> Result<(), LoomError> {
        let key = batch_handle_key(batch)?;
        let mut map = self.sql_batches.lock().expect("sql batch lock");
        let state = map
            .get_mut(&key)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown sql batch handle"))?;
        ensure_batch_no_open_txn(state)?;
        state.store.persist(&mut state.loom, state.ns, &state.db)?;
        save_loom(&mut state.loom)
    }

    /// Persist the batch and record a VCS commit over the workspace's head, returning the commit digest.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown batch, an open transaction, or a commit/save failure.
    pub fn sql_batch_commit_vcs(
        &self,
        batch: &SqlBatch,
        message: &str,
        author: &str,
    ) -> Result<Digest, LoomError> {
        let key = batch_handle_key(batch)?;
        let mut map = self.sql_batches.lock().expect("sql batch lock");
        let state = map
            .get_mut(&key)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown sql batch handle"))?;
        ensure_batch_no_open_txn(state)?;
        state.store.persist(&mut state.loom, state.ns, &state.db)?;
        let digest = state.loom.commit(state.ns, author, message, now_ms())?;
        save_loom(&mut state.loom)?;
        Ok(digest)
    }

    /// Discard the batch's un-persisted changes (and any open SQL transaction) by re-snapshotting the
    /// durable state; the batch stays open.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown batch or a store failure.
    pub fn sql_batch_abort(&self, batch: &SqlBatch) -> Result<(), LoomError> {
        let key = batch_handle_key(batch)?;
        let mut map = self.sql_batches.lock().expect("sql batch lock");
        let state = map
            .get_mut(&key)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown sql batch handle"))?;
        let read = self.open_sql_read_loom(&state.auth)?;
        state.store = LoomSqlStore::open_write(read, state.ns, &state.db)?;
        Ok(())
    }

    /// Close a batch, releasing the write lock; un-persisted changes are discarded. Returns whether a
    /// batch was open.
    pub fn sql_batch_close(&self, batch: &SqlBatch) -> bool {
        match batch_handle_key(batch) {
            Ok(key) => self
                .sql_batches
                .lock()
                .expect("sql batch lock")
                .remove(&key)
                .is_some(),
            Err(_) => false,
        }
    }

    // ---- RowIter (forward-only query iterator; source realization of `sql_query`) ----------------

    /// Open a forward-only row iterator over the first `SELECT` of `sql` on `session`. Rows are buffered
    /// as canonical-CBOR cell arrays and drained by [`LocalLoomClient::iter_next`]; free with
    /// [`LocalLoomClient::iter_free`].
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown SQL session or a SQL failure.
    pub fn sql_query_open(&self, session: &SqlSession, sql: &str) -> Result<RowIter, LoomError> {
        let rows = self.sql_query(session, sql)?;
        let id = {
            let mut next = self.next_id.lock().expect("session id lock");
            *next += 1;
            *next
        };
        self.row_iters
            .lock()
            .expect("row iter lock")
            .insert(id, VecDeque::from(rows));
        Ok(RowIter(HandleId {
            kind: "row_iter".to_string(),
            id: id.to_be_bytes().to_vec(),
            generation: 1,
            owner_session: Vec::new(),
        }))
    }

    /// Advance a row iterator, returning the next row's canonical-CBOR bytes, or `None` at end of stream.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown iterator handle.
    pub fn iter_next(&self, iter: &RowIter) -> Result<Option<Vec<u8>>, LoomError> {
        let key = iter_handle_key(iter)?;
        let mut map = self.row_iters.lock().expect("row iter lock");
        let queue = map
            .get_mut(&key)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown row iterator handle"))?;
        Ok(queue.pop_front())
    }

    /// Free a row iterator handle. Returns whether one was open.
    pub fn iter_free(&self, iter: &RowIter) -> bool {
        match iter_handle_key(iter) {
            Ok(key) => self
                .row_iters
                .lock()
                .expect("row iter lock")
                .remove(&key)
                .is_some(),
            Err(_) => false,
        }
    }

    // ---- Direct / version-aware SQL readers -----------------

    /// Read the staged table `table` in `workspace` as canonical CBOR (`{ columns, rows }`).
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session, workspace, or table.
    pub fn sql_read_table(
        &self,
        session: &LoomSession,
        workspace: &str,
        table: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let t = loom.read_table(ns, table)?;
            loom_sql::result_cbor::table_cbor(&t)
        })
    }

    /// Read `table` in `workspace` as of historical `commit`, without changing the working tree.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace/table or a malformed commit.
    pub fn sql_read_table_at(
        &self,
        session: &LoomSession,
        workspace: &str,
        table: &str,
        commit: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let commit = Digest::parse(commit)?;
            let t = loom.read_table_at(ns, table, commit)?;
            loom_sql::result_cbor::table_cbor(&t)
        })
    }

    /// Scan secondary `index` on `table` for the canonical-CBOR cell-array `prefix`, returning the
    /// matching rows as canonical CBOR (`{ columns, rows }`). An empty prefix is the CBOR of an empty
    /// array.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace/table/index or a malformed prefix.
    pub fn sql_index_scan(
        &self,
        session: &LoomSession,
        workspace: &str,
        table: &str,
        index: &str,
        prefix: &[u8],
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let values = loom_sql::lookup_cbor::values_from_cbor(prefix)?;
            let rows = loom.index_scan(ns, table, index, &values)?;
            let schema = loom.read_table(ns, table)?.schema().clone();
            loom_sql::result_cbor::rows_cbor(&schema, &rows)
        })
    }

    /// Scan secondary `index` on `table` as of historical `commit`.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace/table/index, a malformed prefix, or a
    /// malformed commit.
    pub fn sql_index_scan_at(
        &self,
        session: &LoomSession,
        workspace: &str,
        table: &str,
        index: &str,
        prefix: &[u8],
        commit: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let commit = Digest::parse(commit)?;
            let values = loom_sql::lookup_cbor::values_from_cbor(prefix)?;
            let rows = loom.index_scan_at(ns, table, index, &values, commit)?;
            let schema = loom.read_table_at(ns, table, commit)?.schema().clone();
            loom_sql::result_cbor::rows_cbor(&schema, &rows)
        })
    }

    /// Blame each current row of `table` on `branch` with the commit that last set it, as canonical CBOR.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace/branch/table.
    pub fn sql_blame(
        &self,
        session: &LoomSession,
        workspace: &str,
        branch: &str,
        table: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let rows = loom.blame_table(ns, branch, table)?;
            loom_sql::result_cbor::blame_cbor(&rows)
        })
    }

    /// The row-level diff of `table` between two commits, as canonical CBOR.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace/table or a malformed commit.
    pub fn sql_diff(
        &self,
        session: &LoomSession,
        workspace: &str,
        table: &str,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let from = Digest::parse(from_commit)?;
            let to = Digest::parse(to_commit)?;
            let diffs = loom.diff_table(ns, table, from, to)?;
            loom_sql::result_cbor::diff_cbor(&diffs)
        })
    }

    /// The schema-aware table diff of `table` between two commits, as canonical CBOR.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session/workspace/table or a malformed commit.
    pub fn sql_table_diff(
        &self,
        session: &LoomSession,
        workspace: &str,
        table: &str,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let from = Digest::parse(from_commit)?;
            let to = Digest::parse(to_commit)?;
            let records = loom.diff_table_records(ns, table, from, to)?;
            loom_sql::result_cbor::table_diff_cbor(&records)
        })
    }

    /// The database (collection) names present in `workspace`'s SQL facet, as a canonical-CBOR array of
    /// strings in name order.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown session or workspace.
    pub fn sql_list_databases(
        &self,
        session: &LoomSession,
        workspace: &str,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_session(session, |loom| {
            let ns = resolve_ns_read(loom, workspace)?;
            let root = loom_core::workspace::facet_root(FacetKind::Sql);
            let names: Vec<loom_codec::Value> = match loom.list_directory(ns, &root) {
                Ok(entries) => entries
                    .into_iter()
                    .filter(|e| e.kind == FileKind::Directory)
                    .map(|e| loom_codec::Value::Text(e.name))
                    .collect(),
                // No SQL facet staged yet is an empty database list, not an error.
                Err(err) if err.code == Code::NotFound => Vec::new(),
                Err(err) => return Err(err),
            };
            loom_codec::encode(&loom_codec::Value::Array(names))
                .map_err(|e| LoomError::new(Code::Internal, format!("list databases cbor: {e}")))
        })
    }

    // ---- Tasks: portable cooperative async ---------------------------------------

    fn spawn_task(&self, work: TaskWork) -> Task {
        let id = {
            let mut next = self.next_id.lock().expect("session id lock");
            *next += 1;
            *next
        };
        self.tasks
            .lock()
            .expect("task lock")
            .insert(id, TaskState::Pending(work));
        Task(HandleId {
            kind: "task".to_string(),
            id: id.to_be_bytes().to_vec(),
            generation: 1,
            owner_session: Vec::new(),
        })
    }

    fn run_task_work(&self, work: &TaskWork) -> Result<Vec<u8>, LoomError> {
        match work {
            TaskWork::SqlExec { session, sql } => self.sql_exec(session, sql),
            TaskWork::ReadTable {
                session,
                workspace,
                table,
            } => self.sql_read_table(session, workspace, table),
            TaskWork::IndexScan {
                session,
                workspace,
                table,
                index,
                prefix,
            } => self.sql_index_scan(session, workspace, table, index, prefix),
            TaskWork::Blame {
                session,
                workspace,
                branch,
                table,
            } => self.sql_blame(session, workspace, branch, table),
            TaskWork::Diff {
                session,
                workspace,
                table,
                from,
                to,
            } => self.sql_diff(session, workspace, table, from, to),
            TaskWork::LogAsync {
                session,
                workspace,
                branch,
            } => loom_wire::digest_list_to_cbor(self.log(session, workspace, branch)?),
            TaskWork::MergeAsync {
                session,
                workspace,
                from_branch,
                author,
                cell_level,
            } => loom_wire::vcs::merge_result_to_cbor(&self.vcs_merge(
                session,
                workspace,
                from_branch,
                author,
                *cell_level,
                now_ms(),
            )?),
            TaskWork::ImportFsAsync {
                session,
                workspace,
                src_path,
                author,
                message,
                commit,
                dry_run,
            } => self.import_fs(
                session,
                workspace,
                src_path,
                author.as_deref(),
                message.as_deref(),
                *commit,
                *dry_run,
            ),
            TaskWork::ExportFsAsync {
                session,
                workspace,
                dst_path,
                revision,
                dry_run,
            } => self.export_fs(session, workspace, dst_path, revision.as_deref(), *dry_run),
            TaskWork::ArchiveImportAsync {
                session,
                workspace,
                src_path,
                kind,
                gzip_output_path,
                commit,
                author,
                message,
                dry_run,
            } => self.archive_import(
                session,
                workspace,
                src_path,
                kind,
                gzip_output_path.as_deref(),
                *commit,
                author.as_deref(),
                message.as_deref(),
                *dry_run,
            ),
            TaskWork::ArchiveExportAsync {
                session,
                workspace,
                dst_path,
                kind,
                revision,
                dry_run,
            } => self.archive_export(
                session,
                workspace,
                dst_path,
                kind,
                revision.as_deref(),
                *dry_run,
            ),
            TaskWork::CarImportAsync {
                session,
                src_path,
                dry_run,
            } => self.car_import(session, src_path, *dry_run),
            TaskWork::CarExportAsync {
                session,
                workspace,
                dst_path,
                dry_run,
            } => self.car_export(session, workspace, dst_path, *dry_run),
        }
    }

    /// Start a SQL exec as a pending [`Task`]; the statement runs on the first `task_poll`.
    ///
    /// # Errors
    /// Returns [`LoomError`] only if the handle cannot be minted (never, in practice).
    pub fn sql_exec_async(&self, session: &SqlSession, sql: &str) -> Result<Task, LoomError> {
        Ok(self.spawn_task(TaskWork::SqlExec {
            session: session.clone(),
            sql: sql.to_string(),
        }))
    }

    /// Async form of [`LocalLoomClient::sql_read_table`].
    ///
    /// # Errors
    /// Returns [`LoomError`] only if the handle cannot be minted.
    pub fn sql_read_table_async(
        &self,
        handle: &LoomSession,
        workspace: &str,
        table: &str,
    ) -> Result<Task, LoomError> {
        Ok(self.spawn_task(TaskWork::ReadTable {
            session: handle.clone(),
            workspace: workspace.to_string(),
            table: table.to_string(),
        }))
    }

    /// Async form of [`LocalLoomClient::sql_index_scan`].
    ///
    /// # Errors
    /// Returns [`LoomError`] only if the handle cannot be minted.
    pub fn sql_index_scan_async(
        &self,
        handle: &LoomSession,
        workspace: &str,
        table: &str,
        index: &str,
        prefix: &[u8],
    ) -> Result<Task, LoomError> {
        Ok(self.spawn_task(TaskWork::IndexScan {
            session: handle.clone(),
            workspace: workspace.to_string(),
            table: table.to_string(),
            index: index.to_string(),
            prefix: prefix.to_vec(),
        }))
    }

    /// Async form of [`LocalLoomClient::sql_blame`].
    ///
    /// # Errors
    /// Returns [`LoomError`] only if the handle cannot be minted.
    pub fn sql_blame_async(
        &self,
        handle: &LoomSession,
        workspace: &str,
        branch: &str,
        table: &str,
    ) -> Result<Task, LoomError> {
        Ok(self.spawn_task(TaskWork::Blame {
            session: handle.clone(),
            workspace: workspace.to_string(),
            branch: branch.to_string(),
            table: table.to_string(),
        }))
    }

    /// Async form of [`LocalLoomClient::sql_diff`].
    ///
    /// # Errors
    /// Returns [`LoomError`] only if the handle cannot be minted.
    pub fn sql_diff_async(
        &self,
        handle: &LoomSession,
        workspace: &str,
        table: &str,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Task, LoomError> {
        Ok(self.spawn_task(TaskWork::Diff {
            session: handle.clone(),
            workspace: workspace.to_string(),
            table: table.to_string(),
            from: from_commit.to_string(),
            to: to_commit.to_string(),
        }))
    }

    /// Drive a task toward completion; the first poll runs its work. Returns whether it is terminal.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown task handle.
    pub fn task_poll(&self, task: &Task) -> Result<bool, LoomError> {
        let key = task_handle_key(task)?;
        let pending = {
            let map = self.tasks.lock().expect("task lock");
            match map.get(&key) {
                Some(TaskState::Pending(work)) => Some(work.clone()),
                Some(_) => None,
                None => return Err(LoomError::new(Code::NotFound, "unknown task handle")),
            }
        };
        if let Some(work) = pending {
            let outcome = self.run_task_work(&work);
            let mut map = self.tasks.lock().expect("task lock");
            if let Some(slot @ TaskState::Pending(_)) = map.get_mut(&key) {
                *slot = match outcome {
                    Ok(bytes) => TaskState::Ready(bytes),
                    Err(err) => TaskState::Errored(err),
                };
            }
        }
        let map = self.tasks.lock().expect("task lock");
        Ok(!matches!(map.get(&key), Some(TaskState::Pending(_))))
    }

    /// The task's status as canonical-CBOR text (`pending`/`ready`/`error`/`cancelled`/`taken`), or
    /// `None` for an unknown handle.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a malformed task handle id.
    pub fn task_status(&self, task: &Task) -> Result<Option<Vec<u8>>, LoomError> {
        let key = task_handle_key(task)?;
        let map = self.tasks.lock().expect("task lock");
        Ok(map.get(&key).map(task_status_cbor))
    }

    /// Take a completed task's result buffer, leaving it `Taken`; a stored error is re-raised.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown handle, a still-pending/cancelled/already-taken task, or the
    /// task's own error.
    pub fn task_result(&self, task: &Task) -> Result<Vec<u8>, LoomError> {
        let key = task_handle_key(task)?;
        let mut map = self.tasks.lock().expect("task lock");
        let slot = map
            .get_mut(&key)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown task handle"))?;
        match slot {
            TaskState::Ready(_) => {
                let TaskState::Ready(bytes) = std::mem::replace(slot, TaskState::Taken) else {
                    unreachable!("just matched Ready")
                };
                Ok(bytes)
            }
            TaskState::Errored(err) => Err(err.clone()),
            TaskState::Pending(_) => Err(LoomError::new(
                Code::InvalidArgument,
                "task is still pending",
            )),
            TaskState::Cancelled => {
                Err(LoomError::new(Code::InvalidArgument, "task was cancelled"))
            }
            TaskState::Taken => Err(LoomError::new(
                Code::InvalidArgument,
                "task result already taken",
            )),
        }
    }

    /// Cancel a still-pending task; a terminal task is unaffected.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown task handle.
    pub fn task_cancel(&self, task: &Task) -> Result<(), LoomError> {
        let key = task_handle_key(task)?;
        let mut map = self.tasks.lock().expect("task lock");
        let slot = map
            .get_mut(&key)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown task handle"))?;
        if matches!(slot, TaskState::Pending(_)) {
            *slot = TaskState::Cancelled;
        }
        Ok(())
    }

    /// Free a task handle. Returns whether one was open.
    pub fn task_free(&self, task: &Task) -> bool {
        match task_handle_key(task) {
            Ok(key) => self.tasks.lock().expect("task lock").remove(&key).is_some(),
            Err(_) => false,
        }
    }

    /// Poll a task to completion and take its result in one call.
    ///
    /// # Errors
    /// Returns [`LoomError`] for an unknown handle or the task's own error.
    pub fn task_wait(&self, task: &Task) -> Result<Vec<u8>, LoomError> {
        self.task_poll(task)?;
        self.task_result(task)
    }

    // ---- Diagnostics: local decoders -------------------------------------------

    /// Render a canonical-CBOR result buffer as JSON. Local decode, no session.
    ///
    /// # Errors
    /// Returns [`LoomError`] (`CORRUPT_OBJECT`) when the buffer is not a canonical result; the error is
    /// also recorded for [`LocalLoomClient::last_error`].
    pub fn result_to_json(&self, result: &[u8]) -> Result<String, LoomError> {
        self.record(loom_result::result_to_json(result))
    }

    /// Render a canonical-CBOR result buffer as the React Native bridge JSON projection. Local decode, no
    /// session.
    ///
    /// # Errors
    /// Returns [`LoomError`] when the buffer is not a canonical result; also recorded for `last_error`.
    pub fn result_to_bridge_json(&self, result: &[u8]) -> Result<String, LoomError> {
        self.record(loom_result::to_bridge_json(result))
    }

    /// The most recent error produced by a diagnostics or result-view decode on this client, mirroring
    /// the binding's thread-local last-error state.
    pub fn last_error(&self) -> Option<LoomError> {
        self.last_error.lock().expect("last error lock").clone()
    }

    /// Record a fallible result's error into the client's last-error slot, returning the result.
    pub(crate) fn record<T>(&self, result: Result<T, LoomError>) -> Result<T, LoomError> {
        if let Err(err) = &result {
            *self.last_error.lock().expect("last error lock") = Some(err.clone());
        }
        result
    }

    /// The last recorded error as canonical CBOR `[code_i32, message_or_null, details_or_null]`;
    /// `[0, null, null]` when none.
    pub(crate) fn last_error_cbor(&self) -> Vec<u8> {
        let guard = self.last_error.lock().expect("last error lock");
        let (code, message, details) = match guard.as_ref() {
            Some(err) => (
                err.code.as_i32(),
                loom_codec::Value::Text(err.message.clone()),
                err.details_cbor()
                    .map(loom_codec::Value::Bytes)
                    .unwrap_or(loom_codec::Value::Null),
            ),
            None => (0, loom_codec::Value::Null, loom_codec::Value::Null),
        };
        loom_codec::encode(&loom_codec::Value::Array(vec![
            loom_codec::Value::int(i64::from(code)),
            message,
            details,
        ]))
        .expect("last error always encodes to canonical CBOR")
    }

    /// Register a decoded result payload behind a minted handle id, for the handle-based result views.
    pub(crate) fn register_result_view(
        &self,
        payload: loom_result::result_view::ResultPayload,
    ) -> u64 {
        let id = {
            let mut next = self.next_id.lock().expect("session id lock");
            *next += 1;
            *next
        };
        self.result_views
            .lock()
            .expect("result view lock")
            .insert(id, payload);
        id
    }

    /// Run `f` against the decoded payload behind result-view handle `id`, or fail `NOT_FOUND`.
    pub(crate) fn with_result_view<R>(
        &self,
        id: u64,
        f: impl FnOnce(&loom_result::result_view::ResultPayload) -> Result<R, LoomError>,
    ) -> Result<R, LoomError> {
        let views = self.result_views.lock().expect("result view lock");
        let payload = views
            .get(&id)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown or closed result view"))?;
        f(payload)
    }

    /// Drop a registered result view by handle id.
    pub(crate) fn drop_result_view(&self, id: u64) {
        self.result_views
            .lock()
            .expect("result view lock")
            .remove(&id);
    }

    // ---- ResultViews: local decoders -------------------------------------------

    /// Decode a canonical-CBOR result buffer into an indexed, typed view (`result_open`). Local decode,
    /// no session.
    ///
    /// # Errors
    /// Returns [`LoomError`] when the buffer is not a canonical result; also recorded for `last_error`.
    pub fn result_open(
        &self,
        result: &[u8],
    ) -> Result<crate::result_view::LocalResultView, LoomError> {
        self.record(crate::result_view::LocalResultView::open(result))
    }

    /// Decode a single reader result buffer into a view (`row_open`); the shared decoder handles both
    /// statement and reader payloads, so this mirrors [`LocalLoomClient::result_open`].
    ///
    /// # Errors
    /// Returns [`LoomError`] when the buffer is not a canonical result; also recorded for `last_error`.
    pub fn row_open(
        &self,
        result: &[u8],
    ) -> Result<crate::result_view::LocalResultView, LoomError> {
        self.record(crate::result_view::LocalResultView::open(result))
    }
}

fn handle_key(session: &LoomSession) -> Result<u64, LoomError> {
    let bytes: [u8; 8] = session
        .0
        .id
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::new(Code::InvalidArgument, "malformed session handle id"))?;
    Ok(u64::from_be_bytes(bytes))
}

fn sql_handle_key(session: &SqlSession) -> Result<u64, LoomError> {
    let bytes: [u8; 8] =
        session.0.id.as_slice().try_into().map_err(|_| {
            LoomError::new(Code::InvalidArgument, "malformed sql session handle id")
        })?;
    Ok(u64::from_be_bytes(bytes))
}

fn batch_handle_key(batch: &SqlBatch) -> Result<u64, LoomError> {
    let bytes: [u8; 8] = batch
        .0
        .id
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::new(Code::InvalidArgument, "malformed sql batch handle id"))?;
    Ok(u64::from_be_bytes(bytes))
}

fn iter_handle_key(iter: &RowIter) -> Result<u64, LoomError> {
    let bytes: [u8; 8] =
        iter.0.id.as_slice().try_into().map_err(|_| {
            LoomError::new(Code::InvalidArgument, "malformed row iterator handle id")
        })?;
    Ok(u64::from_be_bytes(bytes))
}

/// An [`loom_interchange::ImportReport`] as a fixed-order canonical-CBOR [`Value`](loom_codec::Value)
/// array. Shared by the filesystem, archive, and CAR import paths (and their async forms) so every result
/// that carries an import report uses the same shape.
/// Whether a byte-transfer `kind` has a store-backed import codec in v1.
fn transfer_kind_import_supported(
    kind: loom_interchange_io::transfer::TransferKind,
) -> Result<(), LoomError> {
    use loom_interchange_io::transfer::TransferKind;
    match kind {
        TransferKind::Tar
        | TransferKind::TarZstd
        | TransferKind::TarGzip
        | TransferKind::Zip
        | TransferKind::Gzip
        | TransferKind::Car => Ok(()),
        other => Err(LoomError::new(
            Code::Unsupported,
            format!(
                "transfer import kind '{}' is not yet implemented over the byte-transfer contract",
                other.as_str()
            ),
        )),
    }
}

/// Map a byte-transfer archive `kind` to the interchange [`loom_interchange::ArchiveKind`].
fn transfer_kind_to_archive(
    kind: loom_interchange_io::transfer::TransferKind,
) -> Result<loom_interchange::ArchiveKind, LoomError> {
    use loom_interchange::ArchiveKind;
    use loom_interchange_io::transfer::TransferKind;
    Ok(match kind {
        TransferKind::Tar => ArchiveKind::Tar,
        TransferKind::TarZstd => ArchiveKind::TarZstd,
        TransferKind::TarGzip => ArchiveKind::TarGzip,
        TransferKind::Zip => ArchiveKind::Zip,
        TransferKind::Gzip => ArchiveKind::Gzip,
        other => {
            return Err(LoomError::new(
                Code::Unsupported,
                format!("transfer kind '{}' has no archive codec", other.as_str()),
            ));
        }
    })
}

/// Apply a staged byte-transfer import to `workspace` under the write authority, returning the
/// canonical `loom.interchange.import-report.v1`. `car` derives its own workspace from the CAR
/// manifest; the archive family imports into the Files facet.
fn apply_transfer_import(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    kind: loom_interchange_io::transfer::TransferKind,
    bytes: &[u8],
    commit: bool,
    dry_run: bool,
) -> Result<Vec<u8>, LoomError> {
    use loom_interchange_io::transfer::TransferKind;
    let report = match kind {
        TransferKind::Car => {
            let mut options = loom_interchange_io::CarImportOptions::new(workspace);
            options.dry_run = dry_run;
            loom_interchange_io::import_car_bytes(loom, bytes, &options)?.report
        }
        _ => {
            let ns = loom.registry_mut().ensure_for_write(
                &ns_selector(workspace, FacetKind::Files),
                mint_workspace_id()?,
            )?;
            let archive_kind = transfer_kind_to_archive(kind)?;
            let mut options = loom_interchange_io::ArchiveImportOptions::new(workspace);
            options.commit = commit;
            options.dry_run = dry_run;
            loom_interchange_io::import_archive_bytes(
                loom,
                ns,
                bytes,
                std::path::Path::new("transfer"),
                archive_kind,
                &options,
            )?
            .report
        }
    };
    if !dry_run {
        save_loom(loom)?;
    }
    Ok(import_report_to_cbor(&report))
}

fn import_report_to_value(r: &loom_interchange::ImportReport) -> loom_codec::Value {
    r.to_value()
}

/// Canonical CBOR of an [`loom_interchange::ImportReport`]. The sync `import_fs` and the async
/// `import_fs_async` return this identical shape (result parity).
fn import_report_to_cbor(r: &loom_interchange::ImportReport) -> Vec<u8> {
    loom_codec::encode(&import_report_to_value(r)).expect("canonical CBOR encode of ImportReport")
}

/// An [`loom_interchange::ExportReport`] as a fixed-order canonical-CBOR [`Value`](loom_codec::Value)
/// array. Shared by the filesystem, archive, and CAR export paths (and their async forms).
fn export_report_to_value(r: &loom_interchange::ExportReport) -> loom_codec::Value {
    use loom_codec::Value;
    Value::Array(vec![
        Value::Text(r.profile.clone()),
        Value::Text(r.destination_scope.clone()),
        Value::Uint(r.files_written),
        Value::Uint(r.rows_written),
        Value::Uint(r.bytes_out),
        Value::Bool(r.dry_run),
        Value::Array(r.warnings.iter().cloned().map(Value::Text).collect()),
        Value::Array(
            r.fidelity_issues
                .iter()
                .map(fidelity_issue_to_value)
                .collect(),
        ),
    ])
}

/// Canonical CBOR of an [`loom_interchange::ExportReport`]. The sync `export_fs` and the async
/// `export_fs_async` return this identical shape (result parity).
fn export_report_to_cbor(r: &loom_interchange::ExportReport) -> Vec<u8> {
    loom_codec::encode(&export_report_to_value(r)).expect("canonical CBOR encode of ExportReport")
}

/// A stable lowercase name for an [`loom_interchange::ArchiveKind`] (the wire form of the manifest kind).
fn archive_kind_str(kind: loom_interchange::ArchiveKind) -> &'static str {
    use loom_interchange::ArchiveKind::*;
    match kind {
        Zip => "zip",
        Tar => "tar",
        Gzip => "gzip",
        TarZstd => "tar-zstd",
        TarGzip => "tar-gzip",
    }
}

/// Parse the IDL `kind` string into an [`loom_interchange::ArchiveKind`] (mirrors the CLI's accepted
/// aliases), returning an `InvalidArgument` error for an unknown kind.
fn archive_kind_from_str(kind: &str) -> Result<loom_interchange::ArchiveKind, LoomError> {
    use loom_interchange::ArchiveKind::*;
    Ok(match kind {
        "zip" => Zip,
        "tar" => Tar,
        "tar-zstd" | "tar.zstd" | "tzst" => TarZstd,
        "tar-gzip" | "tar.gz" | "tgz" => TarGzip,
        "gzip" | "gz" => Gzip,
        other => {
            return Err(LoomError::invalid(format!(
                "unsupported archive kind {other:?}; expected tar-zstd, tar, tar-gzip, zip, or gzip"
            )));
        }
    })
}

/// An [`loom_interchange::ArchiveManifest`] as a canonical-CBOR array
/// `[archive_id, kind, root_digest, entries]`.
fn archive_manifest_to_value(m: &loom_interchange::ArchiveManifest) -> loom_codec::Value {
    use loom_codec::Value;
    Value::Array(vec![
        Value::Text(m.archive_id.clone()),
        Value::Text(archive_kind_str(m.kind).to_string()),
        Value::Text(m.root_digest.to_string()),
        Value::Array(m.entries.iter().map(|entry| entry.to_value()).collect()),
    ])
}

/// Canonical CBOR of an [`loom_interchange_io::ArchiveExportResult`] as `[manifest, export_report]`.
fn archive_export_result_to_cbor(r: &loom_interchange_io::ArchiveExportResult) -> Vec<u8> {
    use loom_codec::Value;
    loom_codec::encode(&Value::Array(vec![
        archive_manifest_to_value(&r.manifest),
        export_report_to_value(&r.report),
    ]))
    .expect("canonical CBOR encode of ArchiveExportResult")
}

/// Canonical CBOR of a [`loom_interchange_io::CarImportResult`] as `[workspace?, root_cid_hex, blocks_read,
/// import_report]`.
fn car_import_result_to_cbor(r: &loom_interchange_io::CarImportResult) -> Vec<u8> {
    use loom_codec::Value;
    loom_codec::encode(&Value::Array(vec![
        r.workspace
            .as_ref()
            .map_or(Value::Null, |w| Value::Text(w.to_string())),
        Value::Text(r.root_cid_hex.clone()),
        Value::Uint(r.blocks_read),
        import_report_to_value(&r.report),
    ]))
    .expect("canonical CBOR encode of CarImportResult")
}

/// Canonical CBOR of a [`loom_interchange_io::CarExportResult`] as `[root_cid_hex, blocks_written,
/// bytes_out, export_report]`.
fn car_export_result_to_cbor(r: &loom_interchange_io::CarExportResult) -> Vec<u8> {
    use loom_codec::Value;
    loom_codec::encode(&Value::Array(vec![
        Value::Text(r.root_cid_hex.clone()),
        Value::Uint(r.blocks_written),
        Value::Uint(r.bytes_out),
        export_report_to_value(&r.report),
    ]))
    .expect("canonical CBOR encode of CarExportResult")
}

fn program_record_value(record: &StoredProgram) -> loom_codec::Value {
    use loom_codec::Value;
    Value::Array(vec![
        Value::Text(record.name.clone()),
        Value::Text(record.manifest_digest.to_string()),
        Value::Text(record.body_digest.to_string()),
        Value::Uint(record.body_len),
        Value::Bytes(record.manifest.encode()),
    ])
}

fn program_record_to_cbor(record: &StoredProgram) -> Result<Vec<u8>, LoomError> {
    loom_codec::encode(&program_record_value(record))
        .map_err(|e| LoomError::new(Code::Internal, format!("encode program record: {e}")))
}

fn program_body_to_cbor(body: &ProgramBody) -> Result<Vec<u8>, LoomError> {
    use loom_codec::Value;
    loom_codec::encode(&Value::Array(vec![
        program_record_value(&body.record),
        Value::Bytes(body.body.clone()),
    ]))
    .map_err(|e| LoomError::new(Code::Internal, format!("encode program body: {e}")))
}

fn program_list_to_cbor(records: &[StoredProgram]) -> Result<Vec<u8>, LoomError> {
    use loom_codec::Value;
    loom_codec::encode(&Value::Array(
        records.iter().map(program_record_value).collect(),
    ))
    .map_err(|e| LoomError::new(Code::Internal, format!("encode program list: {e}")))
}

/// Canonical CBOR of a [`MetricQueryResult`]: `[observations, partial, stale]`, where `observations`
/// is an array of canonical observation-record byte strings.
fn metric_query_result_to_cbor(result: &MetricQueryResult) -> Result<Vec<u8>, LoomError> {
    use loom_codec::Value;
    let observations = result
        .observations
        .iter()
        .map(|o| o.encode().map(Value::Bytes))
        .collect::<Result<Vec<_>, LoomError>>()?;
    loom_codec::encode(&Value::Array(vec![
        Value::Array(observations),
        Value::Bool(result.partial),
        Value::Bool(result.stale),
    ]))
    .map_err(|e| LoomError::new(Code::Internal, format!("encode metric query result: {e}")))
}

fn log_query_result_to_cbor(result: &LogQueryResult) -> Result<Vec<u8>, LoomError> {
    use loom_codec::Value;
    let records = result
        .records
        .iter()
        .map(|record| record.encode().map(Value::Bytes))
        .collect::<Result<Vec<_>, LoomError>>()?;
    loom_codec::encode(&Value::Array(vec![
        Value::Array(records),
        Value::Bool(result.partial),
    ]))
    .map_err(|e| LoomError::new(Code::Internal, format!("encode log query result: {e}")))
}

fn trace_query_result_to_cbor(result: &TraceQueryResult) -> Result<Vec<u8>, LoomError> {
    use loom_codec::Value;
    let spans = result
        .spans
        .iter()
        .map(|span| span.encode().map(Value::Bytes))
        .collect::<Result<Vec<_>, LoomError>>()?;
    loom_codec::encode(&Value::Array(vec![
        Value::Array(spans),
        Value::Bool(result.partial),
    ]))
    .map_err(|e| LoomError::new(Code::Internal, format!("encode trace query result: {e}")))
}

fn serve_drive_policy_registry_write(
    store: &FileStore,
    actor: Option<WorkspaceId>,
    workspace: WorkspaceId,
) -> Result<(WorkflowControlWrite, WorkflowAuditWrite), LoomError> {
    let mut registry = store
        .control_get(&drive_policy_registry_key())?
        .map(|bytes| DrivePolicyRegistry::decode(&bytes))
        .transpose()?
        .unwrap_or_else(DrivePolicyRegistry::empty);
    let workspace_id = workspace.to_string();
    registry.upsert_enabled(DrivePolicyTarget::new(
        workspace,
        workspace_id.clone(),
        true,
    )?)?;
    let target = format!("workspace={workspace};profile={workspace_id};enabled=true");
    Ok((
        WorkflowControlWrite::Put {
            key: drive_policy_registry_key(),
            payload: registry.encode()?,
        },
        WorkflowAuditWrite {
            principal: actor,
            action: "drive.policy_registry.configure".to_string(),
            target: Some(target),
        },
    ))
}

fn inference_instance_settings_from_json(
    settings_json: &str,
) -> Result<BTreeMap<String, String>, LoomError> {
    serde_json::from_str(settings_json).map_err(|error| {
        LoomError::new(
            Code::InvalidArgument,
            format!("invalid inference instance settings JSON: {error}"),
        )
    })
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct InferenceInstanceJsonView<'a> {
    instance: &'a loom_types::InferenceInstanceDescriptor,
    refs: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct InferenceInstanceDeleteJson {
    name: String,
    deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InferenceInstanceMutationReceipt {
    result_json: String,
    audit_sequence: u64,
}

fn inference_instance_view_json(
    state: &loom_inference::InferenceInstanceState,
    instance: &loom_types::InferenceInstanceDescriptor,
) -> Result<String, LoomError> {
    serde_json::to_string(&InferenceInstanceJsonView {
        instance,
        refs: state.instance_ref_count(&instance.name),
    })
    .map_err(|error| LoomError::new(Code::CorruptObject, format!("json: {error}")))
}

fn inference_instance_delete_result_json(name: &str) -> Result<String, LoomError> {
    serde_json::to_string(&InferenceInstanceDeleteJson {
        name: name.to_string(),
        deleted: true,
    })
    .map_err(|error| LoomError::new(Code::CorruptObject, format!("json: {error}")))
}

fn inference_instance_audit_target(workspace: WorkspaceId, instance: &str) -> String {
    format!("workspace={workspace};instance={instance}")
}

fn inference_instance_result_with_audit_sequence(
    json: &str,
    audit_sequence: u64,
) -> Result<String, LoomError> {
    let mut value = serde_json::from_str::<serde_json::Value>(json)
        .map_err(|error| LoomError::new(Code::CorruptObject, format!("json: {error}")))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| LoomError::corrupt("inference mutation result must be a JSON object"))?;
    object.insert(
        "audit-sequence".to_string(),
        serde_json::Value::Number(serde_json::Number::from(audit_sequence)),
    );
    serde_json::to_string(&value)
        .map_err(|error| LoomError::new(Code::CorruptObject, format!("json: {error}")))
}

/// Canonical CBOR of a [`loom_interchange::FidelityIssue`]: `[severity_tag, source_entity_id, field,
/// reason, source_digest?]` (severity 0=Info, 1=Warning, 2=Error).
fn fidelity_issue_to_value(fi: &loom_interchange::FidelityIssue) -> loom_codec::Value {
    use loom_codec::Value;
    let severity = match fi.severity {
        loom_interchange::FidelitySeverity::Info => 0u64,
        loom_interchange::FidelitySeverity::Warning => 1,
        loom_interchange::FidelitySeverity::Error => 2,
    };
    Value::Array(vec![
        Value::Uint(severity),
        Value::Text(fi.source_entity_id.clone()),
        Value::Text(fi.field.clone()),
        Value::Text(fi.reason.clone()),
        fi.source_digest
            .as_ref()
            .map_or(Value::Null, |d| Value::Text(d.to_string())),
    ])
}

#[derive(Debug)]
struct BundlePlanStore {
    algo: Algo,
    objects: Mutex<BTreeMap<[u8; 32], Vec<u8>>>,
}

impl BundlePlanStore {
    fn new(algo: Algo) -> Self {
        Self {
            algo,
            objects: Mutex::new(BTreeMap::new()),
        }
    }
}

impl ObjectStore for BundlePlanStore {
    fn put(&self, canonical: &[u8]) -> Result<Digest, LoomError> {
        let digest = Digest::hash(self.algo, canonical);
        self.objects
            .lock()
            .expect("bundle plan store lock")
            .entry(*digest.bytes())
            .or_insert_with(|| canonical.to_vec());
        Ok(digest)
    }

    fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>, LoomError> {
        Ok(self
            .objects
            .lock()
            .expect("bundle plan store lock")
            .get(digest.bytes())
            .cloned())
    }

    fn has(&self, digest: &Digest) -> Result<bool, LoomError> {
        Ok(self
            .objects
            .lock()
            .expect("bundle plan store lock")
            .contains_key(digest.bytes()))
    }

    fn len(&self) -> usize {
        self.objects.lock().expect("bundle plan store lock").len()
    }

    fn digest_algo(&self) -> Algo {
        self.algo
    }
}

fn decode_store_bundle(bundle_bytes: &[u8]) -> Result<Bundle, LoomError> {
    Bundle::decode(bundle_bytes).map_err(|error| {
        LoomError::new(Code::InvalidArgument, format!("bundle: {}", error.message))
    })
}

pub fn apply_store_bundle_import_generated(
    loom: &mut Loom<FileStore>,
    bundle_bytes: &[u8],
    dry_run: bool,
) -> Result<(Vec<u8>, bool), LoomError> {
    loom.authorize_global_admin()?;
    let bundle = decode_store_bundle(bundle_bytes)?;
    let report = if dry_run {
        plan_store_bundle_import(loom, &bundle)?
    } else {
        plan_store_bundle_import(loom, &bundle)?;
        let (workspace, sync_report) = bundle_import(loom, &bundle)?;
        store_bundle_import_result(&bundle, workspace, sync_report, false)
    };
    Ok((
        loom_wire::store_admin::store_bundle_import_result_to_cbor(&report),
        !dry_run,
    ))
}

fn check_store_bundle_profile(loom: &Loom<FileStore>, bundle: &Bundle) -> Result<(), LoomError> {
    let dst_algo = loom.store().digest_algo();
    if bundle.digest_algo == dst_algo {
        Ok(())
    } else {
        Err(LoomError::new(
            Code::Conflict,
            format!(
                "identity-profile mismatch: source is {}, destination is {}; sync requires matching profiles",
                bundle.digest_algo.as_str(),
                dst_algo.as_str()
            ),
        ))
    }
}

fn reject_existing_bundle_workspace(
    loom: &Loom<FileStore>,
    bundle: &Bundle,
) -> Result<(), LoomError> {
    match loom
        .registry()
        .open(&WsSelector::Name(bundle.ns_name.clone()))
    {
        Ok(_) => {
            return Err(LoomError::new(
                Code::AlreadyExists,
                format!("workspace {:?} already exists", bundle.ns_name),
            ));
        }
        Err(error) if error.code == Code::NotFound => {}
        Err(error) => return Err(error),
    }
    match loom.registry().open(&WsSelector::Id(bundle.ns_id)) {
        Ok(_) => Err(LoomError::new(
            Code::AlreadyExists,
            format!("workspace id {} already in use", bundle.ns_id),
        )),
        Err(error) if error.code == Code::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn plan_store_bundle_import(
    loom: &Loom<FileStore>,
    bundle: &Bundle,
) -> Result<loom_wire::store_admin::StoreBundleImportResult, LoomError> {
    check_store_bundle_profile(loom, bundle)?;
    reject_existing_bundle_workspace(loom, bundle)?;
    let dst_algo = loom.store().digest_algo();
    let mut temp = Loom::new(BundlePlanStore::new(dst_algo));
    let mut report = loom_core::SyncReport::default();
    for frame in &bundle.objects {
        let digest = Digest::hash(dst_algo, frame);
        if loom.has_object(digest)? {
            report.objects_skipped = report.objects_skipped.saturating_add(1);
        } else {
            report.objects_transferred = report.objects_transferred.saturating_add(1);
        }
        temp.ingest_object(frame)?;
    }
    for (_, tip) in bundle.branches.iter().chain(bundle.tags.iter()) {
        temp.reachable(&[*tip], &BTreeSet::new())?;
    }
    for (branch, tip) in &bundle.branches {
        report.new_tips.push((branch.clone(), *tip));
    }
    Ok(store_bundle_import_result(
        bundle,
        bundle.ns_id,
        report,
        true,
    ))
}

fn store_bundle_import_result(
    bundle: &Bundle,
    workspace: WorkspaceId,
    report: loom_core::SyncReport,
    dry_run: bool,
) -> loom_wire::store_admin::StoreBundleImportResult {
    loom_wire::store_admin::StoreBundleImportResult {
        workspace_id: workspace.to_string(),
        workspace_name: bundle.ns_name.clone(),
        facets: bundle
            .facets
            .iter()
            .map(|facet| facet.as_str().to_string())
            .collect(),
        objects_transferred: report.objects_transferred,
        objects_skipped: report.objects_skipped,
        new_tips: report
            .new_tips
            .into_iter()
            .map(|(name, tip)| format!("{name}:{tip}"))
            .collect(),
        dry_run,
    }
}

/// Resolve a workspace by UUID or by unique name for a read; a name or UUID identifies a workspace on its
/// own.
fn resolve_ns_read(loom: &Loom<FileStore>, name: &str) -> Result<WorkspaceId, LoomError> {
    let selector = match WorkspaceId::parse(name) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Name(name.to_string()),
    };
    loom.registry().open(&selector)
}

pub fn execute_generated_sql_result(
    path: &Path,
    loom: &mut Loom<FileStore>,
    workspace: &str,
    db: &str,
    sql: &str,
) -> Result<Vec<u8>, LoomError> {
    let mut candidate =
        loom.fork_state_into(loom_core::provider::PlanningObjectStore::new(loom.store()))?;
    let ns = resolve_ns_read(loom, workspace)?;
    candidate.authorize(ns, FacetKind::Sql, AclRight::Write)?;
    candidate.registry_mut().add_facet(ns, FacetKind::Sql)?;
    let mut store = LoomSqlStore::load_eager_write(&candidate, ns, db)?;
    let out = store.exec_cbor(sql)?;
    if store.in_transaction() {
        return Err(LoomError::invalid(
            "BEGIN without a matching COMMIT/ROLLBACK in one exec",
        ));
    }
    if store.is_dirty() {
        store.persist(&mut candidate, ns, db)?;
        let published = save_generated_planning_candidate(path, loom.store(), &mut candidate)?;
        drop(candidate);
        loom.import_engine_state_preserving_mutable_overlay(&published.engine_state)?;
    }
    Ok(out)
}

fn studio_reindex(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    profile: &str,
) -> Result<StudioReindexReport, LoomError> {
    let source_digest = studio_reindex_source_digest(loom, workspace, profile)?;
    let job = studio_reindex_job(workspace, profile, source_digest, None)?;
    let job_path = job.job_path(loom.store().digest_algo())?;
    loom.create_directory_reserved(workspace, EMBEDDING_PROJECTION_JOBS_DIR, true)?;
    loom.write_file_reserved(workspace, &job_path, &job.encode()?, 0o100644)?;
    let mut vector_records_indexed = 0;
    let mut vector_records_deleted = 0;
    if let Some(resolved) = resolve_optional_vector_binding(loom, workspace, None)? {
        let summary = drain_meetings_vector_outputs(loom, workspace, profile, &resolved)?;
        vector_records_indexed = summary.indexed;
        vector_records_deleted = summary.deleted;
    }
    Ok(StudioReindexReport {
        workspace: workspace.to_string(),
        profile: profile.to_string(),
        job_path,
        state: job.state.as_str().to_string(),
        source_digest: source_digest.to_string(),
        model_id: job.stamp.model_id,
        vector_records_indexed,
        vector_records_deleted,
    })
}

fn studio_reindex_source_digest(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    profile: &str,
) -> Result<Digest, LoomError> {
    let head = loom.registry().head_branch(workspace)?;
    if let Some(tip) = loom.registry().branch_tip(workspace, &head)? {
        Ok(tip)
    } else {
        let seed = format!("studio-reindex:{workspace}:{profile}");
        Ok(Digest::hash(loom.store().digest_algo(), seed.as_bytes()))
    }
}

fn studio_reindex_job(
    workspace: WorkspaceId,
    profile: &str,
    source_digest: Digest,
    instance: Option<&loom_types::InferenceInstanceDescriptor>,
) -> Result<EmbeddingProjectionJob, LoomError> {
    let key = EmbeddingProjectionKey::new(workspace.to_string(), "studio", profile, "reindex")?;
    let stamp = match instance {
        Some(instance) => studio_reindex_stamp_for_instance(source_digest, instance)?,
        None => EmbeddingProjectionStamp::new(
            source_digest,
            "loom-built-in-embedding",
            None,
            "unconfigured",
        )?,
    };
    let job = EmbeddingProjectionJob::queued(key, stamp);
    match instance {
        Some(_) => Ok(job),
        None => Ok(job.no_engine("built-in embedding inference is not configured")?),
    }
}

fn studio_reindex_stamp_for_instance(
    source_digest: Digest,
    instance: &loom_types::InferenceInstanceDescriptor,
) -> Result<EmbeddingProjectionStamp, LoomError> {
    let descriptor_bytes = serde_json::to_vec(instance)
        .map_err(|error| LoomError::new(Code::CorruptObject, format!("json: {error}")))?;
    let descriptor_digest = Digest::hash(source_digest.algo(), &descriptor_bytes);
    EmbeddingProjectionStamp::new(
        source_digest,
        format!(
            "{}@{}",
            instance.model.repo_id,
            instance.model.revision.value()
        ),
        None,
        format!(
            "{}:{}",
            instance.runtime.as_str(),
            descriptor_digest.to_hex()
        ),
    )
}

fn resolve_optional_vector_binding(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    instance: Option<&loom_types::InferenceInstanceDescriptor>,
) -> Result<Option<ResolvedStudioTextEmbeddingInstance>, LoomError> {
    let cache_dir = inference_cache_dir()?;
    let mut hardware = loom_inference::probe_hardware()?;
    hardware.hf_cache_dir = Some(cache_dir.to_string_lossy().into_owned());
    let state = inference_instance_state(loom, workspace)?;
    let instance_name = match instance {
        Some(instance) => instance.name.clone(),
        None => match state
            .vector_bindings
            .iter()
            .find(|binding| binding.workspace == workspace.to_string())
        {
            Some(binding) => binding.embedding_instance.clone(),
            None => return Ok(None),
        },
    };
    let instance = state
        .find_instance(&instance_name)
        .cloned()
        .ok_or_else(|| {
            LoomError::new(
                Code::NotFound,
                format!("inference instance {instance_name:?} not found"),
            )
        })?;
    if instance.kind != InferenceModelKind::TextEmbedding {
        return Err(LoomError::new(
            Code::InvalidArgument,
            format!("inference instance {instance_name:?} is not a text-embedding instance"),
        ));
    }
    let record =
        loom_inference::discover_installed_model(&cache_dir, &instance.model, instance.runtime)?
            .ok_or_else(|| {
                LoomError::new(
                    Code::NotFound,
                    format!(
                        "model {:?} is not installed for runtime {}",
                        instance.model.repo_id,
                        instance.runtime.as_str()
                    ),
                )
            })?;
    let handle = loom_inference::activate_text_embedding(&record, &hardware, &cache_dir)?;
    Ok(Some(ResolvedStudioTextEmbeddingInstance {
        instance,
        handle,
    }))
}

fn inference_cache_dir() -> Result<PathBuf, LoomError> {
    if let Some(hf_home) = std::env::var_os("HF_HOME") {
        return Ok(PathBuf::from(hf_home).join("hub"));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| LoomError::new(Code::Unavailable, "home directory is unavailable"))?;
    Ok(home.join(".cache").join("huggingface").join("hub"))
}

fn drain_meetings_vector_outputs(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    profile: &str,
    resolved: &ResolvedStudioTextEmbeddingInstance,
) -> Result<StudioVectorDrainSummary, LoomError> {
    let model = resolved.handle.model().ok_or_else(|| {
        LoomError::new(
            Code::Unavailable,
            "text embedding provider did not expose a model",
        )
    })?;
    let mut summary = StudioVectorDrainSummary {
        indexed: 0,
        deleted: 0,
    };
    for profile_id in studio_meetings_profile_ids(workspace, profile) {
        let Some(snapshot) = load_meetings_snapshot(loom, &profile_id)? else {
            continue;
        };
        let profile_root = Digest::hash(loom.store().digest_algo(), &snapshot.encode()?);
        let output_set = ProjectionOutputSet::from_snapshot(&snapshot)?;
        let collection = meetings_vector_collection(&profile_id);
        match vector_create(
            loom,
            workspace,
            &collection,
            model.dimension,
            Metric::Cosine,
        ) {
            Ok(()) => {}
            Err(error) if error.code == Code::Conflict => {}
            Err(error) => return Err(error),
        }
        for output in output_set.outputs_for(ProjectionKind::Vector) {
            let job = meetings_vector_projection_job(
                workspace,
                &profile_id,
                profile_root,
                output,
                resolved,
            )?;
            let path = job.job_path(loom.store().digest_algo())?;
            match output.action {
                ProjectionAction::Upsert | ProjectionAction::Append => {
                    loom_core::vector_upsert_text(
                        loom,
                        workspace,
                        &collection,
                        &meetings_vector_id(output),
                        &output.text_body(),
                        meetings_vector_metadata(output),
                        &resolved.handle,
                    )?;
                    summary.indexed = summary.indexed.saturating_add(1);
                }
                ProjectionAction::Invalidate | ProjectionAction::RetainMetadata => {
                    if vector_delete(loom, workspace, &collection, &meetings_vector_id(output))? {
                        summary.deleted = summary.deleted.saturating_add(1);
                    }
                }
            }
            loom.create_directory_reserved(workspace, EMBEDDING_PROJECTION_JOBS_DIR, true)?;
            loom.write_file_reserved(workspace, &path, &job.ready().encode()?, 0o100644)?;
        }
    }
    Ok(summary)
}

fn load_meetings_snapshot(
    loom: &Loom<FileStore>,
    profile_id: &str,
) -> Result<Option<MeetingsProfileSnapshot>, LoomError> {
    let key = meetings_profile_key(profile_id)?;
    loom.store()
        .control_get(&key)?
        .map(|bytes| MeetingsProfileSnapshot::decode(&bytes))
        .transpose()
}

fn studio_meetings_profile_ids(workspace: WorkspaceId, profile: &str) -> Vec<String> {
    match profile {
        "all" | "meetings" => vec![workspace.to_string()],
        profile => vec![profile.to_string()],
    }
}

fn meetings_vector_collection(profile_id: &str) -> String {
    format!("meetings/{profile_id}")
}

fn meetings_vector_id(output: &ProjectionOutput) -> String {
    output
        .output_ref
        .strip_prefix("vector:")
        .unwrap_or(&output.output_ref)
        .to_string()
}

fn meetings_vector_metadata(output: &ProjectionOutput) -> BTreeMap<String, Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "entity_kind".to_string(),
        Value::Text(output.entity_kind.clone()),
    );
    metadata.insert(
        "entity_id".to_string(),
        Value::Text(output.entity_id.clone()),
    );
    metadata.insert(
        "output_ref".to_string(),
        Value::Text(output.output_ref.clone()),
    );
    metadata.insert(
        "output_id".to_string(),
        Value::Text(output.output_id.clone()),
    );
    metadata.insert(
        "source_ids".to_string(),
        Value::List(output.source_ids.iter().cloned().map(Value::Text).collect()),
    );
    metadata
}

fn meetings_vector_projection_job(
    workspace: WorkspaceId,
    profile_id: &str,
    source_digest: Digest,
    output: &ProjectionOutput,
    resolved: &ResolvedStudioTextEmbeddingInstance,
) -> Result<EmbeddingProjectionJob, LoomError> {
    let key = EmbeddingProjectionKey::new(
        workspace.to_string(),
        "meetings",
        profile_id,
        &output.output_id,
    )?;
    let stamp = studio_reindex_stamp_for_instance(source_digest, &resolved.instance)?;
    Ok(EmbeddingProjectionJob::queued(key, stamp))
}

fn rebuild_studio_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    profile: &str,
    dry_run: bool,
) -> Result<StudioRevisionRebuildReport, LoomError> {
    let scope_id = workspace.to_string();
    match profile {
        "drive" => rebuild_drive_revision_index(loom, workspace, &scope_id, dry_run),
        "lifecycle" => rebuild_lifecycle_revision_index(loom, workspace, &scope_id, dry_run),
        "meetings" => rebuild_meetings_revision_index(loom, workspace, &scope_id, dry_run),
        "pages" => rebuild_pages_revision_index(loom, workspace, &scope_id, dry_run),
        other => Err(LoomError::new(
            Code::InvalidArgument,
            format!(
                "unsupported Studio revision rebuild profile {other:?}; supported profiles: drive, lifecycle, meetings, pages"
            ),
        )),
    }
}

fn rebuild_meetings_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    scope_id: &str,
    dry_run: bool,
) -> Result<StudioRevisionRebuildReport, LoomError> {
    loom.authorize(workspace, FacetKind::Vcs, AclRight::Write)?;
    let key = meetings_profile_key(scope_id)?;
    let Some(bytes) = loom.store().control_get(&key)? else {
        return Err(LoomError::new(
            Code::NotFound,
            "meetings snapshot not found",
        ));
    };
    let snapshot = MeetingsProfileSnapshot::decode(&bytes)?;
    let root = Digest::hash(loom.store().digest_algo(), &bytes);
    let updates = snapshot
        .meetings
        .iter()
        .map(|meeting| {
            let body = meeting.encode()?;
            RevisionBackfillUpdate::new(
                format!("meeting:{}", meeting.meeting_id),
                format!("meetings:{scope_id}:{}:backfill:1", meeting.meeting_id),
                BodyRef::new(
                    Digest::hash(loom.store().digest_algo(), &body),
                    body.len() as u64,
                    "application/vnd.uldren.loom.meetings.meeting+cbor",
                )?,
                root,
                meeting.updated_at_ms,
                format!("{}:backfill:1", meeting.meeting_id),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    apply_revision_backfill(loom, workspace, scope_id, "meetings", dry_run, updates)
}

fn rebuild_drive_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    scope_id: &str,
    dry_run: bool,
) -> Result<StudioRevisionRebuildReport, LoomError> {
    loom.authorize(workspace, FacetKind::Vcs, AclRight::Write)?;
    let key = drive_operation_log_key(scope_id)?;
    let Some(bytes) = loom.store().control_get(&key)? else {
        return Err(LoomError::new(
            Code::NotFound,
            "drive operation log not found",
        ));
    };
    let log = DriveOperationLog::decode(&bytes)?;
    let mut latest = BTreeMap::new();
    for record in log.records.iter().rev() {
        let Some(target) = record.target_entity_id.as_deref() else {
            continue;
        };
        let entity_id = format!("drive:metadata:{target}");
        if latest.contains_key(&entity_id) {
            continue;
        }
        let envelope = OperationEnvelope::decode(&record.envelope)?;
        latest.insert(
            entity_id.clone(),
            revision_backfill_update(
                loom,
                entity_id,
                record.operation_id.clone(),
                record.root_after,
                &record.envelope,
                "application/vnd.uldren.loom.drive.operation+cbor",
                envelope.timestamp_ms,
                format!("drive:metadata:{target}:backfill:1"),
            )?,
        );
    }
    apply_revision_backfill(
        loom,
        workspace,
        scope_id,
        "drive",
        dry_run,
        latest.into_values().collect(),
    )
}

fn rebuild_pages_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    scope_id: &str,
    dry_run: bool,
) -> Result<StudioRevisionRebuildReport, LoomError> {
    loom.authorize(workspace, FacetKind::Vcs, AclRight::Write)?;
    let log = loom_pages::page_operation_log(loom.store(), scope_id)?;
    if log.records.is_empty() {
        return Err(LoomError::new(
            Code::NotFound,
            "pages operation log not found",
        ));
    }
    let mut latest = BTreeMap::new();
    for record in log.records.iter().rev() {
        let Some(target) = record.target_entity_id.as_deref() else {
            continue;
        };
        let entity_id = page_operation_revision_entity_id(record.operation_kind.as_str(), target);
        if latest.contains_key(&entity_id) {
            continue;
        }
        let envelope = OperationEnvelope::decode(&record.envelope)?;
        latest.insert(
            entity_id.clone(),
            revision_backfill_update(
                loom,
                entity_id,
                record.operation_id.clone(),
                record.root_after,
                &record.envelope,
                "application/vnd.uldren.loom.pages.operation+cbor",
                envelope.timestamp_ms,
                format!("pages:{scope_id}:{target}:backfill:1"),
            )?,
        );
    }
    apply_revision_backfill(
        loom,
        workspace,
        scope_id,
        "pages",
        dry_run,
        latest.into_values().collect(),
    )
}

fn page_operation_revision_entity_id(operation_kind: &str, target_entity_id: &str) -> String {
    match operation_kind {
        "space.created" => format!("space:{target_entity_id}"),
        "page.created" | "page.updated" => format!("page:draft:{target_entity_id}"),
        "structure.created" => format!("structure:{target_entity_id}"),
        "structure.node_added"
        | "structure.node_updated"
        | "structure.node_bound"
        | "structure.node_moved" => format!("structure-node:{target_entity_id}"),
        "structure.node_linked" => format!("structure-edge:{target_entity_id}"),
        _ => format!("pages:operation:{target_entity_id}"),
    }
}

fn rebuild_lifecycle_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    scope_id: &str,
    dry_run: bool,
) -> Result<StudioRevisionRebuildReport, LoomError> {
    loom.authorize(workspace, FacetKind::Vcs, AclRight::Write)?;
    let mut updates = Vec::new();
    for (key, bytes) in loom
        .store()
        .control_scan_prefix(format!("profile/lifecycle/v1/{scope_id}/definitions/").as_bytes())?
    {
        let definition_id = String::from_utf8_lossy(&key)
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        let root = Digest::hash(loom.store().digest_algo(), &bytes);
        updates.push(revision_backfill_update(
            loom,
            format!("lifecycle:definition:{definition_id}"),
            format!("lifecycle.definition.backfill:{scope_id}:{definition_id}"),
            root,
            &bytes,
            "application/vnd.uldren.loom.lifecycle.definition+cbor",
            0,
            format!("lifecycle:definition:{definition_id}:backfill:1"),
        )?);
    }
    for (key, bytes) in loom
        .store()
        .control_scan_prefix(format!("profile/lifecycle/v1/{scope_id}/instances/").as_bytes())?
    {
        let instance_id = String::from_utf8_lossy(&key)
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        let root = Digest::hash(loom.store().digest_algo(), &bytes);
        updates.push(revision_backfill_update(
            loom,
            format!("lifecycle:instance:{instance_id}"),
            format!("lifecycle.instance.backfill:{scope_id}:{instance_id}"),
            root,
            &bytes,
            "application/vnd.uldren.loom.lifecycle.instance+cbor",
            0,
            format!("lifecycle:instance:{instance_id}:backfill:1"),
        )?);
    }
    let key = lifecycle_operation_log_key(scope_id)?;
    if let Some(bytes) = loom.store().control_get(&key)? {
        let log = LifecycleOperationLog::decode(&bytes)?;
        for record in log.records.iter().rev() {
            let entity_id = format!("lifecycle:instance:{}", record.instance_id);
            if updates.iter().any(|update| update.entity_id == entity_id) {
                continue;
            }
            let envelope = OperationEnvelope::decode(&record.envelope)?;
            updates.push(revision_backfill_update(
                loom,
                entity_id,
                record.operation_id.clone(),
                record.root_after,
                &record.envelope,
                "application/vnd.uldren.loom.lifecycle.operation+cbor",
                envelope.timestamp_ms,
                format!("lifecycle:{}:backfill:1", record.instance_id),
            )?);
        }
    }
    apply_revision_backfill(loom, workspace, scope_id, "lifecycle", dry_run, updates)
}

fn apply_revision_backfill(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    scope_id: &str,
    profile: &str,
    dry_run: bool,
    updates: Vec<RevisionBackfillUpdate>,
) -> Result<StudioRevisionRebuildReport, LoomError> {
    let (mut index, index_present_before) =
        match load_optional_current_revision_index(loom, workspace, scope_id)? {
            Some(index) => (index, true),
            None => (RevisionIndex::new(), false),
        };
    let candidates = updates.len() as u64;
    let backfill = index.backfill_missing_current(scope_id, updates)?;
    if !dry_run && backfill.inserted > 0 {
        persist_current_revision_index(loom, workspace, scope_id, FacetKind::Document, &index)?;
    }
    Ok(StudioRevisionRebuildReport {
        workspace: workspace.to_string(),
        scope_id: scope_id.to_string(),
        profile: profile.to_string(),
        index_present_before,
        candidates,
        inserted: backfill.inserted,
        skipped_existing: backfill.skipped_existing,
        dry_run,
    })
}

fn revision_backfill_update(
    loom: &Loom<FileStore>,
    entity_id: String,
    operation_id: String,
    root: Digest,
    body: &[u8],
    media_type: &str,
    timestamp_ms: u64,
    checkpoint_id: String,
) -> Result<RevisionBackfillUpdate, LoomError> {
    RevisionBackfillUpdate::new(
        entity_id,
        operation_id,
        BodyRef::new(
            Digest::hash(loom.store().digest_algo(), body),
            body.len() as u64,
            media_type,
        )?,
        root,
        timestamp_ms,
        checkpoint_id,
    )
}

struct ExecApplyRequest {
    workspace: String,
    base: String,
    fork: String,
    author: String,
    timestamp_ms: u64,
}

fn decode_exec_apply_request(bytes: &[u8]) -> Result<ExecApplyRequest, LoomError> {
    let value = loom_codec::decode(bytes).map_err(|err| {
        LoomError::new(
            Code::InvalidArgument,
            format!("decode exec apply request: {err}"),
        )
    })?;
    let loom_codec::Value::Map(fields) = value else {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "exec apply request must be a CBOR map",
        ));
    };
    let mut workspace = None;
    let mut base = None;
    let mut fork = None;
    let mut author = None;
    let mut timestamp_ms = None;
    for (key, value) in fields {
        let loom_codec::Value::Text(key) = key else {
            return Err(LoomError::new(
                Code::InvalidArgument,
                "exec apply request keys must be text",
            ));
        };
        match key.as_str() {
            "workspace" => workspace = Some(exec_apply_text_field("workspace", value)?),
            "base" => base = Some(exec_apply_text_field("base", value)?),
            "fork" => fork = Some(exec_apply_text_field("fork", value)?),
            "author" => author = Some(exec_apply_text_field("author", value)?),
            "timestamp_ms" => {
                let loom_codec::Value::Uint(value) = value else {
                    return Err(LoomError::new(
                        Code::InvalidArgument,
                        "exec apply timestamp_ms must be an unsigned integer",
                    ));
                };
                timestamp_ms = Some(value);
            }
            _ => {
                return Err(LoomError::new(
                    Code::InvalidArgument,
                    format!("unknown exec apply request field {key:?}"),
                ));
            }
        }
    }
    Ok(ExecApplyRequest {
        workspace: workspace.ok_or_else(|| exec_apply_missing_field("workspace"))?,
        base: base.ok_or_else(|| exec_apply_missing_field("base"))?,
        fork: fork.ok_or_else(|| exec_apply_missing_field("fork"))?,
        author: author.ok_or_else(|| exec_apply_missing_field("author"))?,
        timestamp_ms: timestamp_ms.ok_or_else(|| exec_apply_missing_field("timestamp_ms"))?,
    })
}

fn exec_apply_text_field(name: &str, value: loom_codec::Value) -> Result<String, LoomError> {
    match value {
        loom_codec::Value::Text(value) => Ok(value),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            format!("exec apply {name} must be text"),
        )),
    }
}

fn exec_apply_missing_field(name: &str) -> LoomError {
    LoomError::new(
        Code::InvalidArgument,
        format!("exec apply request missing {name}"),
    )
}

fn encode_exec_apply_result(outcome: &MergeOutcome) -> Result<Vec<u8>, LoomError> {
    use loom_codec::Value;
    let mut fields = vec![
        (
            Value::Text("schema".to_string()),
            Value::Text("loom.exec.apply.result.v1".to_string()),
        ),
        (
            Value::Text("outcome".to_string()),
            Value::Text(
                match outcome {
                    MergeOutcome::UpToDate => "up_to_date",
                    MergeOutcome::FastForward(_) => "fast_forward",
                    MergeOutcome::Merged(_) => "merged",
                    MergeOutcome::Conflicts(_) => "conflicts",
                }
                .to_string(),
            ),
        ),
    ];
    match outcome {
        MergeOutcome::UpToDate => {}
        MergeOutcome::FastForward(digest) | MergeOutcome::Merged(digest) => {
            fields.push((
                Value::Text("digest".to_string()),
                Value::Bytes(digest.bytes().to_vec()),
            ));
        }
        MergeOutcome::Conflicts(conflicts) => {
            fields.push((
                Value::Text("conflicts".to_string()),
                Value::Array(conflicts.iter().cloned().map(Value::Text).collect()),
            ));
        }
    }
    loom_codec::encode(&Value::Map(fields))
        .map_err(|err| LoomError::new(Code::Internal, format!("encode exec apply result: {err}")))
}

fn exec_error_to_loom(err: ExecError) -> LoomError {
    match err {
        ExecError::Core(err) => err,
        other => LoomError::new(other.code(), other.to_string()),
    }
}

fn authorize_generated_drive_collection<S: ObjectStore>(
    loom: &Loom<S>,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
) -> Result<(), LoomError> {
    loom.authorize_resource(
        AclResource::scoped(
            workspace,
            AclDomain::Files,
            None,
            AclResourceScope::Prefix {
                kind: AclScopeKind::Collection,
                value: drive_workspace_id.as_bytes(),
            },
        ),
        AclRight::Write,
    )
}

fn parse_drive_conflict_resolution(
    resolution: &str,
) -> Result<loom_drive::HostedDriveConflictResolution, LoomError> {
    match resolution {
        "keep_current" | "keep-current" => {
            Ok(loom_drive::HostedDriveConflictResolution::KeepCurrent)
        }
        "keep_conflict" | "keep-conflict" => {
            Ok(loom_drive::HostedDriveConflictResolution::KeepConflict)
        }
        "keep_both" | "keep-both" => Ok(loom_drive::HostedDriveConflictResolution::KeepBoth),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "resolution must be keep_current, keep_conflict, or keep_both",
        )),
    }
}

pub(crate) struct GeneratedPlanningCandidatePublication {
    pub(crate) engine_state: Vec<u8>,
    pub(crate) workflow_receipts: Vec<loom_core::CommitReceipt>,
}

fn save_exec_apply_candidate(
    path: &Path,
    store: &FileStore,
    candidate: &mut Loom<loom_core::provider::PlanningObjectStore<'_, FileStore>>,
) -> Result<Vec<u8>, LoomError> {
    Ok(save_generated_planning_candidate(path, store, candidate)?.engine_state)
}

pub(crate) fn save_generated_planning_candidate(
    path: &Path,
    store: &FileStore,
    candidate: &mut Loom<loom_core::provider::PlanningObjectStore<'_, FileStore>>,
) -> Result<GeneratedPlanningCandidatePublication, LoomError> {
    save_generated_planning_candidate_with_audits(path, store, candidate, Vec::new())
}

pub(crate) fn save_generated_planning_candidate_with_audits(
    path: &Path,
    store: &FileStore,
    candidate: &mut Loom<loom_core::provider::PlanningObjectStore<'_, FileStore>>,
    audits: Vec<WorkflowAuditWrite>,
) -> Result<GeneratedPlanningCandidatePublication, LoomError> {
    let (root, state_objects) = candidate.save_state_objects()?;
    let mut objects = candidate.store().objects()?;
    objects.extend(state_objects);
    let state = candidate.export_state();
    let workflow_transactions = candidate.store().workflow_transactions()?;
    let direct_controls = candidate.store().direct_control_writes()?;
    exec_apply_candidate_save_hook(path)?;
    #[cfg(any(test, feature = "test-hooks"))]
    generated_candidate_publication_test_observe(
        path,
        GeneratedCandidatePublicationTestEvent::Attempt,
    );
    let workflow_receipts = if workflow_transactions.is_empty() {
        if direct_controls.is_empty() {
            if audits.is_empty() {
                store.put_batch_and_set_reference_root(&objects, root)?;
            } else {
                loom_store::put_saved_state_and_audit(store, (root, objects), audits)?;
            }
        } else {
            if !audits.is_empty() {
                return Err(LoomError::corrupt(
                    "generated candidate left audits with direct control writes",
                ));
            }
            store.put_batch_control_delta_and_set_reference_root(
                &objects,
                direct_controls,
                root,
            )?;
        }
        Vec::new()
    } else {
        if !audits.is_empty() {
            return Err(LoomError::corrupt(
                "generated candidate left audits with workflow transactions",
            ));
        }
        if !direct_controls.is_empty() {
            return Err(LoomError::corrupt(
                "workflow candidate left unowned direct control writes",
            ));
        }
        let mut receipts = Vec::new();
        for mut txn in workflow_transactions {
            txn.owner_state.objects.extend(objects.clone());
            txn.owner_state.reference = loom_core::WorkflowReferenceUpdate::Set(Some(root));
            receipts.push(store.commit_workflow_transaction(txn)?);
        }
        receipts
    };
    #[cfg(any(test, feature = "test-hooks"))]
    generated_candidate_publication_test_observe(
        path,
        GeneratedCandidatePublicationTestEvent::Success,
    );
    Ok(GeneratedPlanningCandidatePublication {
        engine_state: state,
        workflow_receipts,
    })
}

#[cfg(test)]
type ExecApplyCandidateSaveHook = Box<dyn FnOnce() -> Result<(), LoomError> + Send>;

#[cfg(test)]
static EXEC_APPLY_CANDIDATE_SAVE_HOOKS: std::sync::LazyLock<
    Mutex<HashMap<PathBuf, ExecApplyCandidateSaveHook>>,
> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneratedCandidatePublicationTestEvent {
    Attempt,
    Success,
}

#[cfg(any(test, feature = "test-hooks"))]
type GeneratedCandidatePublicationTestObserver =
    std::sync::Arc<dyn Fn(GeneratedCandidatePublicationTestEvent) + Send + Sync>;

#[cfg(any(test, feature = "test-hooks"))]
static GENERATED_CANDIDATE_PUBLICATION_TEST_OBSERVERS: std::sync::LazyLock<
    Mutex<HashMap<PathBuf, (u64, GeneratedCandidatePublicationTestObserver)>>,
> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MeetingsImportTestHookPoint {
    AfterWorkspaceCreation,
    AfterPayloadRevisionPlanning,
    BeforePublication,
}

#[cfg(test)]
type MeetingsImportTestHook =
    Box<dyn FnMut(MeetingsImportTestHookPoint) -> Result<(), LoomError> + Send>;

#[cfg(test)]
static MEETINGS_IMPORT_TEST_HOOK: Mutex<Option<(PathBuf, MeetingsImportTestHook)>> =
    Mutex::new(None);

#[cfg(test)]
pub(crate) fn install_exec_apply_candidate_save_hook(
    path: PathBuf,
    hook: ExecApplyCandidateSaveHook,
) {
    EXEC_APPLY_CANDIDATE_SAVE_HOOKS
        .lock()
        .expect("exec apply save hook lock")
        .insert(path, hook);
}

#[cfg(test)]
fn exec_apply_candidate_save_hook(path: &Path) -> Result<(), LoomError> {
    let hook = EXEC_APPLY_CANDIDATE_SAVE_HOOKS
        .lock()
        .expect("exec apply save hook lock")
        .remove(path);
    if let Some(hook) = hook {
        hook()?;
    }
    Ok(())
}

#[cfg(not(test))]
fn exec_apply_candidate_save_hook(_path: &Path) -> Result<(), LoomError> {
    Ok(())
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn install_generated_candidate_publication_test_observer(
    path: PathBuf,
    observer: GeneratedCandidatePublicationTestObserver,
) -> GeneratedCandidatePublicationTestGuard {
    let id = GENERATED_CANDIDATE_PUBLICATION_TEST_OBSERVER_IDS.fetch_add(1, Ordering::SeqCst);
    GENERATED_CANDIDATE_PUBLICATION_TEST_OBSERVERS
        .lock()
        .expect("generated candidate publication observer lock")
        .insert(path.clone(), (id, observer));
    GeneratedCandidatePublicationTestGuard { path, id }
}

#[cfg(any(test, feature = "test-hooks"))]
pub struct GeneratedCandidatePublicationTestGuard {
    path: PathBuf,
    id: u64,
}

#[cfg(any(test, feature = "test-hooks"))]
impl Drop for GeneratedCandidatePublicationTestGuard {
    fn drop(&mut self) {
        let mut observers = GENERATED_CANDIDATE_PUBLICATION_TEST_OBSERVERS
            .lock()
            .expect("generated candidate publication observer lock");
        if observers
            .get(&self.path)
            .is_some_and(|(id, _)| *id == self.id)
        {
            observers.remove(&self.path);
        }
    }
}

#[cfg(any(test, feature = "test-hooks"))]
static GENERATED_CANDIDATE_PUBLICATION_TEST_OBSERVER_IDS: std::sync::LazyLock<AtomicU64> =
    std::sync::LazyLock::new(|| AtomicU64::new(1));

#[cfg(any(test, feature = "test-hooks"))]
pub fn generated_candidate_publication_test_observer_registered(path: &Path) -> bool {
    GENERATED_CANDIDATE_PUBLICATION_TEST_OBSERVERS
        .lock()
        .expect("generated candidate publication observer lock")
        .contains_key(path)
}

#[cfg(any(test, feature = "test-hooks"))]
fn generated_candidate_publication_test_observe(
    path: &Path,
    event: GeneratedCandidatePublicationTestEvent,
) {
    let observer = GENERATED_CANDIDATE_PUBLICATION_TEST_OBSERVERS
        .lock()
        .expect("generated candidate publication observer lock")
        .get(path)
        .map(|(_, observer)| std::sync::Arc::clone(observer));
    if let Some(observer) = observer {
        observer(event);
    }
}

#[cfg(test)]
fn install_meetings_import_test_hook(path: PathBuf, hook: MeetingsImportTestHook) {
    *MEETINGS_IMPORT_TEST_HOOK
        .lock()
        .expect("meetings import hook lock") = Some((path, hook));
}

#[cfg(test)]
fn clear_meetings_import_test_hook() {
    *MEETINGS_IMPORT_TEST_HOOK
        .lock()
        .expect("meetings import hook lock") = None;
}

#[cfg(test)]
fn meetings_import_test_hook(
    path: &std::path::Path,
    point: MeetingsImportTestHookPoint,
) -> Result<(), LoomError> {
    let mut guard = MEETINGS_IMPORT_TEST_HOOK
        .lock()
        .expect("meetings import hook lock");
    if let Some((target_path, hook)) = guard.as_mut()
        && target_path == path
    {
        hook(point)?;
    }
    Ok(())
}

#[cfg(not(test))]
fn meetings_import_test_hook(
    _path: &std::path::Path,
    _point: MeetingsImportTestHookPoint,
) -> Result<(), LoomError> {
    Ok(())
}

fn task_handle_key(task: &Task) -> Result<u64, LoomError> {
    let bytes: [u8; 8] = task
        .0
        .id
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::new(Code::InvalidArgument, "malformed task handle id"))?;
    Ok(u64::from_be_bytes(bytes))
}

/// A task's status as canonical CBOR text: `pending`/`ready`/`error`/`cancelled`/`taken`.
fn task_status_cbor(state: &TaskState) -> Vec<u8> {
    let label = match state {
        TaskState::Pending(_) => "pending",
        TaskState::Ready(_) => "ready",
        TaskState::Errored(_) => "error",
        TaskState::Cancelled => "cancelled",
        TaskState::Taken => "taken",
    };
    loom_codec::encode(&loom_codec::Value::Text(label.to_string()))
        .expect("task status text is always encodable")
}

fn ensure_batch_no_open_txn(state: &SqlBatchState) -> Result<(), LoomError> {
    if state.store.in_transaction() {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "the batch has an open SQL transaction; COMMIT or ROLLBACK before committing the batch",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_substrate::refs::ReferenceSource;

    #[derive(Debug, PartialEq, Eq)]
    struct ExecApplyWorkspaceSnapshot {
        head: String,
        main_log: Vec<Digest>,
        feature_log: Vec<Digest>,
        staged_file: Vec<u8>,
        status: Status,
        merge_in_progress: bool,
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("loom-client-210-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn direct_sql_exec(sql: &str) -> Result<Vec<u8>, LoomError> {
        let mut store = LoomSqlStore::default();
        store.exec_cbor(sql)
    }

    #[derive(Debug)]
    struct ClientFixedMaintenanceClock {
        now_ms: u64,
        instant: std::time::Instant,
    }

    impl StoreMaintenanceClock for ClientFixedMaintenanceClock {
        fn now_ms(&self) -> u64 {
            self.now_ms
        }

        fn monotonic_now(&self) -> std::time::Instant {
            self.instant
        }
    }

    fn cbor_array(bytes: &[u8]) -> Vec<loom_codec::Value> {
        match loom_codec::decode(bytes).expect("decode cbor") {
            loom_codec::Value::Array(items) => items,
            other => panic!("expected cbor array: {other:?}"),
        }
    }

    fn seed_authenticated_user(
        client: &LocalLoomClient,
        session: &LoomSession,
    ) -> (WorkspaceId, WorkspaceId) {
        let root = WorkspaceId::from_bytes([91; 16]);
        let user = WorkspaceId::from_bytes([92; 16]);
        client
            .with_session(session, |loom| {
                let mut identity = IdentityStore::new(root);
                identity.set_passphrase(root, "root-pass", b"root-salt")?;
                identity.add_principal(user, "user", PrincipalKind::User)?;
                identity.set_passphrase(user, "user-pass", b"user-salt")?;
                let mut acl = AclStore::new();
                acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])?;
                loom.store().save_identity_store(&identity)?;
                loom.store().save_acl_store(&acl)?;
                loom.set_identity_store(identity);
                loom.set_acl_store(acl);
                Ok(())
            })
            .expect("seed auth");
        (root, user)
    }

    fn malformed_cbor() -> Vec<u8> {
        vec![0xff]
    }

    #[test]
    fn store_maintenance_generated_unknown_session_is_not_found() {
        let dir = temp_dir("maintenance-unknown-session");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        let unknown = unknown_session(&session);
        let request = loom_wire::store_admin::store_maintenance_status_request_to_cbor(
            &loom_wire::store_admin::StoreMaintenanceStatusRequest {
                include_live_root_diagnostics: false,
            },
        );
        assert_eq!(
            client
                .store_maintenance_status(&unknown, &request)
                .expect_err("unknown session")
                .code,
            Code::NotFound
        );
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_maintenance_generated_authorizes_before_mutation_decode() {
        let dir = temp_dir("maintenance-auth-before-decode");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        let (_root, user) = seed_authenticated_user(&client, &session);
        client
            .authenticate_passphrase(&session, user, b"user-pass")
            .expect("auth user");

        let policy = client
            .store_maintenance_policy_set(&session, &malformed_cbor())
            .expect_err("policy denied before decode");
        assert_eq!(policy.code, Code::PermissionDenied);

        let run = client
            .store_maintenance_run(&session, &malformed_cbor())
            .expect_err("run denied before decode");
        assert_eq!(run.code, Code::PermissionDenied);

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn audit_compact_generated_legal_hold_noop_checkpoint_audit_and_reopen() {
        let dir = temp_dir("audit-compact-generated");
        let hold_path = dir.join("hold.loom");
        let hold = LocalLoomClient::new(&hold_path);
        hold.create().expect("create hold store");
        let hold_session = hold.open().expect("open hold");
        hold.with_session(&hold_session, |loom| {
            loom.store().save_audit_config_audited(
                loom_store::AuditConfig {
                    retention_days: 365,
                    legal_hold: true,
                },
                None,
                "audit.config.set",
                Some("legal_hold=true"),
            )?;
            Ok(())
        })
        .expect("set legal hold");
        assert_eq!(
            hold.audit_compact(&hold_session, 0)
                .expect_err("legal hold")
                .code,
            Code::PermissionDenied
        );
        hold.close(&hold_session);

        let path = dir.join("compact.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");

        let noop = cbor_array(
            &client
                .audit_compact(&session, 0)
                .expect("noop audit compact"),
        );
        assert_eq!(noop[0], loom_codec::Value::Uint(0));
        assert_eq!(noop[1], loom_codec::Value::Null);
        assert_eq!(noop[2], loom_codec::Value::Null);
        assert_eq!(noop[3], loom_codec::Value::Uint(0));

        client
            .with_session(&session, |loom| {
                loom.store().audit_append(None, "seed.one", None)?;
                loom.store().audit_append(None, "seed.two", None)?;
                Ok(())
            })
            .expect("seed audit");
        let compact = cbor_array(
            &client
                .audit_compact(&session, 2)
                .expect("checkpoint audit compact"),
        );
        assert_eq!(compact[0], loom_codec::Value::Uint(3));
        assert_eq!(compact[1], loom_codec::Value::Uint(2));
        assert!(matches!(compact[2], loom_codec::Value::Text(_)));
        assert_eq!(compact[3], loom_codec::Value::Uint(3));
        let records = client
            .with_session(&session, |loom| loom.store().audit_records())
            .expect("audit records");
        assert!(records.iter().any(|record| record.action == "audit.prune"));
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let reopened_session = reopened.open().expect("reopen");
        let reopened_records = reopened
            .with_session(&reopened_session, |loom| loom.store().audit_records())
            .expect("reopened audit");
        assert!(
            reopened_records
                .iter()
                .any(|record| record.action == "audit.prune")
        );
        reopened.close(&reopened_session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_maintenance_generated_status_controls_live_root_diagnostics() {
        let dir = temp_dir("maintenance-status-diagnostics");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");

        let without = loom_wire::store_admin::store_maintenance_status_request_to_cbor(
            &loom_wire::store_admin::StoreMaintenanceStatusRequest {
                include_live_root_diagnostics: false,
            },
        );
        let without = loom_wire::store_admin::store_maintenance_status_result_from_cbor(
            &client
                .store_maintenance_status(&session, &without)
                .expect("status without diagnostics"),
        )
        .expect("decode status without diagnostics");
        assert!(without.live_root_diagnostics.is_none());

        let with = loom_wire::store_admin::store_maintenance_status_request_to_cbor(
            &loom_wire::store_admin::StoreMaintenanceStatusRequest {
                include_live_root_diagnostics: true,
            },
        );
        let with = loom_wire::store_admin::store_maintenance_status_result_from_cbor(
            &client
                .store_maintenance_status(&session, &with)
                .expect("status with diagnostics"),
        )
        .expect("decode status with diagnostics");
        assert!(with.live_root_diagnostics.is_some());

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_maintenance_generated_policy_partial_update_validates_and_audits_atomically() {
        let dir = temp_dir("maintenance-policy-generated");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        let before = client
            .with_session(&session, |loom| loom.store().store_maintenance_policy())
            .expect("policy before");
        let update = loom_wire::store_admin::store_maintenance_policy_update_to_cbor(
            &loom_wire::store_admin::StoreMaintenancePolicyUpdate {
                max_pages: Some(77),
                tail_trim_enabled: Some(false),
                ..loom_wire::store_admin::StoreMaintenancePolicyUpdate {
                    min_candidate_pages: None,
                    min_reusable_pages: None,
                    interval_ms: None,
                    backoff_ms: None,
                    max_segments: None,
                    max_pages: None,
                    full_compaction_enabled: None,
                    tail_trim_enabled: None,
                    tail_compaction_enabled: None,
                    tail_compaction_max_pages: None,
                    tail_compaction_max_objects: None,
                    tail_compaction_max_bytes: None,
                    tail_compaction_interval_ms: None,
                    tail_compaction_backoff_ms: None,
                }
            },
        );
        client
            .store_maintenance_policy_set(&session, &update)
            .expect("partial policy update");
        let after = client
            .with_session(&session, |loom| loom.store().store_maintenance_policy())
            .expect("policy after");
        assert_eq!(after.max_pages, 77);
        assert!(!after.tail_trim_enabled);
        assert_eq!(after.min_candidate_pages, before.min_candidate_pages);
        assert_eq!(after.interval_ms, before.interval_ms);
        let audit = client
            .with_session(&session, |loom| loom.store().audit_records())
            .expect("audit");
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].action, "store.maintenance.policy.set");
        assert!(
            client
                .with_session(&session, |loom| {
                    loom.store().control_get(b"store.maintenance.policy.last")
                })
                .expect("synthetic key")
                .is_none()
        );

        let invalid = loom_wire::store_admin::store_maintenance_policy_update_to_cbor(
            &loom_wire::store_admin::StoreMaintenancePolicyUpdate {
                interval_ms: Some(0),
                ..loom_wire::store_admin::StoreMaintenancePolicyUpdate {
                    min_candidate_pages: None,
                    min_reusable_pages: None,
                    interval_ms: None,
                    backoff_ms: None,
                    max_segments: None,
                    max_pages: None,
                    full_compaction_enabled: None,
                    tail_trim_enabled: None,
                    tail_compaction_enabled: None,
                    tail_compaction_max_pages: None,
                    tail_compaction_max_objects: None,
                    tail_compaction_max_bytes: None,
                    tail_compaction_interval_ms: None,
                    tail_compaction_backoff_ms: None,
                }
            },
        );
        assert_eq!(
            client
                .store_maintenance_policy_set(&session, &invalid)
                .expect_err("invalid policy")
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            client
                .with_session(&session, |loom| loom.store().store_maintenance_policy())
                .expect("policy unchanged"),
            after
        );
        assert_eq!(
            client
                .with_session(&session, |loom| loom.store().audit_records())
                .expect("audit unchanged")
                .len(),
            1
        );
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_maintenance_generated_run_uses_injected_clock_and_decodes_typed_outcome() {
        let dir = temp_dir("maintenance-run-generated");
        let clock = Arc::new(ClientFixedMaintenanceClock {
            now_ms: 7_777,
            instant: std::time::Instant::now(),
        });
        let client = LocalLoomClient::with_store_maintenance_clock(dir.join("t.loom"), clock);
        client.create().expect("create store");
        let session = client.open().expect("open");

        let zero = loom_wire::store_admin::store_maintenance_run_request_to_cbor(
            &loom_wire::store_admin::StoreMaintenanceRunRequest {
                max_segments: Some(0),
                max_pages: None,
            },
        );
        assert_eq!(
            client
                .store_maintenance_run(&session, &zero)
                .expect_err("zero max segments")
                .code,
            Code::InvalidArgument
        );

        let request = loom_wire::store_admin::store_maintenance_run_request_to_cbor(
            &loom_wire::store_admin::StoreMaintenanceRunRequest {
                max_segments: Some(1),
                max_pages: Some(1),
            },
        );
        let outcome = loom_wire::store_admin::store_maintenance_run_result_from_cbor(
            &client
                .store_maintenance_run(&session, &request)
                .expect("maintenance run"),
        )
        .expect("decode maintenance run");
        assert!(matches!(
            outcome.kind,
            loom_wire::store_admin::StoreMaintenanceRunKind::Skipped
                | loom_wire::store_admin::StoreMaintenanceRunKind::Marked
                | loom_wire::store_admin::StoreMaintenanceRunKind::Compacted
                | loom_wire::store_admin::StoreMaintenanceRunKind::Reclaimed
        ));
        let state = client
            .with_session(&session, |loom| loom.store().store_maintenance_run_state())
            .expect("run state");
        assert_eq!(state.last_run_ms, Some(7_777));
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mu_6h_l_b_sql_exec_result_uses_existing_workspace_and_persists_dirty_state() {
        let dir = temp_dir("sql-exec-result");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        let workspace = client
            .workspace_create(&session, Some("repo"), None)
            .expect("workspace");

        let before = std::fs::read(&path).expect("read before");
        let readonly = client
            .sql_exec_result(&session, "repo", "db", "SELECT 1")
            .expect("readonly sql");
        assert_eq!(
            readonly,
            direct_sql_exec("SELECT 1").expect("direct select")
        );
        assert_eq!(std::fs::read(&path).expect("read after readonly"), before);
        let facets = client
            .with_session(&session, |loom| loom.registry().facets(workspace))
            .expect("facets after readonly");
        assert!(!facets.contains(&FacetKind::Sql));

        let create_sql =
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a')";
        let created = client
            .sql_exec_result(&session, "repo", "db", create_sql)
            .expect("create table");
        assert_eq!(created, direct_sql_exec(create_sql).expect("direct create"));
        let facets = client
            .with_session(&session, |loom| loom.registry().facets(workspace))
            .expect("facets after write");
        assert!(facets.contains(&FacetKind::Sql));

        client
            .sql_exec_result(
                &session,
                &workspace.to_string(),
                "db",
                "INSERT INTO t VALUES (2, 'b')",
            )
            .expect("uuid insert");
        let selected = client
            .sql_query_result(&session, "repo", "db", "SELECT id, v FROM t ORDER BY id")
            .expect("select live");
        assert!(!selected.is_empty());
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen");
        assert_eq!(
            reopened
                .sql_query_result(&session, "repo", "db", "SELECT id, v FROM t ORDER BY id")
                .expect("select reopened"),
            selected
        );
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mu_6h_l_b_sql_exec_result_rejects_dangling_transactions_and_preserves_state() {
        let dir = temp_dir("sql-exec-result-dangling");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("repo"), None)
            .expect("workspace");
        client
            .sql_exec_result(
                &session,
                "repo",
                "db",
                "CREATE TABLE t (id INTEGER PRIMARY KEY)",
            )
            .expect("create table");
        let before = client
            .sql_query_result(&session, "repo", "db", "SELECT * FROM t")
            .expect("before");
        let err = client
            .sql_exec_result(&session, "repo", "db", "BEGIN; INSERT INTO t VALUES (1)")
            .expect_err("dangling transaction");
        assert_eq!(err.code, Code::InvalidArgument);
        assert_eq!(
            client
                .sql_query_result(&session, "repo", "db", "SELECT * FROM t")
                .expect("after"),
            before
        );
        client.close(&session);
        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen");
        assert_eq!(
            reopened
                .sql_query_result(&session, "repo", "db", "SELECT * FROM t")
                .expect("after reopen"),
            before
        );
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mu_6h_l_b_sql_exec_result_publication_failure_preserves_state() {
        let dir = temp_dir("sql-exec-result-publication-failure");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("repo"), None)
            .expect("workspace");
        client
            .sql_exec_result(
                &session,
                "repo",
                "db",
                "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a')",
            )
            .expect("seed sql");
        let before_rows = client
            .sql_query_result(&session, "repo", "db", "SELECT id, v FROM t ORDER BY id")
            .expect("before rows");
        let before_file = std::fs::read(&path).expect("read before");
        install_exec_apply_candidate_save_hook(
            path.clone(),
            Box::new(|| {
                Err(LoomError::new(
                    Code::Internal,
                    "injected sql publication failure",
                ))
            }),
        );

        let err = client
            .sql_exec_result(&session, "repo", "db", "INSERT INTO t VALUES (2, 'b')")
            .expect_err("publication failure");
        assert_eq!(err.code, Code::Internal);
        assert_eq!(
            client
                .sql_query_result(&session, "repo", "db", "SELECT id, v FROM t ORDER BY id")
                .expect("live rows after failure"),
            before_rows
        );
        assert_eq!(
            std::fs::read(&path).expect("read after failure"),
            before_file
        );
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen");
        assert_eq!(
            reopened
                .sql_query_result(&session, "repo", "db", "SELECT id, v FROM t ORDER BY id")
                .expect("reopened rows after failure"),
            before_rows
        );
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mu_6h_l_b_sql_exec_result_authorizes_before_sql_parse_or_facet_creation() {
        let dir = temp_dir("sql-exec-result-auth");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        let workspace = client
            .workspace_create(&session, Some("repo"), None)
            .expect("workspace");
        let root = WorkspaceId::from_bytes([41; 16]);
        let user = WorkspaceId::from_bytes([42; 16]);
        client
            .with_session(&session, |loom| {
                let mut identity = IdentityStore::new(root);
                identity.set_passphrase(root, "root-pass", b"12345678")?;
                identity.add_principal(user, "user", PrincipalKind::User)?;
                identity.set_passphrase(user, "user-pass", b"abcdefgh")?;
                let mut acl = AclStore::new();
                acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])?;
                loom.store().save_identity_store(&identity)?;
                loom.store().save_acl_store(&acl)?;
                loom.set_identity_store(identity);
                loom.set_acl_store(acl);
                save_loom(loom)
            })
            .expect("seed auth");
        client
            .authenticate_passphrase(&session, user, b"user-pass")
            .expect("auth user");
        let err = client
            .sql_exec_result(&session, "repo", "db", "THIS IS NOT SQL")
            .expect_err("denied before parse");
        assert_eq!(err.code, Code::PermissionDenied);
        let facets = client
            .with_session(&session, |loom| loom.registry().facets(workspace))
            .expect("facets");
        assert!(!facets.contains(&FacetKind::Sql));

        client.clear_authentication(&session).expect("clear auth");
        client
            .authenticate_passphrase(&session, root, b"root-pass")
            .expect("auth root");
        let direct_error = direct_sql_exec("THIS IS NOT SQL").expect_err("direct sql error");
        let generated_error = client
            .sql_exec_result(&session, "repo", "db", "THIS IS NOT SQL")
            .expect_err("generated sql error");
        assert_eq!(generated_error.code, direct_error.code);
        assert_eq!(generated_error.message, direct_error.message);
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn exec_apply_request(workspace: &str, base: &str, fork: &str) -> Vec<u8> {
        loom_codec::encode(&loom_codec::Value::Map(vec![
            (
                loom_codec::Value::Text("workspace".to_string()),
                loom_codec::Value::Text(workspace.to_string()),
            ),
            (
                loom_codec::Value::Text("base".to_string()),
                loom_codec::Value::Text(base.to_string()),
            ),
            (
                loom_codec::Value::Text("fork".to_string()),
                loom_codec::Value::Text(fork.to_string()),
            ),
            (
                loom_codec::Value::Text("author".to_string()),
                loom_codec::Value::Text("alice".to_string()),
            ),
            (
                loom_codec::Value::Text("timestamp_ms".to_string()),
                loom_codec::Value::Uint(3_000),
            ),
        ]))
        .expect("encode exec apply request")
    }

    fn exec_apply_result_value(bytes: &[u8]) -> BTreeMap<String, loom_codec::Value> {
        let loom_codec::Value::Map(fields) =
            loom_codec::decode(bytes).expect("decode apply result")
        else {
            panic!("apply result must be a map");
        };
        fields
            .into_iter()
            .map(|(key, value)| {
                let loom_codec::Value::Text(key) = key else {
                    panic!("apply result keys must be text");
                };
                (key, value)
            })
            .collect()
    }

    fn seed_exec_apply_dirty_feature(client: &LocalLoomClient, session: &LoomSession) {
        client
            .write_file(session, "repo", "base.txt", b"base", 0)
            .expect("write base");
        client.stage_all(session, "repo").expect("stage base");
        client
            .commit(session, "repo", "alice", "base", 1_000)
            .expect("commit base");
        client
            .branch(session, "repo", "feature")
            .expect("create feature branch");
        client
            .checkout(session, "repo", "feature")
            .expect("checkout feature");
        client
            .write_file(session, "repo", "feature.txt", b"feature", 0)
            .expect("write feature");
        client.stage_all(session, "repo").expect("stage feature");
        client
            .commit(session, "repo", "alice", "feature", 2_000)
            .expect("commit feature");
        client
            .write_file(session, "repo", "staged.txt", b"staged", 0)
            .expect("write staged");
        client.stage_all(session, "repo").expect("stage dirty");
        client
            .write_file(session, "repo", "staged.txt", b"staged-modified", 0)
            .expect("modify staged");
        client
            .write_file(session, "repo", "untracked.txt", b"untracked", 0)
            .expect("write untracked");
    }

    fn exec_apply_snapshot(
        client: &LocalLoomClient,
        session: &LoomSession,
    ) -> ExecApplyWorkspaceSnapshot {
        ExecApplyWorkspaceSnapshot {
            head: client
                .vcs_head_branch(session, "repo")
                .expect("head branch"),
            main_log: client.log(session, "repo", "main").expect("main log"),
            feature_log: client.log(session, "repo", "feature").expect("feature log"),
            staged_file: client
                .read_file(session, "repo", "staged.txt")
                .expect("staged file"),
            status: client.status(session, "repo").expect("status"),
            merge_in_progress: client
                .merge_in_progress(session, "repo")
                .expect("merge state"),
        }
    }

    fn meetings_import_input(profile: &str, source_id: &str, title: &str) -> Vec<u8> {
        let source_digest =
            Digest::hash(Algo::Blake3, format!("{source_id}:{title}").as_bytes()).to_string();
        serde_json::to_vec(&serde_json::json!({
            "snapshot_version": 1,
            "profile": profile,
            "source_system": profile,
            "source_scope": "local-cache",
            "observed_at": 500,
            "coverage": "complete",
            "items": [{
                "source_entity_id": source_id,
                "source_digest": source_digest,
                "source_sidecar": {"id": source_id, "raw": true},
                "title": title,
                "summary_text": format!("{title} summary"),
                "transcript_spans": [{"text": format!("{title} transcript")}],
                "decisions": [{"label": format!("{title} decision")}]
            }]
        }))
        .expect("meetings import json")
    }

    fn meetings_report_value(report: &str) -> serde_json::Value {
        serde_json::from_str(report).expect("meetings report json")
    }

    fn meetings_report_field_names(report: &str) -> Vec<String> {
        let serde_json::Value::Object(fields) =
            serde_json::from_str(report).expect("meetings report json object")
        else {
            panic!("meetings report must be a json object");
        };
        fields.keys().cloned().collect()
    }

    fn meetings_snapshot_count(client: &LocalLoomClient, session: &LoomSession) -> usize {
        client
            .with_session(session, |loom| {
                Ok(loom
                    .store()
                    .control_scan_prefix(
                        loom_substrate::meetings::PROFILE_CONTROL_PREFIX.as_bytes(),
                    )?
                    .len())
            })
            .expect("meetings snapshot count")
    }

    #[derive(Debug, PartialEq, Eq)]
    struct MeetingsImportLiveSnapshot {
        bytes: Vec<u8>,
        len: u64,
        modified: SystemTime,
        workspace_present: bool,
        vcs_facet_present: bool,
        profile_controls: usize,
        checkpoint_controls: usize,
        revision_index_present: bool,
        audit_records: usize,
        mutable_overlay_generation: String,
        mutable_overlay_entries: usize,
    }

    fn meetings_import_live_snapshot(
        client: &LocalLoomClient,
        session: &LoomSession,
        path: &std::path::Path,
    ) -> MeetingsImportLiveSnapshot {
        let bytes = std::fs::read(path).expect("read store bytes");
        let metadata = std::fs::metadata(path).expect("read store metadata");
        client
            .with_session(session, |loom| {
                let workspace = loom.registry().open(&ns_selector_by_name("studio")).ok();
                let vcs_facet_present = workspace
                    .map(|id| loom.registry().has_facet(id, FacetKind::Vcs))
                    .transpose()?
                    .unwrap_or(false);
                let revision_index_present = workspace
                    .map(|id| {
                        load_optional_current_revision_index(loom, id, &id.to_string())
                            .map(|index| index.is_some())
                    })
                    .transpose()?
                    .unwrap_or(false);
                Ok(MeetingsImportLiveSnapshot {
                    bytes,
                    len: metadata.len(),
                    modified: metadata.modified().expect("store modified time"),
                    workspace_present: workspace.is_some(),
                    vcs_facet_present,
                    profile_controls: loom
                        .store()
                        .control_scan_prefix(
                            loom_substrate::meetings::PROFILE_CONTROL_PREFIX.as_bytes(),
                        )?
                        .len(),
                    checkpoint_controls: loom
                        .store()
                        .control_scan_prefix(b"interchange/checkpoints/meetings/")?
                        .len(),
                    revision_index_present,
                    audit_records: loom.store().audit_records()?.len(),
                    mutable_overlay_generation: format!(
                        "{:?}",
                        loom.store().mutable_overlay_generation()?
                    ),
                    mutable_overlay_entries: loom.store().mutable_overlay_current_entries()?.len(),
                })
            })
            .expect("capture meetings import snapshot")
    }

    fn assert_meetings_import_injected_failure_rolls_back(
        point: MeetingsImportTestHookPoint,
        tag: &str,
    ) {
        let dir = temp_dir(tag);
        let store_path = dir.join("t.loom");
        let client = LocalLoomClient::new(store_path.clone());
        client.create().expect("create store");
        let session = client.open().expect("open");
        let before = meetings_import_live_snapshot(&client, &session, &store_path);
        let input = meetings_import_input("granola-app", "note-1", "Planning");

        clear_meetings_import_test_hook();
        install_meetings_import_test_hook(
            store_path.clone(),
            Box::new(move |observed| {
                if observed == point {
                    Err(LoomError::new(
                        Code::Internal,
                        "injected meetings import failure",
                    ))
                } else {
                    Ok(())
                }
            }),
        );
        let err = client
            .meetings_import_snapshot(&session, "studio", "granola-app", &input, false)
            .expect_err("injected failure");
        clear_meetings_import_test_hook();
        assert_eq!(err.code, Code::Internal);
        assert_eq!(
            meetings_import_live_snapshot(&client, &session, &store_path),
            before
        );

        client.close(&session);
        let reopened = LocalLoomClient::new(store_path.clone());
        let reopened_session = reopened.open().expect("reopen after failure");
        assert_eq!(
            meetings_import_live_snapshot(&reopened, &reopened_session, &store_path),
            before
        );
        reopened.close(&reopened_session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn meetings_import_snapshot_imports_and_reopens() {
        let dir = temp_dir("meetings-import-generated-reopen");
        let store_path = dir.join("t.loom");
        let client = LocalLoomClient::new(store_path.clone());
        client.create().expect("create store");
        let session = client.open().expect("open");
        let input = meetings_import_input("granola-app", "note-1", "Planning");

        let report = client
            .meetings_import_snapshot(&session, "studio", "granola-app", &input, false)
            .expect("import meetings");
        let report = meetings_report_value(&report);
        assert_eq!(report["profile"], "meetings");
        assert_eq!(report["source_scope"], "local-cache");
        assert_eq!(report["rows_imported"], 1);
        assert_eq!(report["operations_applied"], report["operations_planned"]);
        assert_eq!(
            meetings_report_field_names(&serde_json::to_string(&report).expect("report json")),
            vec![
                "bytes_in",
                "bytes_stored",
                "commit",
                "dry_run",
                "fidelity_issues",
                "objects_added",
                "operations_applied",
                "operations_planned",
                "profile",
                "rows_imported",
                "skipped",
                "source_scope",
                "warnings"
            ]
        );

        let workspace = client
            .with_session(&session, |loom| {
                loom.registry().open(&ns_selector_by_name("studio"))
            })
            .expect("workspace");
        let profile_id = workspace.to_string();
        client
            .with_session(&session, |loom| {
                assert_eq!(
                    loom.store()
                        .control_scan_prefix(
                            loom_substrate::meetings::PROFILE_CONTROL_PREFIX.as_bytes()
                        )?
                        .len(),
                    1
                );
                assert_eq!(
                    loom.store()
                        .control_scan_prefix(b"interchange/checkpoints/meetings/")?
                        .len(),
                    1
                );
                let history = loom_substrate::versioning::load_current_revision_index(
                    loom,
                    workspace,
                    &profile_id,
                )?;
                assert_eq!(history.history("meeting:meeting/note-1").len(), 1);
                assert_eq!(history.checkpoints().len(), 1);
                assert_eq!(
                    loom.read_file_reserved(
                        workspace,
                        &loom_interchange_io::meetings_source_payload_path(
                            &profile_id,
                            "note-1",
                            "source.json"
                        ),
                    )?,
                    br#"{"id":"note-1","raw":true}"#
                );
                assert_eq!(
                    loom.read_file_reserved(
                        workspace,
                        &loom_interchange_io::meetings_source_payload_path(
                            &profile_id,
                            "note-1",
                            "summary.txt"
                        ),
                    )?,
                    b"Planning summary"
                );
                let actions = loom
                    .store()
                    .audit_records()?
                    .into_iter()
                    .map(|record| record.action)
                    .collect::<Vec<_>>();
                assert!(actions.contains(&"meetings.import".to_string()));
                assert!(actions.contains(&"meetings.import.checkpoint".to_string()));
                Ok(())
            })
            .expect("meetings durable conformance");
        client.close(&session);

        let reopened = LocalLoomClient::new(store_path.clone());
        let session = reopened.open().expect("reopen");
        reopened
            .with_session(&session, |loom| {
                let snapshot = load_meetings_snapshot(loom, &profile_id)?
                    .ok_or_else(|| LoomError::new(Code::NotFound, "meetings snapshot"))?;
                assert_eq!(snapshot.sources[0].source_id, "note-1");
                assert_eq!(snapshot.meetings[0].meeting_id, "meeting/note-1");
                assert_eq!(snapshot.annotations[0].kind, "Decision");
                Ok(())
            })
            .expect("read reopened meetings");
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn meetings_import_snapshot_injected_failures_leave_live_and_reopened_state_unchanged() {
        for (point, tag) in [
            (
                MeetingsImportTestHookPoint::AfterWorkspaceCreation,
                "meetings-import-fail-after-workspace",
            ),
            (
                MeetingsImportTestHookPoint::AfterPayloadRevisionPlanning,
                "meetings-import-fail-after-planning",
            ),
            (
                MeetingsImportTestHookPoint::BeforePublication,
                "meetings-import-fail-before-publication",
            ),
        ] {
            assert_meetings_import_injected_failure_rolls_back(point, tag);
        }
    }

    #[test]
    fn meetings_import_snapshot_dry_run_does_not_mutate_store_bytes() {
        let dir = temp_dir("meetings-import-generated-dry-run");
        let store_path = dir.join("t.loom");
        let client = LocalLoomClient::new(store_path.clone());
        client.create().expect("create store");
        let session = client.open().expect("open");
        let before = std::fs::read(&store_path).expect("read before");
        let before_meta = std::fs::metadata(&store_path).expect("metadata before");
        let input = meetings_import_input("generic", "source-a", "Planning");

        let report = client
            .meetings_import_snapshot(&session, "studio", "generic", &input, true)
            .expect("dry-run meetings");
        let report = meetings_report_value(&report);
        assert_eq!(report["dry_run"], true);
        assert_eq!(report["operations_applied"], 0);
        assert_eq!(meetings_snapshot_count(&client, &session), 0);
        let second_report = client
            .meetings_import_snapshot(&session, "studio", "generic", &input, true)
            .expect("second dry-run meetings");
        assert_eq!(
            second_report,
            serde_json::to_string(&report).expect("canonical dry-run report json")
        );
        assert_eq!(std::fs::read(&store_path).expect("read after"), before);
        let after_meta = std::fs::metadata(&store_path).expect("metadata after");
        assert_eq!(after_meta.len(), before_meta.len());
        assert_eq!(
            after_meta.modified().expect("after modified"),
            before_meta.modified().expect("before modified")
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn meetings_import_snapshot_retry_is_idempotent() {
        let dir = temp_dir("meetings-import-generated-idempotent");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        let input = meetings_import_input("granola-app", "note-1", "Planning");

        client
            .meetings_import_snapshot(&session, "studio", "granola-app", &input, false)
            .expect("first import");
        let retry = client
            .meetings_import_snapshot(&session, "studio", "granola-app", &input, false)
            .expect("retry import");
        let retry = meetings_report_value(&retry);
        assert_eq!(retry["operations_applied"], 0);
        assert!(
            retry["warnings"]
                .as_array()
                .expect("warnings")
                .iter()
                .any(|warning| warning == "meetings snapshot already current")
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn meetings_import_snapshot_rejects_malformed_json_and_profile_mismatch() {
        let dir = temp_dir("meetings-import-generated-invalid");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");

        let err = client
            .meetings_import_snapshot(&session, "studio", "generic", b"not json", false)
            .expect_err("malformed json");
        assert_eq!(err.code, Code::InvalidArgument);

        let mismatch = meetings_import_input("granola-app", "note-1", "Planning");
        let err = client
            .meetings_import_snapshot(&session, "studio", "generic", &mismatch, false)
            .expect_err("profile mismatch");
        assert_eq!(err.code, Code::InvalidArgument);

        let valid = meetings_import_input("generic", "note-1", "Planning");
        let err = client
            .meetings_import_snapshot(&session, "studio", "bad-profile", &valid, false)
            .expect_err("invalid profile");
        assert_eq!(err.code, Code::InvalidArgument);
        assert_eq!(meetings_snapshot_count(&client, &session), 0);

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn meetings_import_snapshot_validates_session_and_auth_before_input_parse() {
        let dir = temp_dir("meetings-import-generated-auth-order");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        seed_authenticated_non_admin(&client, &session);
        let unknown = unknown_session(&session);

        assert_eq!(
            client
                .meetings_import_snapshot(&unknown, "studio", "bad-profile", b"not json", false)
                .expect_err("unknown before parse")
                .code,
            Code::NotFound
        );
        assert_eq!(
            client
                .meetings_import_snapshot(&session, "studio", "bad-profile", b"not json", false)
                .expect_err("auth before parse")
                .code,
            Code::PermissionDenied
        );
        assert_eq!(meetings_snapshot_count(&client, &session), 0);

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn seed_refs_workspace(
        client: &LocalLoomClient,
        session: &LoomSession,
        workspace: &str,
        enqueue_candidate: bool,
    ) -> String {
        client
            .with_session(session, |loom| {
                let ns = loom.registry_mut().ensure_for_write(
                    &ns_selector(workspace, FacetKind::Vcs),
                    mint_workspace_id()?,
                )?;
                let workspace_id = ns.to_string();
                let project = loom_tickets::create_project(
                    loom,
                    ns,
                    &workspace_id,
                    "matrix",
                    "MX",
                    "Matrix",
                    None,
                )?;
                let fields = serde_json::json!({"status": "ready", "title": "Reference target"});
                let labels = Vec::<String>::new();
                loom_tickets::create_ticket(
                    loom,
                    ns,
                    loom_tickets::TicketCreateRequest {
                        workspace_id: &workspace_id,
                        project_id: "matrix",
                        ticket_type: "task",
                        external_source: None,
                        external_id: None,
                        fields: &fields,
                        policy_labels: &labels,
                        expected_root: Some(&project.profile_root),
                    },
                )?;
                if enqueue_candidate {
                    let candidate =
                        UnresolvedReference::new(loom_substrate::refs::UnresolvedReferenceInput {
                            candidate_id: "candidate-1".to_string(),
                            source: loom_substrate::refs::ReferenceSource::new(
                                "tickets",
                                &workspace_id,
                                "source-ticket",
                                "body",
                            )?,
                            source_operation_id: "operation:candidate-1".to_string(),
                            source_root: Digest::hash(Algo::Blake3, b"source-ticket"),
                            alias_text: "!ticket:MX-1".to_string(),
                            relation: "refers_to".to_string(),
                            span_start: 0,
                            span_end: 12,
                            evidence: "!ticket:MX-1".to_string(),
                            next_attempt_ms: 0,
                        })?;
                    loom_reference::enqueue(loom, ns, &candidate)?;
                }
                save_loom(loom)?;
                Ok(workspace_id)
            })
            .expect("seed refs workspace")
    }

    fn refs_reconcile_audit_targets(
        client: &LocalLoomClient,
        session: &LoomSession,
    ) -> Vec<String> {
        client
            .with_session(session, |loom| loom.store().audit_records())
            .expect("audit records")
            .into_iter()
            .filter(|record| record.action == "refs.reconcile")
            .map(|record| record.target.expect("refs target"))
            .collect()
    }

    fn seed_inference_workspace(
        client: &LocalLoomClient,
        session: &LoomSession,
        name: &str,
    ) -> WorkspaceId {
        client
            .workspace_create(session, Some(name), Some(FacetKind::Inference))
            .expect("create inference workspace")
    }

    fn inference_instance_exists(
        client: &LocalLoomClient,
        session: &LoomSession,
        workspace: &str,
        name: &str,
    ) -> bool {
        client
            .with_session(session, |loom| {
                let workspace_id = loom.registry().open(&ns_selector_by_name(workspace))?;
                let state = inference_instance_state(loom, workspace_id)?;
                Ok(state.find_instance(name).is_some())
            })
            .expect("read inference state")
    }

    fn audit_sequence(json: &str) -> u64 {
        serde_json::from_str::<serde_json::Value>(json)
            .expect("json")
            .get("audit-sequence")
            .and_then(serde_json::Value::as_u64)
            .expect("audit sequence")
    }

    fn unknown_session(session: &LoomSession) -> LoomSession {
        let mut handle = session.0.clone();
        handle.id = 999_999u64.to_be_bytes().to_vec();
        LoomSession(handle)
    }

    fn seed_authenticated_inference_non_admin(
        client: &LocalLoomClient,
        session: &LoomSession,
        workspace: &str,
    ) -> WorkspaceId {
        let workspace_id = seed_inference_workspace(client, session, workspace);
        let principal = mint_workspace_id().expect("principal");
        client
            .with_session(session, |loom| {
                let mut identity = IdentityStore::new(principal);
                identity.set_passphrase(principal, "pw", b"inference-salt")?;
                loom.store().save_identity_store(&identity)
            })
            .expect("seed identity");
        client
            .authenticate_passphrase(session, principal, b"pw")
            .expect("authenticate principal");
        workspace_id
    }

    fn seed_authenticated_non_admin(client: &LocalLoomClient, session: &LoomSession) {
        let principal = mint_workspace_id().expect("principal");
        client
            .with_session(session, |loom| {
                let mut identity = IdentityStore::new(principal);
                identity.set_passphrase(principal, "pw", b"serve-salt")?;
                loom.store().save_identity_store(&identity)
            })
            .expect("seed identity");
        client
            .authenticate_passphrase(session, principal, b"pw")
            .expect("authenticate principal");
    }

    fn serve_configure_request(bind: &str, tls: Option<&str>, network: Option<&str>) -> String {
        let mut value = serde_json::json!({
            "surface": "admin",
            "selectors": [],
            "bind": bind,
            "transport": "rest",
            "enabled": true,
            "auth_mode": "owner-or-passphrase",
            "exposure": "read-write",
            "audit_mode": "management-and-security",
            "request_size_limit": 4096,
            "idle_timeout_ms": 1000,
            "session_timeout_ms": 2000
        });
        if let Some(tls) = tls {
            value["tls_certificate_bundle"] = serde_json::Value::String(tls.to_string());
        }
        if let Some(network) = network {
            value["network_access_policy"] = serde_json::Value::String(network.to_string());
        }
        serde_json::to_string(&value).expect("request json")
    }

    fn served_listener_count(client: &LocalLoomClient, session: &LoomSession) -> usize {
        client
            .with_session(session, |loom| loom.store().served_listeners())
            .expect("served listeners")
            .len()
    }

    fn kv_namespace_exists(
        client: &LocalLoomClient,
        session: &LoomSession,
        workspace: &str,
    ) -> bool {
        client
            .with_session(session, |loom| read_ns(loom, workspace, FacetKind::Kv))
            .expect("kv namespace")
            .is_some()
    }

    fn overlay_test_key() -> loom_core::OverlayKey {
        loom_core::OverlayKey::from_segments([
            b"serve",
            b"listener",
            b"rollback",
            b"mutable",
            b"overlay",
            b"entry",
        ])
        .expect("overlay key")
    }

    fn overlay_payload(
        client: &LocalLoomClient,
        session: &LoomSession,
        key: &loom_core::OverlayKey,
    ) -> Option<Vec<u8>> {
        client
            .with_session(session, |loom| {
                Ok(loom
                    .mutable_overlay()
                    .current_entry(key)
                    .map(|entry| entry.payload))
            })
            .expect("overlay entry")
    }

    fn drive_policy_targets(
        client: &LocalLoomClient,
        session: &LoomSession,
    ) -> Vec<(WorkspaceId, String)> {
        client
            .with_session(session, |loom| {
                let registry = loom
                    .store()
                    .control_get(&drive_policy_registry_key())?
                    .map(|bytes| DrivePolicyRegistry::decode(&bytes))
                    .transpose()?
                    .unwrap_or_else(DrivePolicyRegistry::empty);
                Ok(registry
                    .enabled_targets()
                    .map(|target| (target.workspace, target.workspace_id.clone()))
                    .collect())
            })
            .expect("drive policy registry")
    }

    fn seed_drive_listener_workspace(
        client: &LocalLoomClient,
        session: &LoomSession,
        name: &str,
    ) -> WorkspaceId {
        let workspace = mint_workspace_id().expect("workspace");
        client
            .with_session(session, |loom| {
                loom.registry_mut()
                    .create(FacetKind::Files, Some(name), workspace)?;
                save_loom(loom)
            })
            .expect("seed drive workspace");
        workspace
    }

    fn seed_web_workspace(
        client: &LocalLoomClient,
        session: &LoomSession,
        name: &str,
    ) -> WorkspaceId {
        seed_drive_listener_workspace(client, session, name)
    }

    fn web_configure_request(workspace: &str, bind: &str) -> String {
        serde_json::json!({
            "surface": "web",
            "selectors": [workspace],
            "bind": bind,
            "transport": "rest",
            "enabled": true
        })
        .to_string()
    }

    fn web_route_set_request(
        listener: &str,
        route: &str,
        host: Option<&str>,
        prefix: &str,
        workspace: Option<&str>,
        root: &str,
    ) -> String {
        let mut value = serde_json::json!({
            "listener": listener,
            "route": route,
            "prefix": prefix,
            "root": root
        });
        if let Some(host) = host {
            value["host"] = serde_json::Value::String(host.to_string());
        }
        if let Some(workspace) = workspace {
            value["workspace"] = serde_json::Value::String(workspace.to_string());
        }
        value.to_string()
    }

    fn seed_web_listener(
        client: &LocalLoomClient,
        session: &LoomSession,
        workspace: &str,
        bind: &str,
    ) -> String {
        if !client
            .with_session(session, |loom| {
                Ok(read_ns(loom, workspace, FacetKind::Files)?.is_some())
            })
            .expect("check web workspace")
        {
            seed_web_workspace(client, session, workspace);
        }
        let configured: serde_json::Value = serde_json::from_str(
            &client
                .serve_listener_configure_json(session, &web_configure_request(workspace, bind))
                .expect("configure web listener"),
        )
        .expect("configured listener json");
        configured["id"].as_str().expect("listener id").to_string()
    }

    fn web_route_ids(
        client: &LocalLoomClient,
        session: &LoomSession,
        listener: &str,
    ) -> Vec<String> {
        client
            .with_session(session, |loom| {
                let key = crate::serve_config::web_listener_control_key(listener)?;
                let Some(bytes) = loom.store().control_get(&key)? else {
                    return Ok(Vec::new());
                };
                let web_listener = loom_substrate::web::WebListener::decode(&bytes)?;
                Ok(web_listener
                    .routes
                    .routes
                    .into_iter()
                    .map(|route| route.route_id)
                    .collect())
            })
            .expect("web routes")
    }

    fn audit_actions(client: &LocalLoomClient, session: &LoomSession) -> Vec<String> {
        client
            .with_session(session, |loom| loom.store().audit_records())
            .expect("audit records")
            .into_iter()
            .map(|record| record.action)
            .collect()
    }

    #[test]
    fn serve_listener_authenticates_and_authorizes_before_json_validation() {
        let dir = temp_dir("serve-listener-auth-order");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        let unknown = unknown_session(&session);

        assert_eq!(
            client
                .serve_listener_configure_json(&unknown, "{not json")
                .expect_err("unknown configure")
                .code,
            Code::NotFound
        );
        assert_eq!(
            client
                .serve_listener_list_json(&unknown)
                .expect_err("unknown list")
                .code,
            Code::NotFound
        );
        assert_eq!(
            client
                .serve_listener_set_enabled_json(&unknown, "bad listener", true)
                .expect_err("unknown set enabled")
                .code,
            Code::NotFound
        );
        assert_eq!(
            client
                .serve_listener_remove_json(&unknown, "bad listener")
                .expect_err("unknown remove")
                .code,
            Code::NotFound
        );

        let dir = temp_dir("serve-listener-auth-denied");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        seed_authenticated_non_admin(&client, &session);

        for error in [
            client
                .serve_listener_configure_json(&session, "{not json")
                .expect_err("denied configure"),
            client
                .serve_listener_list_json(&session)
                .expect_err("denied list"),
            client
                .serve_listener_set_enabled_json(&session, "bad listener", true)
                .expect_err("denied set enabled"),
            client
                .serve_listener_remove_json(&session, "bad listener")
                .expect_err("denied remove"),
        ] {
            assert_eq!(error.code, Code::PermissionDenied);
        }
        assert_eq!(served_listener_count(&client, &session), 0);
        let audit = client
            .with_session(&session, |loom| loom.store().audit_records())
            .expect("audit records");
        assert!(audit.is_empty());
    }

    #[test]
    fn serve_listener_rejects_missing_references_without_state_or_audit() {
        let dir = temp_dir("serve-listener-missing-references");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");

        let missing_cert = serve_configure_request("127.0.0.1:18081", Some("missing-cert"), None);
        assert_eq!(
            client
                .serve_listener_configure_json(&session, &missing_cert)
                .expect_err("missing cert")
                .code,
            Code::NotFound
        );
        assert_eq!(served_listener_count(&client, &session), 0);

        let missing_policy =
            serve_configure_request("127.0.0.1:18082", None, Some("missing-policy"));
        assert_eq!(
            client
                .serve_listener_configure_json(&session, &missing_policy)
                .expect_err("missing policy")
                .code,
            Code::NotFound
        );
        assert_eq!(served_listener_count(&client, &session), 0);
        let audit = client
            .with_session(&session, |loom| loom.store().audit_records())
            .expect("audit records");
        assert!(audit.is_empty());
    }

    #[test]
    fn serve_listener_accepts_complete_shared_surface_registry_entry() {
        let dir = temp_dir("serve-listener-files-surface");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");

        let request = serde_json::json!({
            "surface": "files",
            "selectors": ["docs"],
            "bind": "127.0.0.1:18084",
            "transport": "rest",
            "enabled": true
        })
        .to_string();
        let configured: serde_json::Value = serde_json::from_str(
            &client
                .serve_listener_configure_json(&session, &request)
                .expect("configure files listener"),
        )
        .expect("configured json");
        assert_eq!(configured["surface"], "files");
        assert_eq!(configured["selectors"][0], "docs");
        let id = configured["id"].as_str().expect("listener id").to_string();

        client.close(&session);
        let reopened = client.open().expect("reopen");
        let reopened_list: serde_json::Value = serde_json::from_str(
            &client
                .serve_listener_list_json(&reopened)
                .expect("reopen list"),
        )
        .expect("reopen list json");
        assert_eq!(reopened_list["listeners"][0]["id"], id);
    }

    #[test]
    fn serve_listener_memcached_publication_failure_rolls_back_state_and_audit() {
        let dir = temp_dir("serve-listener-memcached-rollback");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");

        let request = serde_json::json!({
            "surface": "memcached",
            "selectors": ["work", "sessions"],
            "bind": "127.0.0.1:18085",
            "transport": "text",
            "mode": "write-through",
            "enabled": true
        })
        .to_string();
        let overlay_key = overlay_test_key();
        client
            .with_session(&session, |loom| {
                loom.mutable_overlay_mut().put_value(
                    overlay_key.clone(),
                    None,
                    b"live overlay before failure".to_vec(),
                )?;
                Ok(())
            })
            .expect("seed mutable overlay");
        let err = client
            .serve_listener_configure_json_with_commit(
                &session,
                &request,
                |_, _, _, _, _, _, _, _| {
                    Err(LoomError::new(
                        Code::Internal,
                        "injected served listener publication failure",
                    ))
                },
            )
            .expect_err("injected failure");
        assert_eq!(err.code, Code::Internal);
        assert_eq!(served_listener_count(&client, &session), 0);
        assert!(!kv_namespace_exists(&client, &session, "work"));
        assert_eq!(
            overlay_payload(&client, &session, &overlay_key).as_deref(),
            Some(b"live overlay before failure".as_slice())
        );
        let audit = client
            .with_session(&session, |loom| loom.store().audit_records())
            .expect("audit records");
        assert!(audit.is_empty());

        client.close(&session);
        let reopened = client.open().expect("reopen");
        assert_eq!(served_listener_count(&client, &reopened), 0);
        assert!(!kv_namespace_exists(&client, &reopened, "work"));
        let reopened_audit = client
            .with_session(&reopened, |loom| loom.store().audit_records())
            .expect("reopened audit");
        assert!(reopened_audit.is_empty());
    }

    #[test]
    fn serve_listener_drive_publishes_listener_policy_and_audits_atomically() {
        let dir = temp_dir("serve-listener-drive-atomic");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        let workspace = seed_drive_listener_workspace(&client, &session, "drive");

        let request = serde_json::json!({
            "surface": "drive",
            "selectors": ["drive"],
            "bind": "127.0.0.1:18086",
            "transport": "rest",
            "enabled": true
        })
        .to_string();
        let configured: serde_json::Value = serde_json::from_str(
            &client
                .serve_listener_configure_json(&session, &request)
                .expect("configure drive listener"),
        )
        .expect("configured json");
        let id = configured["id"].as_str().expect("listener id").to_string();
        assert_eq!(
            drive_policy_targets(&client, &session),
            vec![(workspace, workspace.to_string())]
        );
        let audit = client
            .with_session(&session, |loom| loom.store().audit_records())
            .expect("audit records");
        let actions = audit
            .iter()
            .map(|record| record.action.as_str())
            .collect::<Vec<_>>();
        assert!(actions.contains(&"serve.listener.configure"));
        assert!(actions.contains(&"drive.policy_registry.configure"));

        client.close(&session);
        let reopened = client.open().expect("reopen");
        assert_eq!(
            drive_policy_targets(&client, &reopened),
            vec![(workspace, workspace.to_string())]
        );
        let reopened_list: serde_json::Value = serde_json::from_str(
            &client
                .serve_listener_list_json(&reopened)
                .expect("reopen list"),
        )
        .expect("reopen list json");
        assert_eq!(reopened_list["listeners"][0]["id"], id);
        let reopened_audit = client
            .with_session(&reopened, |loom| loom.store().audit_records())
            .expect("reopened audit");
        let reopened_actions = reopened_audit
            .iter()
            .map(|record| record.action.as_str())
            .collect::<Vec<_>>();
        assert!(reopened_actions.contains(&"serve.listener.configure"));
        assert!(reopened_actions.contains(&"drive.policy_registry.configure"));
    }

    #[test]
    fn serve_listener_drive_publication_failure_leaves_policy_listener_and_audit_absent() {
        let dir = temp_dir("serve-listener-drive-rollback");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        seed_drive_listener_workspace(&client, &session, "drive");

        let request = serde_json::json!({
            "surface": "drive",
            "selectors": ["drive"],
            "bind": "127.0.0.1:18087",
            "transport": "rest",
            "enabled": true
        })
        .to_string();
        let err = client
            .serve_listener_configure_json_with_commit(
                &session,
                &request,
                |_, _, _, _, _, _, _, _| {
                    Err(LoomError::new(
                        Code::Internal,
                        "injected drive listener publication failure",
                    ))
                },
            )
            .expect_err("injected failure");
        assert_eq!(err.code, Code::Internal);
        assert_eq!(served_listener_count(&client, &session), 0);
        assert!(drive_policy_targets(&client, &session).is_empty());
        let audit = client
            .with_session(&session, |loom| loom.store().audit_records())
            .expect("audit records");
        assert!(audit.is_empty());

        client.close(&session);
        let reopened = client.open().expect("reopen");
        assert_eq!(served_listener_count(&client, &reopened), 0);
        assert!(drive_policy_targets(&client, &reopened).is_empty());
        let reopened_audit = client
            .with_session(&reopened, |loom| loom.store().audit_records())
            .expect("reopened audit");
        assert!(reopened_audit.is_empty());
    }

    #[test]
    fn serve_listener_configure_list_enable_disable_remove_and_reopen() {
        let dir = temp_dir("serve-listener-authority");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .certificate_generate_self_signed_json(
                &session,
                "admin",
                vec!["localhost".to_string()],
                Vec::new(),
                None,
                1,
                "p256",
                true,
            )
            .expect("generate cert");
        let rules = r#"[{"id":"local","action":"allow","source_cidr":"127.0.0.1/32","require_mtls":false}]"#;
        client
            .network_access_set_json(&session, "local", None, "deny", rules)
            .expect("network policy");

        let configured_json = client
            .serve_listener_configure_json(
                &session,
                &serve_configure_request("127.0.0.1:18083", Some("admin"), Some("local")),
            )
            .expect("configure listener");
        let configured: serde_json::Value =
            serde_json::from_str(&configured_json).expect("configured json");
        let id = configured["id"].as_str().expect("listener id").to_string();
        assert_eq!(configured["tls"]["mode"], "direct");
        assert_eq!(configured["tls"]["certificate_bundle_ref"], "admin");
        assert_eq!(configured["network_access_policy_ref"], "local");
        assert_eq!(configured["limits"]["request_size_limit"], 4096);

        let disabled: serde_json::Value = serde_json::from_str(
            &client
                .serve_listener_set_enabled_json(&session, &id, false)
                .expect("disable"),
        )
        .expect("disable json");
        assert_eq!(disabled["enabled"], false);
        let enabled: serde_json::Value = serde_json::from_str(
            &client
                .serve_listener_set_enabled_json(&session, &id, true)
                .expect("enable"),
        )
        .expect("enable json");
        assert_eq!(enabled["enabled"], true);

        let list: serde_json::Value =
            serde_json::from_str(&client.serve_listener_list_json(&session).expect("list"))
                .expect("list json");
        assert_eq!(list["listeners"].as_array().expect("listeners").len(), 1);
        assert_eq!(list["listeners"][0]["id"], id);

        client.close(&session);
        let reopened = client.open().expect("reopen");
        let reopened_list: serde_json::Value = serde_json::from_str(
            &client
                .serve_listener_list_json(&reopened)
                .expect("reopen list"),
        )
        .expect("reopen list json");
        assert_eq!(reopened_list["listeners"][0]["id"], id);

        let removed: serde_json::Value = serde_json::from_str(
            &client
                .serve_listener_remove_json(&reopened, &id)
                .expect("remove"),
        )
        .expect("remove json");
        assert_eq!(removed["id"], id);

        let audit = client
            .with_session(&reopened, |loom| loom.store().audit_records())
            .expect("audit records");
        let actions = audit
            .iter()
            .map(|record| record.action.as_str())
            .collect::<Vec<_>>();
        assert!(actions.contains(&"serve.listener.configure"));
        assert!(actions.contains(&"serve.listener.disable"));
        assert!(actions.contains(&"serve.listener.enable"));
        assert!(actions.contains(&"serve.listener.list"));
        assert!(actions.contains(&"serve.listener.remove"));
    }

    #[test]
    fn serve_web_route_authenticates_and_authorizes_before_payload_validation() {
        let dir = temp_dir("serve-web-route-auth-order");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        let unknown = unknown_session(&session);

        assert_eq!(
            client
                .serve_web_route_set_json(&unknown, "{not json")
                .expect_err("unknown set")
                .code,
            Code::NotFound
        );
        assert_eq!(
            client
                .serve_web_route_list_json(&unknown, "bad listener")
                .expect_err("unknown list")
                .code,
            Code::NotFound
        );
        assert_eq!(
            client
                .serve_web_route_remove_json(&unknown, "bad listener", "bad route")
                .expect_err("unknown remove")
                .code,
            Code::NotFound
        );

        let dir = temp_dir("serve-web-route-auth-denied");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        seed_authenticated_non_admin(&client, &session);

        for error in [
            client
                .serve_web_route_set_json(&session, "{not json")
                .expect_err("denied set"),
            client
                .serve_web_route_list_json(&session, "bad listener")
                .expect_err("denied list"),
            client
                .serve_web_route_remove_json(&session, "bad listener", "bad route")
                .expect_err("denied remove"),
        ] {
            assert_eq!(error.code, Code::PermissionDenied);
        }
        assert!(audit_actions(&client, &session).is_empty());
    }

    #[test]
    fn serve_web_route_rejects_invalid_inputs_without_route_state_or_audit() {
        let dir = temp_dir("serve-web-route-invalid-inputs");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        seed_web_workspace(&client, &session, "site");
        let admin: serde_json::Value = serde_json::from_str(
            &client
                .serve_listener_configure_json(
                    &session,
                    &serve_configure_request("127.0.0.1:18088", None, None),
                )
                .expect("configure admin listener"),
        )
        .expect("admin listener json");
        let admin_id = admin["id"].as_str().expect("admin id").to_string();
        let web_id = seed_web_listener(&client, &session, "site", "127.0.0.1:18089");

        assert_eq!(
            client
                .serve_web_route_set_json(
                    &session,
                    &web_route_set_request(&admin_id, "docs", None, "/docs", None, "/docs")
                )
                .expect_err("non-web listener")
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            client
                .serve_web_route_set_json(
                    &session,
                    &web_route_set_request("missing", "docs", None, "/docs", None, "/docs")
                )
                .expect_err("missing listener")
                .code,
            Code::NotFound
        );
        let relative_prefix: serde_json::Value = serde_json::from_str(
            &client
                .serve_web_route_set_json(
                    &session,
                    &web_route_set_request(&web_id, "docs", None, "docs", None, "/docs"),
                )
                .expect("relative prefix is canonicalized"),
        )
        .expect("relative prefix json");
        assert_eq!(relative_prefix["routes"][0]["path_prefix"], "/docs");
        assert_eq!(web_route_ids(&client, &session, &web_id), vec!["docs"]);
        client
            .serve_web_route_remove_json(&session, &web_id, "docs")
            .expect("remove canonicalized route");
        let baseline_actions = audit_actions(&client, &session);
        assert_eq!(
            client
                .serve_web_route_set_json(
                    &session,
                    &web_route_set_request(&web_id, "docs", None, "/docs", None, "/../secret")
                )
                .expect_err("invalid root")
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            client
                .serve_web_route_set_json(
                    &session,
                    &web_route_set_request(
                        &web_id,
                        "docs",
                        None,
                        "/docs",
                        Some("missing"),
                        "/docs"
                    )
                )
                .expect_err("missing workspace")
                .code,
            Code::NotFound
        );
        assert_eq!(
            client
                .serve_web_route_set_json(
                    &session,
                    &web_route_set_request(&web_id, "docs", None, "", None, "/docs")
                )
                .expect_err("empty prefix")
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            client
                .serve_web_route_set_json(
                    &session,
                    &web_route_set_request(&web_id, "docs", None, "/bad\u{0}", None, "/docs")
                )
                .expect_err("forbidden prefix character")
                .code,
            Code::InvalidArgument
        );
        assert!(web_route_ids(&client, &session, &web_id).is_empty());
        assert_eq!(audit_actions(&client, &session), baseline_actions);
    }

    #[test]
    fn serve_web_route_set_list_remove_are_ordered_audited_and_reopened() {
        let dir = temp_dir("serve-web-route-authority");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        let workspace = seed_web_workspace(&client, &session, "site");
        let id = seed_web_listener(&client, &session, "site", "127.0.0.1:18090");

        let b_json = client
            .serve_web_route_set_json(
                &session,
                &web_route_set_request(
                    &id,
                    "route-b",
                    Some("example.test"),
                    "/b",
                    Some("site"),
                    "/content/b",
                ),
            )
            .expect("set route b");
        let b: serde_json::Value = serde_json::from_str(&b_json).expect("route b json");
        assert_eq!(b["routes"][0]["route_id"], "route-b");
        assert_eq!(b["routes"][0]["workspace"], workspace.to_string());
        client
            .serve_web_route_set_json(
                &session,
                &web_route_set_request(&id, "route-a", None, "/a", None, "/content/a"),
            )
            .expect("set route a");

        let listed: serde_json::Value = serde_json::from_str(
            &client
                .serve_web_route_list_json(&session, &id)
                .expect("list routes"),
        )
        .expect("route list json");
        let routes = listed["routes"].as_array().expect("routes");
        assert_eq!(routes[0]["route_id"], "route-a");
        assert_eq!(routes[1]["route_id"], "route-b");

        client.close(&session);
        let reopened = client.open().expect("reopen");
        assert_eq!(
            web_route_ids(&client, &reopened, &id),
            vec!["route-a".to_string(), "route-b".to_string()]
        );
        let removed: serde_json::Value = serde_json::from_str(
            &client
                .serve_web_route_remove_json(&reopened, &id, "route-b")
                .expect("remove route b"),
        )
        .expect("remove json");
        assert_eq!(removed["routes"].as_array().expect("routes").len(), 1);
        assert_eq!(web_route_ids(&client, &reopened, &id), vec!["route-a"]);
        let actions = audit_actions(&client, &reopened);
        assert!(actions.contains(&"serve.web.route.set".to_string()));
        assert!(actions.contains(&"serve.web.route.list".to_string()));
        assert!(actions.contains(&"serve.web.route.remove".to_string()));
    }

    #[test]
    fn serve_web_route_publication_failure_preserves_route_table_and_audit() {
        let dir = temp_dir("serve-web-route-rollback");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        let id = seed_web_listener(&client, &session, "site", "127.0.0.1:18091");
        client
            .serve_web_route_set_json(
                &session,
                &web_route_set_request(&id, "stable", None, "/stable", None, "/stable"),
            )
            .expect("set stable route");
        let baseline_routes = web_route_ids(&client, &session, &id);
        let baseline_actions = audit_actions(&client, &session);

        let err = client
            .serve_web_route_set_json_with_commit(
                &session,
                &web_route_set_request(&id, "new", None, "/new", None, "/new"),
                |_, _, _, _, _, _| {
                    Err(LoomError::new(Code::Internal, "injected route set failure"))
                },
            )
            .expect_err("set failure");
        assert_eq!(err.code, Code::Internal);
        assert_eq!(web_route_ids(&client, &session, &id), baseline_routes);
        assert_eq!(audit_actions(&client, &session), baseline_actions);

        let err = client
            .serve_web_route_remove_json_with_commit(&session, &id, "stable", |_, _, _, _, _, _| {
                Err(LoomError::new(
                    Code::Internal,
                    "injected route remove failure",
                ))
            })
            .expect_err("remove failure");
        assert_eq!(err.code, Code::Internal);
        assert_eq!(web_route_ids(&client, &session, &id), baseline_routes);
        assert_eq!(audit_actions(&client, &session), baseline_actions);

        client.close(&session);
        let reopened = client.open().expect("reopen");
        assert_eq!(web_route_ids(&client, &reopened, &id), baseline_routes);
        assert_eq!(audit_actions(&client, &reopened), baseline_actions);
    }

    #[test]
    fn inference_instance_authenticates_and_authorizes_before_payload_validation() {
        let dir = temp_dir("inference-instance-auth-order");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        seed_inference_workspace(&client, &session, "ai");
        let unknown = unknown_session(&session);

        assert_eq!(
            client
                .inference_instance_create_json(
                    &unknown,
                    "ai",
                    "bad name".to_string(),
                    "model".to_string(),
                    "bad-kind".to_string(),
                    "bad-runtime".to_string(),
                    Some("bad preset".to_string()),
                    r#"{"bad.key":"1"}"#,
                )
                .expect_err("unknown create")
                .code,
            Code::NotFound
        );
        assert_eq!(
            client
                .inference_instance_update_json(
                    &unknown,
                    "ai",
                    "bad name".to_string(),
                    Some("bad preset".to_string()),
                    r#"{"bad.key":"1"}"#,
                )
                .expect_err("unknown update")
                .code,
            Code::NotFound
        );
        assert_eq!(
            client
                .inference_instance_delete_json(&unknown, "ai", "bad name".to_string())
                .expect_err("unknown delete")
                .code,
            Code::NotFound
        );
        client.close(&session);

        let dir = temp_dir("inference-instance-auth-denied");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        seed_authenticated_inference_non_admin(&client, &session, "ai");

        for error in [
            client
                .inference_instance_create_json(
                    &session,
                    "ai",
                    "bad name".to_string(),
                    "model".to_string(),
                    "bad-kind".to_string(),
                    "bad-runtime".to_string(),
                    Some("bad preset".to_string()),
                    r#"{"bad.key":"1"}"#,
                )
                .expect_err("denied create"),
            client
                .inference_instance_update_json(
                    &session,
                    "ai",
                    "bad name".to_string(),
                    Some("bad preset".to_string()),
                    r#"{"bad.key":"1"}"#,
                )
                .expect_err("denied update"),
            client
                .inference_instance_delete_json(&session, "ai", "bad name".to_string())
                .expect_err("denied delete"),
        ] {
            assert_eq!(error.code, Code::PermissionDenied);
        }
        assert!(!inference_instance_exists(
            &client, &session, "ai", "bad name"
        ));
        let audit = client
            .with_session(&session, |loom| loom.store().audit_records())
            .expect("audit records");
        assert!(audit.is_empty());
    }

    #[test]
    fn inference_instance_mutations_publish_state_and_one_audit_atomically() {
        let dir = temp_dir("inference-instance-audit");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        let workspace_id = seed_inference_workspace(&client, &session, "ai");

        let create = client
            .inference_instance_create_json(
                &session,
                "ai",
                "embed".to_string(),
                "BAAI/bge-small-en-v1.5".to_string(),
                "text-embedding".to_string(),
                "candle-safetensors".to_string(),
                None,
                "{}",
            )
            .expect("create instance");
        assert_eq!(audit_sequence(&create), 0);
        assert!(inference_instance_exists(&client, &session, "ai", "embed"));

        let update = client
            .inference_instance_update_json(
                &session,
                "ai",
                "embed".to_string(),
                Some("balanced".to_string()),
                "{}",
            )
            .expect("update instance");
        assert_eq!(audit_sequence(&update), 1);

        let delete = client
            .inference_instance_delete_json(&session, "ai", "embed".to_string())
            .expect("delete instance");
        assert_eq!(audit_sequence(&delete), 2);

        let audit = client
            .with_session(&session, |loom| loom.store().audit_records())
            .expect("audit records");
        assert_eq!(audit.len(), 3);
        assert_eq!(audit[0].seq, 0);
        assert_eq!(audit[0].action, "inference.instance.create");
        assert_eq!(
            audit[0].target.as_deref(),
            Some(format!("workspace={workspace_id};instance=embed").as_str())
        );
        assert_eq!(audit[1].seq, 1);
        assert_eq!(audit[1].action, "inference.instance.update");
        assert_eq!(audit[2].seq, 2);
        assert_eq!(audit[2].action, "inference.instance.delete");
        client.close(&session);

        let reopened = client.open().expect("reopen");
        assert!(!inference_instance_exists(
            &client, &reopened, "ai", "embed"
        ));
        let reopened_audit = client
            .with_session(&reopened, |loom| loom.store().audit_records())
            .expect("reopened audit");
        assert_eq!(reopened_audit.len(), 3);
        assert_eq!(reopened_audit[2].action, "inference.instance.delete");
    }

    #[test]
    fn inference_instance_generated_reads_preserve_filters_refs_and_not_found() {
        let dir = temp_dir("inference-instance-generated-reads");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        seed_inference_workspace(&client, &session, "ai");

        client
            .inference_instance_create_json(
                &session,
                "ai",
                "embed".to_string(),
                "embedding-model".to_string(),
                "text-embedding".to_string(),
                "candle-safetensors".to_string(),
                None,
                r#"{}"#,
            )
            .expect("create embedding instance");
        client
            .inference_instance_create_json(
                &session,
                "ai",
                "chat".to_string(),
                "chat-model".to_string(),
                "llm".to_string(),
                "candle-safetensors".to_string(),
                Some("balanced".to_string()),
                r#"{"temperature":"0.2"}"#,
            )
            .expect("create llm instance");

        let listed = client
            .inference_instance_list_json(&session, "ai", None)
            .expect("list instances");
        let listed: Vec<serde_json::Value> = serde_json::from_str(&listed).expect("list json");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0]["instance"]["name"], "chat");
        assert_eq!(listed[0]["refs"], 0);
        assert_eq!(listed[1]["instance"]["name"], "embed");

        let filtered = client
            .inference_instance_list_json(&session, "ai", Some("text-embedding".to_string()))
            .expect("filtered instances");
        let filtered: Vec<serde_json::Value> =
            serde_json::from_str(&filtered).expect("filtered json");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0]["instance"]["name"], "embed");

        let shown: serde_json::Value = serde_json::from_str(
            &client
                .inference_instance_get_json(&session, "ai", "chat")
                .expect("get instance"),
        )
        .expect("get json");
        assert_eq!(shown["instance"]["name"], "chat");
        assert_eq!(shown["refs"], 0);

        let missing = client
            .inference_instance_get_json(&session, "ai", "missing")
            .expect_err("missing instance");
        assert_eq!(missing.code, Code::NotFound);
    }

    #[test]
    fn inference_instance_commit_failure_rolls_back_engine_and_store() {
        let dir = temp_dir("inference-instance-commit-rollback");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        seed_inference_workspace(&client, &session, "ai");

        let failed = client.inference_instance_mutate_audited_with_commit(
            &session,
            "ai",
            "embed",
            "inference.instance.create",
            |state| {
                let instance = loom_inference::build_instance_descriptor(
                    "embed",
                    InferenceModelKind::TextEmbedding,
                    loom_types::ModelRef::new(
                        InferenceModelKind::TextEmbedding,
                        "BAAI/bge-small-en-v1.5",
                    ),
                    loom_types::RuntimeKind::CandleSafetensors,
                    None,
                    BTreeMap::new(),
                )?;
                state.upsert_instance(instance.clone());
                inference_instance_view_json(state, &instance)
            },
            |_, _, _| Err(LoomError::new(Code::Internal, "injected commit failure")),
        );
        assert_eq!(failed.unwrap_err().code, Code::Internal);
        assert!(!inference_instance_exists(&client, &session, "ai", "embed"));
        let audit = client
            .with_session(&session, |loom| loom.store().audit_records())
            .expect("audit records");
        assert!(audit.is_empty());
        client.close(&session);

        let reopened = client.open().expect("reopen");
        assert!(!inference_instance_exists(
            &client, &reopened, "ai", "embed"
        ));
        let reopened_audit = client
            .with_session(&reopened, |loom| loom.store().audit_records())
            .expect("reopened audit");
        assert!(reopened_audit.is_empty());
    }

    #[test]
    fn refs_reconcile_changed_batch_persists_one_audit_with_state() {
        let dir = temp_dir("refs-reconcile-audit");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        let workspace_id = seed_refs_workspace(&client, &session, "repo", true);

        let result: loom_reference::ReconciliationSummary =
            serde_json::from_str(&client.refs_reconcile_json(&session, "repo", 1).unwrap())
                .expect("summary json");
        assert_eq!(result.processed, 1);
        assert_eq!(result.resolved, 1);
        assert_eq!(result.failed, 0);
        assert_eq!(result.pending, 0);
        assert_eq!(
            refs_reconcile_audit_targets(&client, &session),
            vec![format!(
                "workspace={workspace_id};processed=1;resolved=1;failed=0;pending=0"
            )]
        );

        client.close(&session);
        let reopened = client.open().expect("reopen");
        let settled: loom_reference::ReconciliationSummary =
            serde_json::from_str(&client.refs_reconcile_json(&reopened, "repo", 1).unwrap())
                .expect("summary json");
        assert_eq!(settled.processed, 0);
        assert_eq!(settled.resolved, 1);
        assert_eq!(
            refs_reconcile_audit_targets(&client, &reopened),
            vec![format!(
                "workspace={workspace_id};processed=1;resolved=1;failed=0;pending=0"
            )]
        );
        client.close(&reopened);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refs_reconcile_noop_batches_emit_no_audit() {
        let dir = temp_dir("refs-reconcile-noop");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        seed_refs_workspace(&client, &session, "repo", true);

        let max_zero: loom_reference::ReconciliationSummary =
            serde_json::from_str(&client.refs_reconcile_json(&session, "repo", 0).unwrap())
                .expect("summary json");
        assert_eq!(max_zero.processed, 0);
        assert_eq!(max_zero.pending, 1);
        assert!(refs_reconcile_audit_targets(&client, &session).is_empty());

        client.close(&session);
        let empty_session = client.open().expect("reopen");
        seed_refs_workspace(&client, &empty_session, "empty", false);
        let no_candidate: loom_reference::ReconciliationSummary = serde_json::from_str(
            &client
                .refs_reconcile_json(&empty_session, "empty", 1)
                .unwrap(),
        )
        .expect("summary json");
        assert_eq!(no_candidate.processed, 0);
        assert!(refs_reconcile_audit_targets(&client, &empty_session).is_empty());
        client.close(&empty_session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_admin_policy_and_stat_round_trip() {
        let dir = temp_dir("storeadmin-policy");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");

        let update = loom_wire::store_admin::StorePolicyUpdate {
            fips_required: Some(true),
            default_durability: Some(OverlayDurabilityPolicy::Strict),
            facet_durability_assignments: vec![
                loom_wire::store_admin::StoreFacetDurabilityAssignment {
                    facet: FacetKind::Document,
                    durability: OverlayDurabilityPolicy::Relaxed,
                },
            ],
            clear_facet_durability: Vec::new(),
        };
        let update = loom_wire::store_admin::store_policy_update_to_cbor(&update);

        // policy set is audited (returns a seq); policy get echoes the value with no seq.
        let set = loom_wire::store_admin::store_policy_result_from_cbor(
            &client
                .store_policy_set(&session, &update)
                .expect("policy set"),
        )
        .expect("decode set");
        assert!(set.fips_required);
        assert_eq!(set.default_durability, OverlayDurabilityPolicy::Strict);
        assert_eq!(
            set.facet_durability_overrides,
            vec![loom_wire::store_admin::StoreFacetDurabilityAssignment {
                facet: FacetKind::Document,
                durability: OverlayDurabilityPolicy::Relaxed,
            }]
        );
        assert!(set.audit_seq.is_some(), "policy_set is audited");

        let get = loom_wire::store_admin::store_policy_result_from_cbor(
            &client.store_policy_get(&session).expect("policy get"),
        )
        .expect("decode get");
        assert!(get.fips_required);
        assert_eq!(get.default_durability, OverlayDurabilityPolicy::Strict);
        assert_eq!(
            get.facet_durability_overrides,
            set.facet_durability_overrides
        );
        assert!(get.audit_seq.is_none(), "policy_get carries no audit seq");

        // stat decodes as the canonical maintenance snapshot.
        let _stat = loom_wire::store_admin::store_stat_from_cbor(
            &client.store_stat(&session).expect("stat"),
        )
        .expect("decode stat");

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_admin_fails_closed_without_global_admin_grant() {
        use loom_core::identity::IdentityStore;
        let dir = temp_dir("storeadmin-acl");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");

        let root = mint_workspace_id().expect("root id");
        client
            .with_session(&session, |loom| {
                let mut identity = IdentityStore::new(root);
                identity.set_passphrase(root, "rootpw", b"root-salt-bytes")?;
                loom.store().save_identity_store(&identity)
            })
            .expect("seed identity");
        client
            .authenticate_passphrase(&session, root, b"rootpw")
            .expect("authenticate root");

        // Authenticated but without a global-admin grant: every StoreAdmin method fails closed.
        assert!(
            client.store_policy_get(&session).is_err(),
            "policy_get must fail closed without global admin"
        );
        assert!(
            client
                .store_policy_set(
                    &session,
                    &loom_wire::store_admin::store_policy_update_to_cbor(
                        &loom_wire::store_admin::StorePolicyUpdate {
                            fips_required: Some(true),
                            default_durability: None,
                            facet_durability_assignments: Vec::new(),
                            clear_facet_durability: Vec::new(),
                        },
                    ),
                )
                .is_err(),
            "policy_set must fail closed without global admin"
        );
        assert!(
            client.store_stat(&session).is_err(),
            "stat must fail closed without global admin"
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_admin_succeeds_for_authenticated_global_admin() {
        use loom_core::identity::IdentityStore;
        let dir = temp_dir("storeadmin-admin");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");

        let root = mint_workspace_id().expect("root id");
        // Seed the authenticated identity AND the global-admin grant, then close so the grant is
        // persisted; a freshly opened session loads the granted ACL into its engine.
        client
            .with_session(&session, |loom| {
                let mut identity = IdentityStore::new(root);
                identity.set_passphrase(root, "rootpw", b"root-salt-bytes")?;
                loom.store().save_identity_store(&identity)?;
                let mut acl = loom.store().acl_store()?.unwrap_or_default();
                acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])?;
                loom.store().save_acl_store(&acl)
            })
            .expect("seed identity + global admin grant");
        client.close(&session);

        let admin = client.open().expect("reopen with granted acl");
        client
            .authenticate_passphrase(&admin, root, b"rootpw")
            .expect("authenticate root");

        // An authenticated global admin can read stat/policy and set policy (audited under the actor).
        let _stat = client.store_stat(&admin).expect("stat as admin");
        let set = loom_wire::store_admin::store_policy_result_from_cbor(
            &client
                .store_policy_set(
                    &admin,
                    &loom_wire::store_admin::store_policy_update_to_cbor(
                        &loom_wire::store_admin::StorePolicyUpdate {
                            fips_required: Some(true),
                            default_durability: None,
                            facet_durability_assignments: Vec::new(),
                            clear_facet_durability: Vec::new(),
                        },
                    ),
                )
                .expect("policy set as admin"),
        )
        .expect("decode set");
        assert!(set.fips_required && set.audit_seq.is_some());
        assert!(client.store_policy_get(&admin).is_ok());

        client.close(&admin);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_rekey_rejects_unencrypted_store() {
        let dir = temp_dir("storeadmin-rekey");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        // A fresh store is unencrypted; rekey requires an encrypted store. (The server generates the
        // salt/nonce/DEK; here they are placeholders since the op rejects before using them.)
        let err = client
            .store_rekey(
                &session,
                &loom_wire::store_admin::store_rekey_request_to_cbor(
                    &loom_wire::store_admin::StoreRekeyRequest {
                        credential: loom_wire::store_admin::StoreRekeyCredential::Passphrase(
                            b"newpw".to_vec(),
                        ),
                        reseal: false,
                        suite: None,
                    },
                ),
                vec![0u8; 16],
                vec![0u8; 24],
                None,
            )
            .unwrap_err();
        assert_eq!(err.code, Code::Unsupported);
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn transfer_import_round_trips_export_bytes_with_finalize_once() {
        let dir = temp_dir("transfer");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");

        // Seed a file in workspace "src" and export it as a tar payload (server produces the bytes).
        client
            .write_file(&session, "src", "hello.txt", b"hello transfer", 0o644)
            .expect("write");
        client
            .commit(&session, "src", "test", "promote transfer source", 1)
            .expect("promote transfer source");
        let payload = client
            .transfer_export_bytes(&session, "src", "tar", None, &[])
            .expect("export tar");
        assert!(!payload.is_empty());
        let digest = Digest::hash(Algo::Blake3, &payload);

        // Import the payload into a fresh workspace "dst" via open -> write -> finish.
        let id = client
            .transfer_import_open(&session, "dst", "tar", &[])
            .expect("open transfer");
        let accept = client
            .transfer_import_write(&session, &id, &payload, 0, None)
            .expect("write");
        let (accepted, _credit) =
            loom_wire::transfer::transfer_accept_from_cbor(&accept).expect("decode accept");
        assert_eq!(accepted, payload.len() as u64);
        // Replayed write of an already-accepted seq is a no-op with unchanged counters.
        let accept2 = client
            .transfer_import_write(&session, &id, &payload, 0, None)
            .expect("replay write");
        assert_eq!(
            loom_wire::transfer::transfer_accept_from_cbor(&accept2)
                .unwrap()
                .0,
            payload.len() as u64
        );

        let report = client
            .transfer_import_finish(&session, &id, true, false, &digest)
            .expect("finish");
        assert!(!report.is_empty());
        // Finalize-once: a replayed finish returns the same cached report.
        let report2 = client
            .transfer_import_finish(&session, &id, true, false, &digest)
            .expect("finish replay");
        assert_eq!(report, report2);

        // The imported file is present in "dst".
        let content = client
            .read_file(&session, "dst", "hello.txt")
            .expect("read imported");
        assert_eq!(content, b"hello transfer");

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn transfer_import_rejects_bad_final_digest_and_supports_cancel() {
        let dir = temp_dir("transfer-bad");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .write_file(&session, "src", "a.txt", b"data", 0o644)
            .expect("write");
        let payload = client
            .transfer_export_bytes(&session, "src", "tar", None, &[])
            .expect("export");

        let id = client
            .transfer_import_open(&session, "dst", "tar", &[])
            .expect("open");
        client
            .transfer_import_write(&session, &id, &payload, 0, None)
            .expect("write");
        let bad = Digest::hash(Algo::Blake3, b"not the payload");
        let err = client
            .transfer_import_finish(&session, &id, true, false, &bad)
            .unwrap_err();
        assert_eq!(err.code, Code::IntegrityFailure);

        // Cancel releases the handle; a subsequent write is NotFound.
        client
            .transfer_import_cancel(&session, &id)
            .expect("cancel");
        let err = client
            .transfer_import_write(&session, &id, &payload, 1, None)
            .unwrap_err();
        assert_eq!(err.code, Code::NotFound);

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn transfer_open_rejects_unsupported_kinds() {
        let dir = temp_dir("transfer-kind");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open");
        assert_eq!(
            client
                .transfer_import_open(&session, "w", "parquet", &[])
                .unwrap_err()
                .code,
            Code::Unsupported
        );
        assert_eq!(
            client
                .transfer_import_open(&session, "w", "fs-tree", &[])
                .unwrap_err()
                .code,
            Code::Unsupported
        );
        assert_eq!(
            client
                .transfer_import_open(&session, "w", "no-such-kind", &[])
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_open_save_close_roundtrips() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);

        client.create().expect("create store");
        let session = client.open().expect("open session");
        assert_eq!(client.session_count(), 1);

        client.save(&session).expect("save session");
        assert!(client.close(&session), "first close reports open");
        assert_eq!(client.session_count(), 0);
        assert!(!client.close(&session), "second close is idempotent");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cas_put_get_roundtrips_through_the_engine() {
        let dir = temp_dir("cas");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open session");

        let digest = client
            .cas_put(&session, "blobs", b"hello world")
            .expect("cas put");
        let got = client.cas_get(&session, "blobs", &digest).expect("cas get");
        assert_eq!(got.as_deref(), Some(&b"hello world"[..]));

        assert!(client.close(&session));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn workspace_group_roundtrips() {
        let dir = temp_dir("ns");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        client
            .workspace_create(&session, Some("alpha"), Some(FacetKind::Kv))
            .expect("create alpha");
        client
            .workspace_create(&session, Some("beta"), None)
            .expect("create beta");

        let names: Vec<String> = client
            .workspace_list(&session)
            .expect("list")
            .into_iter()
            .map(|n| n.name)
            .collect();
        assert!(names.contains(&"alpha".to_string()));
        assert!(names.contains(&"beta".to_string()));

        client
            .workspace_rename(&session, "alpha", "alpha2")
            .expect("rename");
        client.workspace_delete(&session, "beta").expect("delete");

        let after: Vec<String> = client
            .workspace_list(&session)
            .expect("list after")
            .into_iter()
            .map(|n| n.name)
            .collect();
        assert!(after.contains(&"alpha2".to_string()));
        assert!(!after.contains(&"alpha".to_string()));
        assert!(!after.contains(&"beta".to_string()));

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn metadata_session_lists_workspaces_without_materializing_then_materializes_on_use() {
        let dir = temp_dir("metadata-session");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        client
            .workspace_create(&session, Some("alpha"), Some(FacetKind::Files))
            .expect("create alpha");
        client
            .write_file(&session, "alpha", "a.txt", b"a", 0o100644)
            .expect("write file");
        client.save(&session).expect("save");
        client.close(&session);

        let metadata = client.open_metadata().expect("metadata open");
        let names: Vec<String> = client
            .workspace_list(&metadata)
            .expect("metadata list")
            .into_iter()
            .map(|info| info.name)
            .collect();
        assert!(names.contains(&"alpha".to_string()));
        client
            .with_metadata_session(&metadata, |loom| {
                assert!(loom.is_state_lazy());
                Ok(())
            })
            .expect("inspect lazy state");

        assert_eq!(
            client
                .read_file(&metadata, "alpha", "a.txt")
                .expect("read after materialization"),
            b"a".to_vec()
        );
        client
            .with_metadata_session(&metadata, |loom| {
                assert!(!loom.is_state_lazy());
                Ok(())
            })
            .expect("inspect materialized state");

        client.close(&metadata);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ledger_group_roundtrips_through_the_engine() {
        let dir = temp_dir("ledger");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        // Absent ledger reads as empty.
        assert_eq!(client.ledger_len(&session, "audit", "log").expect("len"), 0);
        assert_eq!(
            client.ledger_get(&session, "audit", "log", 0).expect("get"),
            None
        );
        client
            .ledger_verify(&session, "audit", "log")
            .expect("empty verify");

        let seq0 = client
            .ledger_append(&session, "audit", "log", b"first")
            .expect("append 0");
        let seq1 = client
            .ledger_append(&session, "audit", "log", b"second")
            .expect("append 1");
        assert_eq!(seq0, 0);
        assert_eq!(seq1, 1);
        assert_eq!(client.ledger_len(&session, "audit", "log").expect("len"), 2);
        assert_eq!(
            client
                .ledger_get(&session, "audit", "log", 0)
                .expect("get 0")
                .as_deref(),
            Some(&b"first"[..])
        );
        assert!(
            client
                .ledger_head(&session, "audit", "log")
                .expect("head")
                .is_some()
        );
        client
            .ledger_verify(&session, "audit", "log")
            .expect("verify chain");

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn timeseries_group_roundtrips_through_the_engine() {
        let dir = temp_dir("ts");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        assert_eq!(
            client
                .ts_get(&session, "metrics", "cpu", 100)
                .expect("get absent"),
            None
        );
        assert_eq!(
            client
                .ts_latest(&session, "metrics", "cpu")
                .expect("latest absent"),
            None
        );

        client
            .ts_put(&session, "metrics", "cpu", 100, b"a")
            .expect("put 100");
        client
            .ts_put(&session, "metrics", "cpu", 200, b"b")
            .expect("put 200");

        assert_eq!(
            client
                .ts_get(&session, "metrics", "cpu", 100)
                .expect("get 100")
                .as_deref(),
            Some(&b"a"[..])
        );
        assert_eq!(
            client
                .ts_latest(&session, "metrics", "cpu")
                .expect("latest"),
            Some((200, b"b".to_vec()))
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn queue_group_roundtrips_through_the_engine() {
        let dir = temp_dir("queue");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        assert_eq!(
            client
                .queue_len(&session, "jobs", "in")
                .expect("len absent"),
            0
        );
        assert_eq!(
            client
                .queue_get(&session, "jobs", "in", 0)
                .expect("get absent"),
            None
        );

        assert_eq!(
            client
                .queue_append(&session, "jobs", "in", b"a")
                .expect("append a"),
            0
        );
        assert_eq!(
            client
                .queue_append(&session, "jobs", "in", b"b")
                .expect("append b"),
            1
        );
        assert_eq!(client.queue_len(&session, "jobs", "in").expect("len"), 2);
        assert_eq!(
            client
                .queue_get(&session, "jobs", "in", 1)
                .expect("get 1")
                .as_deref(),
            Some(&b"b"[..])
        );
        assert_eq!(
            client
                .queue_range(&session, "jobs", "in", 0, 2)
                .expect("range")
                .len(),
            2
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn consumer_group_roundtrips_through_the_engine() {
        let dir = temp_dir("consumer");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        client
            .queue_append(&session, "jobs", "in", b"a")
            .expect("append a");
        client
            .queue_append(&session, "jobs", "in", b"b")
            .expect("append b");

        assert_eq!(
            client
                .consumer_position(&session, "jobs", "in", "w1")
                .expect("pos"),
            0
        );
        assert_eq!(
            client
                .consumer_read(&session, "jobs", "in", "w1", 10)
                .expect("read")
                .len(),
            2
        );

        client
            .consumer_advance(&session, "jobs", "in", "w1", 1)
            .expect("advance");
        assert_eq!(
            client
                .consumer_position(&session, "jobs", "in", "w1")
                .expect("pos2"),
            1
        );
        let after = client
            .consumer_read(&session, "jobs", "in", "w1", 10)
            .expect("read2");
        assert_eq!(after, vec![b"b".to_vec()]);

        client
            .consumer_reset(&session, "jobs", "in", "w1", 0)
            .expect("reset");
        assert_eq!(
            client
                .consumer_position(&session, "jobs", "in", "w1")
                .expect("pos3"),
            0
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn document_group_roundtrips_through_the_engine() {
        let dir = temp_dir("doc");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        assert_eq!(
            client
                .document_get_binary(&session, "app", "users", "a")
                .expect("get absent"),
            None
        );
        assert!(
            !client
                .document_delete(&session, "app", "users", "a")
                .expect("delete absent")
        );

        client
            .document_put_binary(&session, "app", "users", "a", br#"{"n":1}"#, None)
            .expect("put");
        let document = client
            .document_get_binary(&session, "app", "users", "a")
            .expect("get")
            .expect("document");
        let (bytes, _, _) = loom_wire::document::binary_result_from_cbor(&document).unwrap();
        assert_eq!(bytes, br#"{"n":1}"#);
        assert!(
            !client
                .document_list_binary(&session, "app", "users")
                .expect("list")
                .is_empty()
        );
        assert!(
            client
                .document_delete(&session, "app", "users", "a")
                .expect("delete")
        );
        assert_eq!(
            client
                .document_get_binary(&session, "app", "users", "a")
                .expect("get gone"),
            None
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn filesystem_group_roundtrips_through_the_engine() {
        let dir = temp_dir("fs");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        client
            .write_file(&session, "tree", "notes.txt", b"hello", 0)
            .expect("write");
        assert_eq!(
            client
                .read_file(&session, "tree", "notes.txt")
                .expect("read"),
            b"hello".to_vec()
        );

        client
            .remove_file(&session, "tree", "notes.txt")
            .expect("remove");
        assert!(matches!(
            client.read_file(&session, "tree", "notes.txt"),
            Err(e) if e.code == Code::NotFound
        ));

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn graph_group_roundtrips_through_the_engine() {
        let dir = temp_dir("graph");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        client
            .graph_upsert_node(&session, "social", "g", "a", Props::new())
            .expect("node a");
        client
            .graph_upsert_node(&session, "social", "g", "b", Props::new())
            .expect("node b");
        client
            .graph_upsert_edge(
                &session,
                "social",
                "g",
                "e1",
                "a",
                "b",
                "knows",
                Props::new(),
            )
            .expect("edge");

        assert!(
            client
                .graph_get_node(&session, "social", "g", "a")
                .expect("get a")
                .is_some()
        );
        assert_eq!(
            client
                .graph_get_node(&session, "social", "g", "z")
                .expect("get z"),
            None
        );
        assert_eq!(
            client
                .graph_neighbors(&session, "social", "g", "a")
                .expect("neighbors"),
            vec!["b".to_string()]
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn kv_and_management_group_roundtrips_through_the_engine() {
        let dir = temp_dir("kv");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        let key = kv::key_to_cbor(&Value::Text("k1".to_string()));
        assert_eq!(
            client
                .kv_get(&session, "store", "m", &key)
                .expect("get absent"),
            None
        );

        client
            .kv_put(&session, "store", "m", &key, b"v1")
            .expect("put");
        assert_eq!(
            client
                .kv_get(&session, "store", "m", &key)
                .expect("get")
                .as_deref(),
            Some(&b"v1"[..])
        );
        assert!(
            client
                .kv_delete(&session, "store", "m", &key)
                .expect("delete")
        );
        assert_eq!(
            client
                .kv_get(&session, "store", "m", &key)
                .expect("get gone"),
            None
        );

        client
            .set_config(&session, "store", "m", KvMapConfig::VERSIONED)
            .expect("set config");
        assert_eq!(
            client
                .get_config(&session, "store", "m")
                .expect("get config"),
            KvMapConfig::VERSIONED
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn search_absent_paths_are_empty() {
        let dir = temp_dir("search");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        assert_eq!(
            client
                .search_get(&session, "idx", "c", b"x")
                .expect("get absent"),
            None
        );
        assert!(
            !client
                .search_delete(&session, "idx", "c", b"x")
                .expect("delete absent")
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn trigger_absent_paths_are_empty() {
        let dir = temp_dir("trigger");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        let id = WorkspaceId::v4_from_bytes([0u8; 16]);
        assert_eq!(
            client.trigger_list(&session, "prog").expect("list"),
            Vec::<Vec<u8>>::new()
        );
        assert_eq!(client.trigger_get(&session, "prog", id).expect("get"), None);
        assert!(!client.trigger_remove(&session, "prog", id).expect("remove"));

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn columnar_group_roundtrips_through_the_engine() {
        let dir = temp_dir("columnar");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        let columns = vec![
            ("label".to_string(), ColumnType::Text),
            ("count".to_string(), ColumnType::Int),
        ];
        client
            .columnar_create(&session, "analytics", "events", columns.clone(), 1024)
            .expect("create dataset");
        client
            .columnar_append(
                &session,
                "analytics",
                "events",
                vec![Value::Text("a".into()), Value::Int(1)],
            )
            .expect("append 1");
        client
            .columnar_append(
                &session,
                "analytics",
                "events",
                vec![Value::Text("b".into()), Value::Int(2)],
            )
            .expect("append 2");

        assert_eq!(
            client
                .columnar_rows(&session, "analytics", "events")
                .expect("rows"),
            2
        );
        assert_eq!(
            client
                .columnar_columns(&session, "analytics", "events")
                .expect("columns"),
            columns
        );
        assert_eq!(
            client
                .columnar_scan(&session, "analytics", "events")
                .expect("scan")
                .len(),
            2
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn vector_group_roundtrips_through_the_engine() {
        let dir = temp_dir("vector");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        client
            .vector_create(&session, "emb", "vs", 2, Metric::Cosine)
            .expect("create set");
        client
            .vector_upsert(&session, "emb", "vs", "a", vec![1.0, 0.0], BTreeMap::new())
            .expect("upsert a");
        client
            .vector_upsert(&session, "emb", "vs", "b", vec![0.0, 1.0], BTreeMap::new())
            .expect("upsert b");

        assert!(
            client
                .vector_get(&session, "emb", "vs", "a")
                .expect("get a")
                .is_some()
        );
        assert_eq!(
            client
                .vector_ids(&session, "emb", "vs", None)
                .expect("ids")
                .len(),
            2
        );
        let hits = client
            .vector_search(&session, "emb", "vs", &[1.0, 0.0], 2, &MetaFilter::All)
            .expect("search");
        assert_eq!(hits.len(), 2);
        assert!(
            client
                .vector_delete(&session, "emb", "vs", "a")
                .expect("delete")
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn vcs_group_roundtrips_through_the_engine() {
        let dir = temp_dir("vcs");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        client
            .write_file(&session, "repo", "f.txt", b"hi", 0)
            .expect("write file");
        client.stage_all(&session, "repo").expect("stage all");
        let _commit = client
            .commit(&session, "repo", "alice", "init", 1_000)
            .expect("commit");

        assert_eq!(client.log(&session, "repo", "main").expect("log").len(), 1);
        assert!(client.tag_list(&session, "repo").expect("tags").is_empty());
        let _status = client.status(&session, "repo").expect("status");

        client.branch(&session, "repo", "dev").expect("branch");
        client.checkout(&session, "repo", "dev").expect("checkout");

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn calendar_group_roundtrips_through_the_engine() {
        let dir = temp_dir("calendar");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        let meta = calendar::CollectionMeta {
            display_name: "Work".to_string(),
            component_set: Vec::new(),
        };
        client
            .calendar_create_collection(&session, "pim", "alice", "cal", &meta.encode())
            .expect("create collection");

        let entry = calendar::CalendarEntry::event("u1", "Standup", "20240101T090000");
        client
            .calendar_put_entry(&session, "pim", "alice", "cal", &entry.encode())
            .expect("put entry");

        assert!(
            client
                .calendar_get_entry(&session, "pim", "alice", "cal", "u1")
                .expect("get entry")
                .is_some()
        );
        assert_eq!(
            client
                .calendar_list_entries(&session, "pim", "alice", "cal")
                .expect("list")
                .len(),
            1
        );
        assert!(
            client
                .calendar_delete_entry(&session, "pim", "alice", "cal", "u1")
                .expect("delete")
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn contacts_group_roundtrips_through_the_engine() {
        let dir = temp_dir("contacts");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        let meta = contacts::BookMeta {
            display_name: "Friends".to_string(),
        };
        client
            .contacts_create_book(&session, "pim", "alice", "book", &meta.encode())
            .expect("create book");

        let entry = contacts::ContactEntry::new("u1", "Bob");
        client
            .contacts_put_entry(&session, "pim", "alice", "book", &entry.encode())
            .expect("put entry");

        assert!(
            client
                .contacts_get_entry(&session, "pim", "alice", "book", "u1")
                .expect("get")
                .is_some()
        );
        assert_eq!(
            client
                .contacts_list_entries(&session, "pim", "alice", "book")
                .expect("list")
                .len(),
            1
        );
        assert!(
            client
                .contacts_delete_entry(&session, "pim", "alice", "book", "u1")
                .expect("delete")
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mail_group_roundtrips_through_the_engine() {
        let dir = temp_dir("mail");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        let meta = mail::MailboxMeta {
            display_name: "Inbox".to_string(),
        };
        client
            .mail_create_mailbox(&session, "pim", "alice", "inbox", &meta.encode())
            .expect("create mailbox");

        let raw = b"From: a@b.com\r\nSubject: Hi\r\n\r\nHello body\r\n";
        client
            .mail_ingest_message(&session, "pim", "alice", "inbox", "m1", raw)
            .expect("ingest");

        assert_eq!(
            client
                .mail_to_eml(&session, "pim", "alice", "inbox", "m1")
                .expect("to_eml"),
            Some(raw.to_vec())
        );
        assert!(
            client
                .mail_get_message(&session, "pim", "alice", "inbox", "m1")
                .expect("get")
                .is_some()
        );
        assert_eq!(
            client
                .mail_list_messages(&session, "pim", "alice", "inbox")
                .expect("list")
                .len(),
            1
        );
        assert!(
            client
                .mail_delete_message(&session, "pim", "alice", "inbox", "m1")
                .expect("delete")
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dataframe_absent_frame_errors() {
        let dir = temp_dir("dataframe");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        // No frame created: reads error rather than returning empty.
        assert!(client.dataframe_plan_digest(&session, "df", "x").is_err());
        assert!(client.dataframe_collect(&session, "df", "x").is_err());

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exec_rejects_a_malformed_request() {
        let dir = temp_dir("exec");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        assert!(
            client
                .exec_cbor(&session, b"not a valid exec request")
                .is_err()
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exec_apply_validates_session_before_decoding_request() {
        let dir = temp_dir("exec-apply-session");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let unknown = LoomSession(HandleId {
            kind: "session".to_string(),
            id: 999_u64.to_be_bytes().to_vec(),
            generation: 0,
            owner_session: Vec::new(),
        });

        let err = client
            .apply_cbor(&unknown, b"not a valid apply request")
            .expect_err("unknown session wins before request decode");
        assert_eq!(err.code, Code::NotFound);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exec_apply_rejects_malformed_request_and_invalid_revisions() {
        let dir = temp_dir("exec-apply-invalid");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        let err = client
            .apply_cbor(&session, b"not cbor")
            .expect_err("malformed apply request");
        assert_eq!(err.code, Code::InvalidArgument);

        let request = exec_apply_request("missing-repo", "main", "feature");
        let err = client
            .apply_cbor(&session, &request)
            .expect_err("missing workspace");
        assert_eq!(err.code, Code::NotFound);

        client
            .write_file(&session, "repo", "a.txt", b"base", 0)
            .expect("write base");
        client.stage_all(&session, "repo").expect("stage base");
        client
            .commit(&session, "repo", "alice", "base", 1_000)
            .expect("commit base");
        let request = exec_apply_request("repo", "main", "missing-branch");
        let err = client
            .apply_cbor(&session, &request)
            .expect_err("missing branch");
        assert_eq!(err.code, Code::NotFound);

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exec_apply_cbor_fast_forward_persists() {
        let dir = temp_dir("exec-apply-fast-forward");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open session");

        client
            .write_file(&session, "repo", "a.txt", b"base", 0)
            .expect("write base");
        client.stage_all(&session, "repo").expect("stage base");
        client
            .commit(&session, "repo", "alice", "base", 1_000)
            .expect("commit base");
        client
            .branch(&session, "repo", "feature")
            .expect("create feature branch");
        client
            .checkout(&session, "repo", "feature")
            .expect("checkout feature");
        client
            .write_file(&session, "repo", "b.txt", b"feature", 0)
            .expect("write feature");
        client.stage_all(&session, "repo").expect("stage feature");
        client
            .commit(&session, "repo", "alice", "feature", 2_000)
            .expect("commit feature");

        let request = exec_apply_request("repo", "main", "feature");
        let result = client
            .apply_cbor(&session, &request)
            .expect("apply generated owner");
        let loom_codec::Value::Map(result_fields) =
            loom_codec::decode(&result).expect("decode apply result")
        else {
            panic!("apply result must be a map");
        };
        assert!(result_fields.iter().any(|(key, value)| {
            matches!(
                (key, value),
                (loom_codec::Value::Text(key), loom_codec::Value::Text(value))
                    if key == "schema" && value == "loom.exec.apply.result.v1"
            )
        }));
        assert!(result_fields.iter().any(|(key, value)| {
            matches!(
                (key, value),
                (loom_codec::Value::Text(key), loom_codec::Value::Text(value))
                    if key == "outcome" && value == "fast_forward"
            )
        }));
        assert!(result_fields.iter().any(|(key, value)| {
            matches!(
                (key, value),
                (loom_codec::Value::Text(key), loom_codec::Value::Bytes(bytes))
                    if key == "digest" && bytes.len() == 32
            )
        }));
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen store");
        assert_eq!(
            reopened
                .log(&session, "repo", "main")
                .expect("main log")
                .len(),
            2
        );
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exec_apply_conflict_result_is_deterministic_and_does_not_move_branch_tip() {
        let dir = temp_dir("exec-apply-conflict");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        client
            .write_file(&session, "repo", "shared.txt", b"base", 0)
            .expect("write base");
        client.stage_all(&session, "repo").expect("stage base");
        client
            .commit(&session, "repo", "alice", "base", 1_000)
            .expect("commit base");
        client
            .branch(&session, "repo", "feature")
            .expect("create feature branch");
        client
            .checkout(&session, "repo", "feature")
            .expect("checkout feature");
        client
            .write_file(&session, "repo", "shared.txt", b"feature", 0)
            .expect("write feature");
        client.stage_all(&session, "repo").expect("stage feature");
        client
            .commit(&session, "repo", "alice", "feature", 2_000)
            .expect("commit feature");
        client
            .checkout(&session, "repo", "main")
            .expect("checkout main");
        client
            .write_file(&session, "repo", "shared.txt", b"main", 0)
            .expect("write main");
        client.stage_all(&session, "repo").expect("stage main");
        client
            .commit(&session, "repo", "alice", "main", 3_000)
            .expect("commit main");
        let before_main = client.log(&session, "repo", "main").expect("main log");

        let request = exec_apply_request("repo", "main", "feature");
        let first = client
            .apply_cbor(&session, &request)
            .expect("apply conflict result");
        let fields = exec_apply_result_value(&first);
        assert_eq!(
            fields.get("schema"),
            Some(&loom_codec::Value::Text(
                "loom.exec.apply.result.v1".to_string()
            ))
        );
        assert_eq!(
            fields.get("outcome"),
            Some(&loom_codec::Value::Text("conflicts".to_string()))
        );
        assert_eq!(
            fields.get("conflicts"),
            Some(&loom_codec::Value::Array(vec![loom_codec::Value::Text(
                "shared.txt".to_string()
            )]))
        );
        assert_eq!(
            client.log(&session, "repo", "main").expect("main log"),
            before_main
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exec_apply_merge_failure_leaves_live_and_durable_state_unchanged() {
        let dir = temp_dir("exec-apply-merge-failure");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open session");
        seed_exec_apply_dirty_feature(&client, &session);
        let before = exec_apply_snapshot(&client, &session);

        let request = exec_apply_request("repo", "main", "missing-fork");
        let err = client
            .apply_cbor(&session, &request)
            .expect_err("missing fork fails after candidate checkout");
        assert_eq!(err.code, Code::NotFound);
        assert_eq!(exec_apply_snapshot(&client, &session), before);
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen store");
        assert_eq!(exec_apply_snapshot(&reopened, &session), before);
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exec_apply_candidate_save_failure_leaves_live_and_durable_state_unchanged() {
        let dir = temp_dir("exec-apply-save-failure");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open session");
        seed_exec_apply_dirty_feature(&client, &session);
        let before = exec_apply_snapshot(&client, &session);

        install_exec_apply_candidate_save_hook(
            path.clone(),
            Box::new(|| {
                Err(LoomError::new(
                    Code::Internal,
                    "injected exec apply candidate save failure",
                ))
            }),
        );
        let request = exec_apply_request("repo", "main", "feature");
        let err = client
            .apply_cbor(&session, &request)
            .expect_err("save hook rejects candidate publication");
        assert_eq!(err.code, Code::Internal);
        assert_eq!(exec_apply_snapshot(&client, &session), before);
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen store");
        assert_eq!(exec_apply_snapshot(&reopened, &session), before);
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn drive_root(
        client: &LocalLoomClient,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
    ) -> loom_drive::HostedDriveFolder {
        client
            .with_session(session, |loom| {
                let workspace = loom.registry().open(&ns_selector_by_name(workspace))?;
                loom_drive::list_folder(loom, workspace, drive_workspace_id, "root")
            })
            .expect("drive root")
    }

    fn drive_folder(
        client: &LocalLoomClient,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        folder_id: &str,
    ) -> loom_drive::HostedDriveFolder {
        client
            .with_session(session, |loom| {
                let workspace = loom.registry().open(&ns_selector_by_name(workspace))?;
                loom_drive::list_folder(loom, workspace, drive_workspace_id, folder_id)
            })
            .expect("drive folder")
    }

    fn drive_conflicts(
        client: &LocalLoomClient,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
    ) -> Vec<loom_drive::HostedDriveConflict> {
        client
            .with_session(session, |loom| {
                let workspace = loom.registry().open(&ns_selector_by_name(workspace))?;
                loom_drive::list_conflicts(loom, workspace, drive_workspace_id)
            })
            .expect("drive conflicts")
    }

    fn drive_shares(
        client: &LocalLoomClient,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
    ) -> Vec<loom_drive::HostedDriveShareGrant> {
        client
            .with_session(session, |loom| {
                let workspace = loom.registry().open(&ns_selector_by_name(workspace))?;
                loom_drive::list_shares(loom, workspace, drive_workspace_id)
            })
            .expect("drive shares")
    }

    fn drive_retention(
        client: &LocalLoomClient,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
    ) -> Vec<loom_drive::HostedDriveRetentionPin> {
        client
            .with_session(session, |loom| {
                let workspace = loom.registry().open(&ns_selector_by_name(workspace))?;
                loom_drive::list_retention(loom, workspace, drive_workspace_id)
            })
            .expect("drive retention")
    }

    fn drive_operation_kinds(
        client: &LocalLoomClient,
        session: &LoomSession,
        drive_workspace_id: &str,
    ) -> Vec<String> {
        client
            .with_session(session, |loom| {
                let log = match loom
                    .store()
                    .control_get(&drive_operation_log_key(drive_workspace_id)?)?
                {
                    Some(bytes) => DriveOperationLog::decode(&bytes)?,
                    None => DriveOperationLog::new(drive_workspace_id, Vec::new())?,
                };
                Ok(log
                    .records
                    .into_iter()
                    .map(|record| record.operation_kind)
                    .collect())
            })
            .expect("drive operation log")
    }

    fn drive_share_read_allowed(
        client: &LocalLoomClient,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        target_kind: &str,
        target_id: &str,
        principal: WorkspaceId,
    ) -> bool {
        client
            .with_session(session, |loom| {
                let workspace = loom.registry().open(&ns_selector_by_name(workspace))?;
                let target = format!("{drive_workspace_id}/{target_kind}/{target_id}");
                let acl = loom.store().acl_store()?.unwrap_or_default();
                Ok(acl
                    .authorize_resource_with_roles(
                        true,
                        principal,
                        [],
                        AclResource::scoped(
                            workspace,
                            AclDomain::Files,
                            None,
                            AclResourceScope::Prefix {
                                kind: AclScopeKind::Collection,
                                value: target.as_bytes(),
                            },
                        ),
                        AclRight::Read,
                    )
                    .is_ok())
            })
            .expect("drive share acl check")
    }

    fn drive_write_value(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("drive write json")
    }

    fn drive_file_bytes(
        client: &LocalLoomClient,
        session: &LoomSession,
        workspace: &str,
        drive_workspace_id: &str,
        file_id: &str,
    ) -> Vec<u8> {
        client
            .with_session(session, |loom| {
                let workspace = loom.registry().open(&ns_selector_by_name(workspace))?;
                loom_drive::read_file(loom, workspace, drive_workspace_id, file_id)
            })
            .expect("drive file bytes")
    }

    fn assert_immediate_writable_reopen_after_close(
        path: &Path,
        client: LocalLoomClient,
        session: LoomSession,
    ) {
        assert!(client.close(&session));
        assert_eq!(client.session_count(), 0);
        drop(session);
        drop(client);
        let reopened = LocalLoomClient::new(path);
        let reopened_session = reopened.open().expect("immediate writable reopen");
        assert!(reopened.close(&reopened_session));
        assert_eq!(reopened.session_count(), 0);
        drop(reopened_session);
        drop(reopened);
        let reopened_again = LocalLoomClient::new(path);
        let reopened_again_session = reopened_again
            .open()
            .expect("second immediate writable reopen");
        assert!(reopened_again.close(&reopened_again_session));
        std::fs::remove_dir_all(path.parent().expect("test path parent")).ok();
    }

    #[test]
    fn drive_generated_hierarchy_writes_persist_after_reopen() {
        let dir = temp_dir("drive-generated-hierarchy");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;

        let create_a = client
            .drive_create_folder_json(&session, "files", drive_id, "root", "folder-a", "A", &root)
            .expect("create folder a");
        let create_a = drive_write_value(&create_a);
        assert_eq!(create_a["operation_kind"], "folder.created");
        let root = create_a["profile_root"].as_str().expect("profile root");
        let create_b = client
            .drive_create_folder_json(&session, "files", drive_id, "root", "folder-b", "B", root)
            .expect("create folder b");
        let root = drive_write_value(&create_b)["profile_root"]
            .as_str()
            .expect("profile root")
            .to_string();

        let rename = client
            .drive_rename_json(&session, "files", drive_id, "root", "folder-a", "A2", &root)
            .expect("rename folder");
        let rename = drive_write_value(&rename);
        assert_eq!(rename["operation_kind"], "file.renamed");
        let root = rename["profile_root"].as_str().expect("profile root");
        let moved = client
            .drive_move_json(
                &session, "files", drive_id, "root", "folder-b", "folder-a", root,
            )
            .expect("move folder");
        let moved = drive_write_value(&moved);
        assert_eq!(moved["operation_kind"], "file.moved");
        let root = moved["profile_root"].as_str().expect("profile root");
        let deleted = client
            .drive_delete_json(&session, "files", drive_id, "folder-b", "folder-a", root)
            .expect("delete folder");
        let deleted = drive_write_value(&deleted);
        assert_eq!(deleted["operation_kind"], "file.deleted");

        client.close(&session);
        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen");
        let root = drive_root(&reopened, &session, "files", drive_id);
        assert_eq!(root.entries.len(), 1);
        assert_eq!(root.entries[0].node_id, "folder-b");
        assert_eq!(
            drive_folder(&reopened, &session, "files", drive_id, "folder-b")
                .entries
                .len(),
            0
        );
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_generated_create_folder_close_releases_writable_store() {
        let dir = temp_dir("drive-generated-create-folder-close-release");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;
        client
            .drive_create_folder_json(&session, "files", drive_id, "root", "folder-a", "A", &root)
            .expect("create folder");
        assert_immediate_writable_reopen_after_close(&path, client, session);
    }

    #[test]
    fn drive_generated_rename_close_releases_writable_store() {
        let dir = temp_dir("drive-generated-rename-close-release");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;
        let created = client
            .drive_create_folder_json(&session, "files", drive_id, "root", "folder-a", "A", &root)
            .expect("create folder");
        let root = drive_write_value(&created)["profile_root"]
            .as_str()
            .expect("profile root")
            .to_string();
        client
            .drive_rename_json(&session, "files", drive_id, "root", "folder-a", "A2", &root)
            .expect("rename");
        assert_immediate_writable_reopen_after_close(&path, client, session);
    }

    #[test]
    fn drive_generated_move_close_releases_writable_store() {
        let dir = temp_dir("drive-generated-move-close-release");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;
        let created = client
            .drive_create_folder_json(&session, "files", drive_id, "root", "folder-a", "A", &root)
            .expect("create folder a");
        let root = drive_write_value(&created)["profile_root"]
            .as_str()
            .expect("profile root")
            .to_string();
        let created = client
            .drive_create_folder_json(&session, "files", drive_id, "root", "folder-b", "B", &root)
            .expect("create folder b");
        let root = drive_write_value(&created)["profile_root"]
            .as_str()
            .expect("profile root")
            .to_string();
        client
            .drive_move_json(
                &session, "files", drive_id, "root", "folder-b", "folder-a", &root,
            )
            .expect("move");
        assert_immediate_writable_reopen_after_close(&path, client, session);
    }

    #[test]
    fn drive_generated_stale_delete_close_releases_writable_store() {
        let dir = temp_dir("drive-generated-stale-delete-close-release");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let stale_root = drive_root(&client, &session, "files", drive_id).profile_root;
        client
            .drive_create_folder_json(
                &session,
                "files",
                drive_id,
                "root",
                "folder-a",
                "A",
                &stale_root,
            )
            .expect("create folder");
        client
            .drive_delete_json(&session, "files", drive_id, "root", "folder-a", &stale_root)
            .expect("stale delete");
        assert_immediate_writable_reopen_after_close(&path, client, session);
    }

    #[test]
    fn drive_generated_resolve_conflict_close_releases_writable_store() {
        let dir = temp_dir("drive-generated-resolve-conflict-close-release");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let stale_root = drive_root(&client, &session, "files", drive_id).profile_root;
        client
            .drive_create_folder_json(
                &session,
                "files",
                drive_id,
                "root",
                "folder-a",
                "A",
                &stale_root,
            )
            .expect("create folder");
        let held_delete = client
            .drive_delete_json(&session, "files", drive_id, "root", "folder-a", &stale_root)
            .expect("stale delete");
        let conflict_id = drive_write_value(&held_delete)["conflict_id"]
            .as_str()
            .expect("conflict id")
            .to_string();
        client
            .drive_resolve_conflict_json(&session, "files", drive_id, &conflict_id, "keep_current")
            .expect("resolve conflict");
        assert_immediate_writable_reopen_after_close(&path, client, session);
    }

    #[test]
    fn drive_generated_create_upload_close_releases_writable_store() {
        let dir = temp_dir("drive-generated-create-upload-close-release");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;
        client
            .drive_create_upload_json(
                &session, "files", drive_id, "upload-a", "root", "A.bin", "file-a", &root, 10,
                false,
            )
            .expect("create upload");
        assert_immediate_writable_reopen_after_close(&path, client, session);
    }

    #[test]
    fn drive_generated_upload_chunk_close_releases_writable_store() {
        let dir = temp_dir("drive-generated-upload-chunk-close-release");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;
        client
            .drive_create_upload_json(
                &session, "files", drive_id, "upload-a", "root", "A.bin", "file-a", &root, 10,
                false,
            )
            .expect("create upload");
        client
            .drive_upload_chunk_json(&session, "files", drive_id, "upload-a", b"bytes")
            .expect("upload chunk");
        assert_immediate_writable_reopen_after_close(&path, client, session);
    }

    #[test]
    fn drive_generated_commit_upload_close_releases_writable_store() {
        let dir = temp_dir("drive-generated-commit-upload-close-release");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;
        client
            .drive_create_upload_json(
                &session, "files", drive_id, "upload-a", "root", "A.bin", "file-a", &root, 10,
                false,
            )
            .expect("create upload");
        client
            .drive_upload_chunk_json(&session, "files", drive_id, "upload-a", b"bytes")
            .expect("upload chunk");
        client
            .drive_commit_upload_json(&session, "files", drive_id, "upload-a")
            .expect("commit upload");
        assert_immediate_writable_reopen_after_close(&path, client, session);
    }

    #[test]
    fn drive_generated_seeded_store_commit_upload_close_releases_writable_store() {
        let dir = temp_dir("drive-generated-seeded-commit-upload-close-release");
        let path = dir.join("t.loom");
        let workspace = WorkspaceId::from_bytes([78; 16]);
        {
            let store = FileStore::create_with_profile(&path, Algo::Blake3).expect("create store");
            let mut loom = Loom::new(store);
            loom.registry_mut()
                .create(FacetKind::Files, Some("files"), workspace)
                .expect("seed fixed files workspace");
            save_loom(&mut loom).expect("save seeded files workspace");
        }
        let client = LocalLoomClient::new(&path);
        let session = client.open().expect("open");
        let drive_id = workspace.to_string();
        let root = drive_root(&client, &session, "files", &drive_id).profile_root;
        client
            .drive_create_upload_json(
                &session, "files", &drive_id, "upload-a", "root", "A.bin", "file-a", &root, 10,
                false,
            )
            .expect("create upload");
        client
            .drive_upload_chunk_json(&session, "files", &drive_id, "upload-a", b"bytes")
            .expect("upload chunk");
        client
            .drive_commit_upload_json(&session, "files", &drive_id, "upload-a")
            .expect("commit upload");
        assert_immediate_writable_reopen_after_close(&path, client, session);
    }

    #[test]
    fn drive_generated_writes_require_collection_auth_before_root_parse() {
        let dir = temp_dir("drive-generated-auth-order");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        seed_authenticated_non_admin(&client, &session);
        let before = std::fs::read(&path).expect("read before");

        let err = client
            .drive_create_folder_json(
                &session,
                "files",
                "drive-local",
                "root",
                "folder-a",
                "A",
                "not-a-digest",
            )
            .expect_err("collection auth wins");
        assert_eq!(err.code, Code::PermissionDenied);
        assert_eq!(std::fs::read(&path).expect("read after"), before);
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_generated_denied_malformed_resolution_does_not_parse() {
        let dir = temp_dir("drive-generated-resolution-auth-order");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        seed_authenticated_non_admin(&client, &session);
        let before = std::fs::read(&path).expect("read before");

        let err = client
            .drive_resolve_conflict_json(
                &session,
                "files",
                "drive-local",
                "conflict-a",
                "not-a-resolution",
            )
            .expect_err("collection auth wins before resolution parsing");
        assert_eq!(err.code, Code::PermissionDenied);
        assert_eq!(std::fs::read(&path).expect("read after"), before);
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_generated_candidate_save_failure_leaves_live_and_durable_state_unchanged() {
        let dir = temp_dir("drive-generated-save-failure");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let before_root = drive_root(&client, &session, "files", drive_id);
        let before_bytes = std::fs::read(&path).expect("read before");

        install_exec_apply_candidate_save_hook(
            path.clone(),
            Box::new(|| {
                Err(LoomError::new(
                    Code::Internal,
                    "injected generated candidate publication failure",
                ))
            }),
        );
        let err = client
            .drive_create_folder_json(
                &session,
                "files",
                drive_id,
                "root",
                "folder-a",
                "A",
                &before_root.profile_root,
            )
            .expect_err("candidate publication fails");
        assert_eq!(err.code, Code::Internal);
        assert_eq!(
            drive_root(&client, &session, "files", drive_id),
            before_root
        );
        assert_eq!(std::fs::read(&path).expect("read after"), before_bytes);
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen");
        assert_eq!(
            drive_root(&reopened, &session, "files", drive_id),
            before_root
        );
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_upload_generated_requires_collection_auth_before_parse_or_lookup() {
        let dir = temp_dir("drive-upload-generated-auth-order");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        seed_authenticated_non_admin(&client, &session);
        let before = std::fs::read(&path).expect("read before");

        let err = client
            .drive_create_upload_json(
                &session,
                "files",
                "drive-local",
                "upload-a",
                "root",
                "A",
                "file-a",
                "not-a-digest",
                1,
                false,
            )
            .expect_err("collection auth wins before expected root parsing");
        assert_eq!(err.code, Code::PermissionDenied);

        let err = client
            .drive_upload_chunk_json(&session, "files", "drive-local", "missing-upload", b"chunk")
            .expect_err("collection auth wins before upload session lookup");
        assert_eq!(err.code, Code::PermissionDenied);

        let err = client
            .drive_commit_upload_json(&session, "files", "drive-local", "missing-upload")
            .expect_err("collection auth wins before commit session lookup");
        assert_eq!(err.code, Code::PermissionDenied);
        assert_eq!(std::fs::read(&path).expect("read after"), before);
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_upload_generated_preserves_raw_bytes_cleans_session_and_reopens() {
        let dir = temp_dir("drive-upload-generated-raw-reopen");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;

        let created = client
            .drive_create_upload_json(
                &session, "files", drive_id, "upload-a", "root", "A.bin", "file-a", &root, 10,
                false,
            )
            .expect("create upload");
        assert_eq!(drive_write_value(&created)["chunk_count"], 0);
        let first = b"\x00raw-\xff".as_slice();
        let second = b"\xfe-tail".as_slice();
        let uploaded = client
            .drive_upload_chunk_json(&session, "files", drive_id, "upload-a", first)
            .expect("upload first chunk");
        assert_eq!(drive_write_value(&uploaded)["chunk_count"], 1);
        let uploaded = client
            .drive_upload_chunk_json(&session, "files", drive_id, "upload-a", second)
            .expect("upload second chunk");
        assert_eq!(drive_write_value(&uploaded)["total_size"], 12);
        let committed = client
            .drive_commit_upload_json(&session, "files", drive_id, "upload-a")
            .expect("commit upload");
        let committed = drive_write_value(&committed);
        assert_eq!(committed["operation_kind"], "file.upload_committed");
        assert!(committed["conflict_id"].is_null());
        let mut expected: Vec<u8> = Vec::new();
        expected.extend(first);
        expected.extend(second);
        assert_eq!(
            drive_file_bytes(&client, &session, "files", drive_id, "file-a"),
            expected
        );
        let err = client
            .drive_upload_chunk_json(&session, "files", drive_id, "upload-a", b"again")
            .expect_err("committed upload session is cleaned up");
        assert_eq!(err.code, Code::NotFound);
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen");
        assert_eq!(
            drive_file_bytes(&reopened, &session, "files", drive_id, "file-a"),
            expected
        );
        let err = reopened
            .drive_commit_upload_json(&session, "files", drive_id, "upload-a")
            .expect_err("upload session remains cleaned after reopen");
        assert_eq!(err.code, Code::NotFound);
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_upload_generated_preserves_replacement_and_new_file_conflict_semantics() {
        let dir = temp_dir("drive-upload-generated-conflicts");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;

        client
            .drive_create_upload_json(
                &session,
                "files",
                drive_id,
                "upload-collision",
                "root",
                "A2.txt",
                "file-b",
                &root,
                3,
                false,
            )
            .expect("create new-file upload before colliding root advances");
        client
            .drive_upload_chunk_json(
                &session,
                "files",
                drive_id,
                "upload-collision",
                b"collision",
            )
            .expect("collision chunk");
        client
            .drive_create_upload_json(
                &session,
                "files",
                drive_id,
                "upload-base",
                "root",
                "A.txt",
                "file-a",
                &root,
                1,
                false,
            )
            .expect("create base upload");
        client
            .drive_upload_chunk_json(&session, "files", drive_id, "upload-base", b"base")
            .expect("base chunk");
        let base_commit = client
            .drive_commit_upload_json(&session, "files", drive_id, "upload-base")
            .expect("base commit");
        let base_root = drive_write_value(&base_commit)["profile_root"]
            .as_str()
            .expect("base profile root")
            .to_string();

        client
            .drive_create_upload_json(
                &session,
                "files",
                drive_id,
                "upload-replace-stale",
                "root",
                "A.txt",
                "file-a",
                &base_root,
                2,
                true,
            )
            .expect("create stale replacement upload");
        client
            .drive_upload_chunk_json(
                &session,
                "files",
                drive_id,
                "upload-replace-stale",
                b"replacement",
            )
            .expect("replacement chunk");
        let renamed = client
            .drive_rename_json(
                &session, "files", drive_id, "root", "file-a", "A2.txt", &base_root,
            )
            .expect("advance root");
        let advanced_root = drive_write_value(&renamed)["profile_root"]
            .as_str()
            .expect("advanced profile root")
            .to_string();
        let err = client
            .drive_commit_upload_json(&session, "files", drive_id, "upload-replace-stale")
            .expect_err("stale replacement conflicts");
        assert_eq!(err.code, Code::Conflict);
        assert_eq!(
            drive_file_bytes(&client, &session, "files", drive_id, "file-a"),
            b"base"
        );

        let collision = client
            .drive_commit_upload_json(&session, "files", drive_id, "upload-collision")
            .expect("new-file collision records conflict");
        let collision = drive_write_value(&collision);
        assert_eq!(collision["operation_kind"], "file.upload_committed");
        assert!(collision["conflict_id"].as_str().is_some());
        assert_ne!(
            collision["profile_root"].as_str().expect("collision root"),
            advanced_root
        );
        assert_eq!(
            drive_conflicts(&client, &session, "files", drive_id)
                .iter()
                .filter(|conflict| {
                    Some(conflict.conflict_id.as_str()) == collision["conflict_id"].as_str()
                })
                .count(),
            1
        );
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_upload_generated_publication_failure_leaves_live_and_reopened_state_unchanged() {
        let dir = temp_dir("drive-upload-generated-publication-failure");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;
        client
            .drive_create_upload_json(
                &session, "files", drive_id, "upload-a", "root", "A.txt", "file-a", &root, 1, false,
            )
            .expect("create upload");
        let before_root = drive_root(&client, &session, "files", drive_id);
        let before_conflicts = drive_conflicts(&client, &session, "files", drive_id);
        let before_bytes = std::fs::read(&path).expect("read before");

        install_exec_apply_candidate_save_hook(
            path.clone(),
            Box::new(|| {
                Err(LoomError::new(
                    Code::Internal,
                    "injected upload candidate publication failure",
                ))
            }),
        );
        let err = client
            .drive_upload_chunk_json(&session, "files", drive_id, "upload-a", b"not-published")
            .expect_err("candidate publication fails before upload chunk persists");
        assert_eq!(err.code, Code::Internal);
        assert_eq!(
            drive_root(&client, &session, "files", drive_id),
            before_root
        );
        assert_eq!(
            drive_conflicts(&client, &session, "files", drive_id),
            before_conflicts
        );
        assert_eq!(std::fs::read(&path).expect("read after"), before_bytes);
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen");
        assert_eq!(
            drive_root(&reopened, &session, "files", drive_id),
            before_root
        );
        assert_eq!(
            drive_conflicts(&reopened, &session, "files", drive_id),
            before_conflicts
        );
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_retention_generated_requires_collection_auth_before_parse_or_lookup() {
        let dir = temp_dir("drive-retention-generated-auth-order");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        seed_authenticated_non_admin(&client, &session);
        let before = std::fs::read(&path).expect("read before");

        let err = client
            .drive_pin_retention_json(
                &session,
                "files",
                "drive-local",
                "pin-bad",
                "not-a-kind",
                "not-a-digest",
                Some("not-a-target"),
                1,
                Some(2),
            )
            .expect_err("collection auth wins before retention parsing");
        assert_eq!(err.code, Code::PermissionDenied);

        let err = client
            .drive_unpin_retention_json(&session, "files", "drive-local", "missing-pin")
            .expect_err("collection auth wins before retention lookup");
        assert_eq!(err.code, Code::PermissionDenied);

        let err = client
            .drive_apply_retention_json(&session, "files", "drive-local", 50)
            .expect_err("collection auth wins before retention scan");
        assert_eq!(err.code, Code::PermissionDenied);
        assert_eq!(std::fs::read(&path).expect("read after"), before);
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_retention_generated_pin_unpin_apply_reopen_and_notifications() {
        let dir = temp_dir("drive-retention-generated-flow");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;
        client
            .drive_create_folder_json(&session, "files", drive_id, "root", "folder-a", "A", &root)
            .expect("create folder");
        let current_root = drive_root(&client, &session, "files", drive_id).profile_root;
        let before_audit = audit_actions(&client, &session);

        let invalid_digest = client
            .drive_pin_retention_json(
                &session,
                "files",
                drive_id,
                "pin-invalid",
                "current_root",
                "not-a-digest",
                Some("folder:folder-a"),
                10,
                None,
            )
            .expect_err("invalid digest");
        assert_eq!(invalid_digest.code, Code::InvalidArgument);
        assert!(drive_retention(&client, &session, "files", drive_id).is_empty());

        let malformed_target = client
            .drive_pin_retention_json(
                &session,
                "files",
                drive_id,
                "pin-target",
                "current_root",
                &current_root,
                Some("folder-a"),
                10,
                None,
            )
            .expect_err("malformed target");
        assert_eq!(malformed_target.code, Code::InvalidArgument);
        assert!(drive_retention(&client, &session, "files", drive_id).is_empty());

        let pinned = client
            .drive_pin_retention_json(
                &session,
                "files",
                drive_id,
                "pin-live",
                "trash_subtree",
                &current_root,
                Some("folder:folder-a"),
                20,
                Some(100),
            )
            .expect("pin retention");
        let pinned = drive_write_value(&pinned);
        assert_eq!(pinned["operation_kind"], "retention.pinned");
        assert_eq!(pinned["target_entity_id"], "retention:pin-live");
        assert_eq!(
            drive_retention(&client, &session, "files", drive_id)
                .iter()
                .map(|pin| pin.pin_id.clone())
                .collect::<Vec<_>>(),
            vec!["pin-live".to_string()]
        );
        let duplicate = client
            .drive_pin_retention_json(
                &session,
                "files",
                drive_id,
                "pin-live",
                "trash_subtree",
                &current_root,
                Some("folder:folder-a"),
                21,
                Some(101),
            )
            .expect_err("duplicate pin");
        assert_eq!(duplicate.code, Code::AlreadyExists);
        assert_eq!(
            drive_retention(&client, &session, "files", drive_id).len(),
            1
        );

        let no_op = client
            .drive_apply_retention_json(&session, "files", drive_id, 50)
            .expect("no-op apply");
        let no_op = drive_write_value(&no_op);
        assert!(no_op["operation"].is_null());
        assert_eq!(no_op["remaining_pins"], 1);
        assert_eq!(
            drive_operation_kinds(&client, &session, drive_id),
            vec!["folder.created".to_string(), "retention.pinned".to_string()]
        );

        let expired = client
            .drive_apply_retention_json(&session, "files", drive_id, 100)
            .expect("apply expired retention");
        let expired = drive_write_value(&expired);
        assert_eq!(expired["expired_pin_ids"], serde_json::json!(["pin-live"]));
        assert_eq!(expired["operation"]["operation_kind"], "retention.applied");
        assert!(drive_retention(&client, &session, "files", drive_id).is_empty());

        let missing = client
            .drive_unpin_retention_json(&session, "files", drive_id, "missing")
            .expect_err("missing pin");
        assert_eq!(missing.code, Code::NotFound);

        client
            .drive_pin_retention_json(
                &session,
                "files",
                drive_id,
                "pin-remove",
                "current_root",
                &current_root,
                None,
                200,
                None,
            )
            .expect("pin for unpin");
        let unpinned = client
            .drive_unpin_retention_json(&session, "files", drive_id, "pin-remove")
            .expect("unpin retention");
        assert_eq!(
            drive_write_value(&unpinned)["operation_kind"],
            "retention.unpinned"
        );
        assert!(drive_retention(&client, &session, "files", drive_id).is_empty());
        assert!(drive_retention(&client, &session, "files", "other-drive").is_empty());
        assert_eq!(audit_actions(&client, &session), before_audit);
        assert_eq!(
            drive_operation_kinds(&client, &session, drive_id),
            vec![
                "folder.created".to_string(),
                "retention.pinned".to_string(),
                "retention.applied".to_string(),
                "retention.pinned".to_string(),
                "retention.unpinned".to_string(),
            ]
        );
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen");
        assert!(drive_retention(&reopened, &session, "files", drive_id).is_empty());
        assert_eq!(audit_actions(&reopened, &session), before_audit);
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_retention_generated_publication_failure_preserves_live_and_reopened_state() {
        let dir = temp_dir("drive-retention-generated-publication-failure");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;
        let before_retention = drive_retention(&client, &session, "files", drive_id);
        let before_operations = drive_operation_kinds(&client, &session, drive_id);
        let before_audit = audit_actions(&client, &session);
        let before_bytes = std::fs::read(&path).expect("read before");

        install_exec_apply_candidate_save_hook(
            path.clone(),
            Box::new(|| {
                Err(LoomError::new(
                    Code::Internal,
                    "injected retention candidate publication failure",
                ))
            }),
        );
        let err = client
            .drive_pin_retention_json(
                &session,
                "files",
                drive_id,
                "pin-fail",
                "current_root",
                &root,
                None,
                10,
                None,
            )
            .expect_err("candidate publication fails before retention persists");
        assert_eq!(err.code, Code::Internal);
        assert_eq!(
            drive_retention(&client, &session, "files", drive_id),
            before_retention
        );
        assert_eq!(
            drive_operation_kinds(&client, &session, drive_id),
            before_operations
        );
        assert_eq!(audit_actions(&client, &session), before_audit);
        assert_eq!(std::fs::read(&path).expect("read after"), before_bytes);
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen");
        assert_eq!(
            drive_retention(&reopened, &session, "files", drive_id),
            before_retention
        );
        assert_eq!(
            drive_operation_kinds(&reopened, &session, drive_id),
            before_operations
        );
        assert_eq!(audit_actions(&reopened, &session), before_audit);
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_generated_stale_and_missing_mutations_preserve_existing_state() {
        let dir = temp_dir("drive-generated-stale");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let stale_root = drive_root(&client, &session, "files", drive_id).profile_root;
        let created = client
            .drive_create_folder_json(
                &session,
                "files",
                drive_id,
                "root",
                "folder-a",
                "A",
                &stale_root,
            )
            .expect("create folder");
        let current_root = drive_write_value(&created)["profile_root"]
            .as_str()
            .expect("profile root")
            .to_string();
        let before = drive_root(&client, &session, "files", drive_id);

        let err = client
            .drive_rename_json(
                &session,
                "files",
                drive_id,
                "root",
                "folder-a",
                "A2",
                &stale_root,
            )
            .expect_err("stale rename conflicts");
        assert_eq!(err.code, Code::Conflict);
        assert_eq!(
            drive_root(&client, &session, "files", drive_id).entries,
            before.entries
        );

        let err = client
            .drive_move_json(
                &session,
                "files",
                drive_id,
                "root",
                "folder-a",
                "missing-node",
                &current_root,
            )
            .expect_err("missing node");
        assert_eq!(err.code, Code::NotFound);
        assert_eq!(
            drive_root(&client, &session, "files", drive_id).entries,
            before.entries
        );

        let held_delete = client
            .drive_delete_json(&session, "files", drive_id, "root", "folder-a", &stale_root)
            .expect("stale delete creates held conflict");
        let held_delete = drive_write_value(&held_delete);
        assert_eq!(held_delete["operation_kind"], "folder.delete_held");
        let conflict_id = held_delete["conflict_id"]
            .as_str()
            .expect("held delete conflict");
        assert_eq!(
            drive_root(&client, &session, "files", drive_id).entries,
            before.entries
        );
        assert_eq!(
            drive_conflicts(&client, &session, "files", drive_id)
                .iter()
                .filter(|conflict| conflict.conflict_id == conflict_id)
                .count(),
            1
        );

        let resolved = client
            .drive_resolve_conflict_json(&session, "files", drive_id, conflict_id, "keep_current")
            .expect("resolve held delete");
        assert_eq!(
            drive_write_value(&resolved)["operation_kind"],
            "conflict.resolved"
        );
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen");
        assert_eq!(
            drive_root(&reopened, &session, "files", drive_id).entries,
            before.entries
        );
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn watch_group_observes_a_new_commit() {
        let dir = temp_dir("watch");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        client
            .write_file(&session, "repo", "a.txt", b"1", 0)
            .expect("write a");
        client.stage_all(&session, "repo").expect("stage");
        client
            .commit(&session, "repo", "alice", "first", 1_000)
            .expect("commit 1");

        let cursor = client
            .watch_subscribe(&session, "repo", "main", None)
            .expect("subscribe");

        client
            .write_file(&session, "repo", "b.txt", b"2", 0)
            .expect("write b");
        client.stage_all(&session, "repo").expect("stage 2");
        client
            .commit(&session, "repo", "alice", "second", 2_000)
            .expect("commit 2");

        let batch = client.watch_poll(&session, &cursor, 10).expect("poll");
        assert!(!batch.events.is_empty(), "poll sees the second commit");

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_generated_share_writes_authorize_before_caller_fields() {
        let dir = temp_dir("drive-generated-share-auth-order");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        seed_authenticated_non_admin(&client, &session);
        let before = std::fs::read(&path).expect("read before");

        let err = client
            .drive_grant_share_json(
                &session,
                "files",
                "drive-local",
                "grant-bad",
                "not-target-kind",
                "bad-target",
                "not-a-principal",
                "not-a-role",
                1,
                Some(2),
            )
            .expect_err("collection auth wins before share field parsing");
        assert_eq!(err.code, Code::PermissionDenied);
        assert_eq!(std::fs::read(&path).expect("read after"), before);
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_generated_share_grant_revoke_expiry_acl_audit_and_reopen() {
        let dir = temp_dir("drive-generated-share-flow");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;
        let created = client
            .drive_create_folder_json(&session, "files", drive_id, "root", "folder-a", "A", &root)
            .expect("create folder");
        assert_eq!(
            drive_write_value(&created)["operation_kind"],
            "folder.created"
        );
        let grantee = WorkspaceId::from_bytes([77; 16]);

        let grant = client
            .drive_grant_share_json(
                &session,
                "files",
                drive_id,
                "grant-live",
                "folder",
                "folder-a",
                &grantee.to_string(),
                "viewer",
                10,
                Some(100),
            )
            .expect("grant share");
        assert_eq!(drive_write_value(&grant)["operation_kind"], "share.granted");
        assert!(drive_share_read_allowed(
            &client, &session, "files", drive_id, "folder", "folder-a", grantee
        ));
        assert_eq!(
            drive_shares(&client, &session, "files", drive_id)
                .iter()
                .map(|grant| grant.grant_id.clone())
                .collect::<Vec<_>>(),
            vec!["grant-live".to_string()]
        );

        let duplicate = client
            .drive_grant_share_json(
                &session,
                "files",
                drive_id,
                "grant-live",
                "folder",
                "folder-a",
                &grantee.to_string(),
                "viewer",
                11,
                None,
            )
            .expect_err("duplicate share");
        assert_eq!(duplicate.code, Code::AlreadyExists);
        assert_eq!(drive_shares(&client, &session, "files", drive_id).len(), 1);

        let no_op = client
            .drive_apply_share_expiry_json(&session, "files", drive_id, 50)
            .expect("no-op expiry");
        let no_op = drive_write_value(&no_op);
        assert!(no_op["operation"].is_null());
        assert_eq!(no_op["remaining_grants"], 1);
        assert_eq!(
            drive_operation_kinds(&client, &session, drive_id),
            vec!["folder.created".to_string(), "share.granted".to_string()]
        );

        let revoke = client
            .drive_revoke_share_json(&session, "files", drive_id, "grant-live")
            .expect("revoke share");
        assert_eq!(
            drive_write_value(&revoke)["operation_kind"],
            "share.revoked"
        );
        assert!(!drive_share_read_allowed(
            &client, &session, "files", drive_id, "folder", "folder-a", grantee
        ));
        assert!(drive_shares(&client, &session, "files", drive_id).is_empty());
        let missing = client
            .drive_revoke_share_json(&session, "files", drive_id, "missing")
            .expect_err("missing grant");
        assert_eq!(missing.code, Code::NotFound);

        client
            .drive_grant_share_json(
                &session,
                "files",
                drive_id,
                "grant-expiring",
                "folder",
                "folder-a",
                &grantee.to_string(),
                "editor",
                20,
                Some(30),
            )
            .expect("grant expiring share");
        let expired = client
            .drive_apply_share_expiry_json(&session, "files", drive_id, 30)
            .expect("apply expiry");
        let expired = drive_write_value(&expired);
        assert_eq!(
            expired["expired_grant_ids"],
            serde_json::json!(["grant-expiring"])
        );
        assert_eq!(
            expired["operation"]["operation_kind"],
            serde_json::json!("share.expired")
        );
        assert!(!drive_share_read_allowed(
            &client, &session, "files", drive_id, "folder", "folder-a", grantee
        ));
        assert!(drive_shares(&client, &session, "files", drive_id).is_empty());
        assert_eq!(
            audit_actions(&client, &session),
            vec![
                "drive.share_acl.grant".to_string(),
                "drive.share_acl.revoke".to_string(),
                "drive.share_acl.grant".to_string(),
                "drive.share_acl.expire".to_string(),
            ]
        );
        assert_eq!(
            drive_operation_kinds(&client, &session, drive_id),
            vec![
                "folder.created".to_string(),
                "share.granted".to_string(),
                "share.revoked".to_string(),
                "share.granted".to_string(),
                "share.expired".to_string(),
            ]
        );

        let other_drive = "drive-other";
        assert!(drive_shares(&client, &session, "files", other_drive).is_empty());
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen");
        assert!(drive_shares(&reopened, &session, "files", drive_id).is_empty());
        assert_eq!(
            audit_actions(&reopened, &session),
            vec![
                "drive.share_acl.grant".to_string(),
                "drive.share_acl.revoke".to_string(),
                "drive.share_acl.grant".to_string(),
                "drive.share_acl.expire".to_string(),
            ]
        );
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn drive_generated_share_publication_failure_preserves_live_and_reopened_state() {
        let dir = temp_dir("drive-generated-share-save-failure");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let session = client.open().expect("open");
        client
            .workspace_create(&session, Some("files"), Some(FacetKind::Files))
            .expect("workspace");
        let drive_id = "drive-local";
        let root = drive_root(&client, &session, "files", drive_id).profile_root;
        client
            .drive_create_folder_json(&session, "files", drive_id, "root", "folder-a", "A", &root)
            .expect("create folder");
        let before_shares = drive_shares(&client, &session, "files", drive_id);
        let before_audit = audit_actions(&client, &session);
        let before_bytes = std::fs::read(&path).expect("read before");
        let grantee = WorkspaceId::from_bytes([78; 16]);

        install_exec_apply_candidate_save_hook(
            path.clone(),
            Box::new(|| {
                Err(LoomError::new(
                    Code::Internal,
                    "injected generated candidate publication failure",
                ))
            }),
        );
        let err = client
            .drive_grant_share_json(
                &session,
                "files",
                drive_id,
                "grant-fail",
                "folder",
                "folder-a",
                &grantee.to_string(),
                "viewer",
                40,
                None,
            )
            .expect_err("publication fails");
        assert_eq!(err.code, Code::Internal);
        assert_eq!(
            drive_shares(&client, &session, "files", drive_id),
            before_shares
        );
        assert_eq!(audit_actions(&client, &session), before_audit);
        assert_eq!(std::fs::read(&path).expect("read after"), before_bytes);
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen");
        assert_eq!(
            drive_shares(&reopened, &session, "files", drive_id),
            before_shares
        );
        assert_eq!(audit_actions(&reopened, &session), before_audit);
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_handle_group_roundtrips_through_the_engine() {
        let dir = temp_dir("filehandle");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        let writer = client
            .file_open(&session, "fs", "h.txt", OpenMode::Write)
            .expect("open write");
        client
            .file_write(&session, writer, b"hello")
            .expect("write");
        assert_eq!(client.file_stat(&session, writer).expect("stat").size, 5);
        client.file_close(&session, writer).expect("close writer");

        let reader = client
            .file_open(&session, "fs", "h.txt", OpenMode::Read)
            .expect("open read");
        assert_eq!(
            client.file_read(&session, reader, 100).expect("read"),
            b"hello".to_vec()
        );
        client.file_close(&session, reader).expect("close reader");

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_version_is_reported() {
        let client = LocalLoomClient::new("unused.loom");
        assert!(!client.store_version().is_empty());
    }

    #[test]
    fn protected_ref_group_roundtrips_through_the_engine() {
        let dir = temp_dir("protectedref");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        client
            .write_file(&session, "repo", "f.txt", b"1", 0)
            .expect("write");
        client.stage_all(&session, "repo").expect("stage");
        client
            .commit(&session, "repo", "alice", "init", 1_000)
            .expect("commit");

        let policy = ProtectedRefPolicy {
            fast_forward_only: true,
            signed_commits_required: false,
            signed_ref_advance_required: false,
            required_review_count: 0,
            retention_lock: false,
            governance_lock: false,
        };
        client
            .protected_ref_set(&session, "repo", "branch/main", policy)
            .expect("set policy");
        assert!(
            client
                .protected_ref_get(&session, "repo", "branch/main")
                .expect("get")
                .is_some()
        );
        assert_eq!(
            client
                .protected_ref_list(&session, "repo")
                .expect("list")
                .len(),
            1
        );
        assert!(
            client
                .protected_ref_remove(&session, "repo", "branch/main")
                .expect("remove")
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn acl_group_roundtrips_through_the_engine() {
        use loom_core::acl::{AclEffect, AclRight, AclScope, AclSubject};
        use std::collections::BTreeSet;
        let dir = temp_dir("acl");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        assert!(client.acl_list(&session).expect("list empty").is_empty());

        let grant = AclGrant {
            subject: AclSubject::Everyone,
            workspace: None,
            domain: None,
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: BTreeSet::from([AclRight::Read]),
            effect: AclEffect::Allow,
            predicate: None,
        };
        client.acl_grant(&session, grant.clone()).expect("grant");
        assert_eq!(client.acl_list(&session).expect("list").len(), 1);
        assert!(client.acl_revoke(&session, &grant).expect("revoke"));
        assert!(client.acl_list(&session).expect("list after").is_empty());

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn canonical_locks_local_client_derives_session_owner() {
        let dir = temp_dir("locks");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let loom = Arc::new(Mutex::new(open_loom(&path).expect("open shared loom")));
        let alice = client
            .register_daemon_shared_loom(Arc::clone(&loom), None)
            .expect("register alice");
        let bob = client
            .register_daemon_shared_loom(loom, None)
            .expect("register bob");

        let token = client
            .lock_acquire(&alice, b"resource", LockMode::Exclusive, 60_000, 0)
            .expect("acquire");
        assert_eq!(token.owner.principal, "unauthenticated-root");

        assert!(matches!(
            client.lock_acquire(&bob, b"resource", LockMode::Exclusive, 60_000, 0),
            Err(e) if e.code == Code::Locked
        ));

        let refreshed = client
            .lock_refresh(&alice, &token, 60_000)
            .expect("refresh");
        assert_eq!(
            client
                .lock_refresh(&bob, &token, 60_000)
                .expect_err("foreign refresh")
                .code,
            Code::PermissionDenied
        );
        assert_eq!(
            client
                .lock_release(&bob, &token)
                .expect_err("foreign release")
                .code,
            Code::PermissionDenied
        );
        client.lock_release(&alice, &refreshed).expect("release");

        let token2 = client
            .lock_acquire(&bob, b"resource", LockMode::Exclusive, 60_000, 0)
            .expect("re-acquire");
        client.lock_release(&bob, &token2).expect("release 2");
        client.close(&alice);
        client.close(&bob);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn canonical_locks_embedded_logical_session_survives_attachment_replacement() {
        let dir = temp_dir("embedded-logical-locks");
        let path = dir.join("store.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let loom = Arc::new(Mutex::new(open_loom(&path).expect("open shared loom")));
        let first = client
            .register_daemon_shared_loom(Arc::clone(&loom), None)
            .expect("first attachment");
        client
            .bind_clear_logical_lock_session(&first, "logical-session".to_string())
            .expect("bind first attachment");
        let token = client
            .lock_acquire(&first, b"resource", LockMode::Exclusive, 60_000, 0)
            .expect("acquire through first attachment");
        client.close(&first);

        let resumed = client
            .register_daemon_shared_loom(Arc::clone(&loom), None)
            .expect("replacement attachment");
        client
            .bind_clear_logical_lock_session(&resumed, "logical-session".to_string())
            .expect("resume logical owner");
        let refreshed = client
            .lock_refresh(&resumed, &token, 60_000)
            .expect("refresh through replacement attachment");

        let foreign = client
            .register_daemon_shared_loom(loom, None)
            .expect("foreign attachment");
        client
            .bind_clear_logical_lock_session(&foreign, "foreign-session".to_string())
            .expect("bind foreign owner");
        assert_eq!(
            client
                .lock_release(&foreign, &refreshed)
                .expect_err("foreign logical session release")
                .code,
            Code::PermissionDenied
        );
        client
            .lock_release(&resumed, &refreshed)
            .expect("release through resumed owner");
        client.close(&resumed);
        client.close(&foreign);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn canonical_locks_local_clients_share_injected_authority() {
        let dir = temp_dir("shared-locks");
        let authority: Arc<dyn LocksAuthority> = Arc::new(InProcessLocksAuthority::default());
        let first =
            LocalLoomClient::with_locks_authority(dir.join("first.loom"), Arc::clone(&authority));
        let second =
            LocalLoomClient::with_locks_authority(dir.join("second.loom"), Arc::clone(&authority));
        first.create().expect("create first");
        second.create().expect("create second");
        let first_session = first.open().expect("open first");
        let second_session = second.open().expect("open second");
        let token = first
            .lock_acquire(
                &first_session,
                b"shared-resource",
                LockMode::Exclusive,
                60_000,
                0,
            )
            .expect("first acquire");
        assert_eq!(
            second
                .lock_acquire(
                    &second_session,
                    b"shared-resource",
                    LockMode::Exclusive,
                    60_000,
                    0,
                )
                .expect_err("shared contention")
                .code,
            Code::Locked
        );
        assert_eq!(
            second
                .lock_release(&second_session, &token)
                .expect_err("foreign client release")
                .code,
            Code::PermissionDenied
        );
        first
            .lock_release(&first_session, &token)
            .expect("owner release");
        first.close(&first_session);
        second.close(&second_session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn canonical_locks_local_client_uses_registered_authenticated_owner() {
        let dir = temp_dir("authenticated-locks");
        let path = dir.join("store.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create store");
        let loom = Arc::new(Mutex::new(open_loom(&path).expect("open shared loom")));
        let principal = mint_workspace_id().expect("principal");
        let session = client
            .register_daemon_shared_loom(loom, Some((principal, "authenticated-session".into())))
            .expect("register authenticated session");
        let token = client
            .lock_acquire(&session, b"resource", LockMode::Exclusive, 60_000, 0)
            .expect("acquire");
        assert_eq!(token.owner.principal, principal.to_string());
        assert_eq!(token.owner.session, "authenticated-session");
        client.lock_release(&session, &token).expect("release");
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_session_is_not_found() {
        let dir = temp_dir("unknown");
        let client = LocalLoomClient::new(dir.join("absent.loom"));
        let bogus = LoomSession(HandleId {
            kind: "session".to_string(),
            id: 99u64.to_be_bytes().to_vec(),
            generation: 1,
            owner_session: Vec::new(),
        });
        let result = client.with_session(&bogus, |_| Ok(()));
        assert!(matches!(result, Err(e) if e.code == Code::NotFound));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn identity_and_sessions_group() {
        use loom_core::identity::IdentityStore;
        let dir = temp_dir("identity");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        // A fresh unauthenticated-root store carries no identity control plane.
        assert!(
            client
                .identity_list(&session)
                .expect("list empty")
                .is_empty()
        );
        assert!(!client.identity_authenticated(&session).expect("auth flag"));
        assert!(matches!(
            client.identity_add_principal(&session, "svc", "Service", PrincipalKind::Service),
            Err(e) if e.code == Code::Unsupported
        ));
        assert!(matches!(
            client.authenticate_passphrase(&session, mint_workspace_id().unwrap(), b"pw"),
            Err(e) if e.code == Code::Unsupported
        ));
        client.clear_authentication(&session).expect("clear");

        // Seed an identity control plane with a root and a user principal.
        let root = mint_workspace_id().expect("root id");
        let user = mint_workspace_id().expect("user id");
        client
            .with_session(&session, |loom| {
                let mut identity = IdentityStore::new(root);
                identity.add_principal(user, "user", PrincipalKind::User)?;
                identity.set_passphrase(user, "s3cret", b"salt-bytes")?;
                loom.store().save_identity_store(&identity)
            })
            .expect("seed identity");

        let ids: Vec<PrincipalId> = client
            .identity_list(&session)
            .expect("list seeded")
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert!(ids.contains(&root));
        assert!(ids.contains(&user));

        // Authenticating with the right passphrase binds the session; a wrong one fails.
        client
            .authenticate_passphrase(&session, user, b"s3cret")
            .expect("authenticate");
        assert!(matches!(
            client.authenticate_passphrase(&session, user, b"wrong"),
            Err(e) if e.code == Code::AuthenticationFailed
        ));
        client
            .clear_authentication(&session)
            .expect("clear after auth");

        // Admin mutations persist through the control plane.
        let added = client
            .identity_add_principal(&session, "svc", "Service", PrincipalKind::Service)
            .expect("add principal");
        assert!(
            client
                .identity_list(&session)
                .expect("list after add")
                .into_iter()
                .any(|p| p.id == added)
        );
        client
            .identity_set_passphrase(&session, added, b"pw2", b"salt2-bytes")
            .expect("set passphrase");

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn identity_audited_mutations_return_audit_result() {
        use loom_core::identity::{IdentityPublicKeySpec, IdentityStore};
        let dir = temp_dir("identity-audit");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        let root = mint_workspace_id().expect("root id");
        let user = mint_workspace_id().expect("user id");
        client
            .with_session(&session, |loom| {
                let mut identity = IdentityStore::new(root);
                identity.set_passphrase(root, "rootpw", b"root-salt-bytes")?;
                identity.add_principal(user, "user", PrincipalKind::User)?;
                loom.store().save_identity_store(&identity)
            })
            .expect("seed identity");
        client
            .authenticate_passphrase(&session, root, b"rootpw")
            .expect("authenticate root");

        let key_id = mint_workspace_id().expect("key id");
        let add = client
            .identity_add_public_key(
                &session,
                user,
                IdentityPublicKeySpec {
                    id: key_id,
                    label: "laptop".to_string(),
                    algorithm: "Ed25519".to_string(),
                    public_key: vec![7u8; 32],
                },
            )
            .expect("add public key");
        assert_eq!(add.id, Some(key_id));
        assert_eq!(add.action, "identity.public_key.add");
        assert_eq!(
            add.target.as_deref(),
            Some(format!("principal={user};key={key_id}").as_str())
        );

        let revoke = client
            .identity_revoke_public_key(&session, key_id)
            .expect("revoke public key");
        assert_eq!(revoke.id, Some(key_id));
        assert_eq!(revoke.action, "identity.public_key.revoke");
        assert!(revoke.audit_seq > add.audit_seq);

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn identity_authority_policy_generated_json_mutations_roundtrip() {
        use loom_core::identity::IdentityStore;
        let dir = temp_dir("identity-authority-policy");
        let source_path = dir.join("source.loom");
        let destination_path = dir.join("destination.loom");
        let root = mint_workspace_id().expect("root id");

        let source = LocalLoomClient::new(&source_path);
        source.create().expect("create source");
        let source_session = source.open().expect("open source");
        source
            .with_session(&source_session, |loom| {
                let identity = IdentityStore::new(root);
                loom.store().save_identity_store(&identity)?;
                loom.set_identity_store(identity);
                Ok(())
            })
            .expect("seed source");

        let client = LocalLoomClient::new(&destination_path);
        client.create().expect("create destination");
        let session = client.open().expect("open destination");
        client
            .with_session(&session, |loom| {
                let identity = IdentityStore::new(root);
                loom.store().save_identity_store(&identity)?;
                loom.set_identity_store(identity);
                Ok(())
            })
            .expect("seed destination");

        let replicate = client
            .identity_replicate_authority_json(
                &session,
                source_path.to_str().expect("source path"),
                false,
            )
            .expect("replicate");
        let replicate: serde_json::Value =
            serde_json::from_str(&replicate).expect("replicate json");
        assert_eq!(replicate["applied"], false);
        assert!(replicate["seq"].as_u64().is_some());

        let detach = client
            .identity_force_detach_authority_json(&session, root, 7, "authority unreachable")
            .expect("detach");
        let detach: serde_json::Value = serde_json::from_str(&detach).expect("detach json");
        assert_eq!(detach["detach"]["new_authority"], root.to_string().as_str());
        assert_eq!(detach["detach"]["generation"], 7);

        let configured = client
            .identity_configure_authority_replication_json(
                &session,
                "primary",
                source_path.to_str().expect("source path"),
                false,
                true,
                Some(250),
                5,
                60_000,
                true,
            )
            .expect("configure");
        let configured: serde_json::Value =
            serde_json::from_str(&configured).expect("configure json");
        assert_eq!(configured["policy"]["id"], "primary");
        assert_eq!(configured["policy"]["interval_ms"], 250);

        client.close(&session);
        let reopened = client.open().expect("reopen destination");
        let stored_policy = client
            .with_session(&reopened, |loom| {
                loom.store().authority_replication_policy_by_id("primary")
            })
            .expect("policy lookup")
            .expect("policy persisted");
        assert_eq!(stored_policy.id, "primary");
        assert_eq!(
            stored_policy.source,
            source_path.to_str().expect("source path")
        );
        assert_eq!(stored_policy.interval_ms, Some(250));
        client.close(&reopened);
        let session = client.open().expect("reopen for remove");

        let removed = client
            .identity_remove_authority_replication_json(&session, "primary")
            .expect("remove");
        let removed: serde_json::Value = serde_json::from_str(&removed).expect("remove json");
        assert_eq!(removed["id"], "primary");
        assert!(
            client
                .with_session(&session, |loom| {
                    loom.store().authority_replication_policy_by_id("primary")
                })
                .expect("policy lookup")
                .is_none()
        );
        let audit = client
            .with_session(&session, |loom| loom.store().audit_records())
            .expect("audit records");
        assert_eq!(audit[0].action, "identity.authority.replicate");
        assert_eq!(audit[1].action, "identity.authority.force_detach");
        assert_eq!(audit[2].action, "authority.replication.configure");
        assert_eq!(audit[3].action, "authority.replication.remove");

        client.close(&session);
        source.close(&source_session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn identity_authority_policy_denies_before_parsing_attacker_values() {
        use loom_core::identity::IdentityStore;
        let dir = temp_dir("identity-authority-policy-auth");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open destination");
        let root = mint_workspace_id().expect("root id");
        client
            .with_session(&session, |loom| {
                let mut identity = IdentityStore::new(root);
                identity.set_passphrase(root, "rootpw", b"root-salt-bytes")?;
                loom.store().save_identity_store(&identity)?;
                loom.set_identity_store(identity);
                let mut acl = AclStore::new();
                acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])?;
                loom.store().save_acl_store(&acl)?;
                loom.set_acl_store(acl);
                Ok(())
            })
            .expect("seed authenticated identity");

        let err = client
            .identity_configure_authority_replication_json(
                &session,
                "not allowed",
                "",
                false,
                true,
                Some(0),
                0,
                0,
                true,
            )
            .expect_err("unauthenticated configure must fail before validation");
        assert_eq!(err.code, Code::AuthenticationFailed);
        assert!(
            client
                .with_session(&session, |loom| loom.store().audit_records())
                .expect("audit records")
                .is_empty()
        );

        client
            .authenticate_passphrase(&session, root, b"rootpw")
            .expect("authenticate root");
        assert!(matches!(
            client.identity_configure_authority_replication_json(
                &session,
                "not allowed",
                "",
                false,
                true,
                Some(0),
                0,
                0,
                true,
            ),
            Err(e) if e.code == Code::InvalidArgument
        ));

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn identity_app_credential_mint_returns_secret_once_and_redacts() {
        use loom_core::identity::IdentityStore;
        let dir = temp_dir("appcred");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");
        let root = mint_workspace_id().expect("root id");
        client
            .with_session(&session, |loom| {
                let mut identity = IdentityStore::new(root);
                identity.set_passphrase(root, "rootpw", b"root-salt-bytes")?;
                loom.store().save_identity_store(&identity)
            })
            .expect("seed identity");
        client
            .authenticate_passphrase(&session, root, b"rootpw")
            .expect("authenticate root");

        let created = client
            .identity_create_app_credential(&session, root, "ci-runner")
            .expect("create app credential");
        assert!(
            created.secret_token.starts_with("loom_app_"),
            "token: {}",
            created.secret_token
        );
        assert_eq!(created.principal, root);
        assert_eq!(created.label, "ci-runner");
        assert!(created.enabled);
        assert_eq!(created.audit_seq, 0);

        // The plaintext secret is delivered only in the create result; the raw secret bytes recovered from
        // the token are absent from the persisted identity (the store keeps only the salted verifier).
        let secret_hex = created
            .secret_token
            .rsplit('_')
            .next()
            .expect("token secret");
        let secret: Vec<u8> = (0..secret_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&secret_hex[i..i + 2], 16).expect("hex"))
            .collect();
        client
            .with_session(&session, |loom| {
                let bytes = loom
                    .store()
                    .identity_store()?
                    .expect("identity present")
                    .encode();
                assert!(
                    !bytes.windows(secret.len()).any(|w| w == secret.as_slice()),
                    "plaintext secret persisted in identity store"
                );
                Ok(())
            })
            .expect("inspect persisted identity");

        // Revoke is audited and carries no secret material.
        let revoked = client
            .identity_revoke_app_credential(&session, created.id)
            .expect("revoke app credential");
        assert_eq!(revoked.id, Some(created.id));
        assert_eq!(revoked.action, "identity.app_credential.revoke");
        assert!(revoked.audit_seq > created.audit_seq);
        assert!(!revoked.target.as_deref().unwrap_or("").contains(secret_hex));

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn keysource_group_rejects_unencrypted_store() {
        let dir = temp_dir("keysource");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        // The default store is unencrypted, so at-rest wrap management is unsupported.
        assert!(matches!(
            client.key_add_wrap_keyed(&session, b"pw", vec![0u8; 16], vec![0u8; 12], false),
            Err(e) if e.code == Code::Unsupported
        ));
        assert!(matches!(
            client.key_add_wrap_with_kek(&session, [0u8; KEY_LEN], vec![0u8; 16], vec![0u8; 12], false),
            Err(e) if e.code == Code::Unsupported
        ));
        assert!(matches!(
            client.key_remove_wrap(&session, 0, false),
            Err(e) if e.code == Code::Unsupported
        ));

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_reports_capabilities_and_runtime_profile() {
        let client = LocalLoomClient::new("unused.loom");
        assert!(!client.store_capabilities().is_empty());
        let profile = client.store_runtime_profile();
        assert!(!profile.binary_channel.is_empty());
        assert!(!profile.crypto_provider.is_empty());
    }

    #[test]
    fn store_digest_algo_obeys_fips_required_policy() {
        let dir = temp_dir("digest-fips-policy");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client
            .create_store("fips", None, None)
            .expect("create fips-profile store");
        let session = client.open().expect("open fips-profile store");
        client
            .store_policy_set(
                &session,
                &loom_wire::store_admin::store_policy_update_to_cbor(
                    &loom_wire::store_admin::StorePolicyUpdate {
                        fips_required: Some(true),
                        default_durability: None,
                        facet_durability_assignments: Vec::new(),
                        clear_facet_durability: Vec::new(),
                    },
                ),
            )
            .expect("mark fips required");
        client.close(&session);

        let result = client.store_digest_algo();
        if loom_core::runtime_profile().fips_capable {
            assert_eq!(result.expect("digest algo"), "sha256");
        } else {
            let err = result.expect_err("non-FIPS runtime rejects fips_required metadata");
            assert_eq!(err.code, Code::PermissionDenied);
            assert!(err.message.contains("FIPS-required"));
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sql_group_exec_query_commit_roundtrips() {
        let dir = temp_dir("sql");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");

        // Path-bound: no parent LoomSession; the client is already bound to one store.
        let sql = client.sql_open("app", "main").expect("sql open");
        client
            .sql_exec(&sql, "CREATE TABLE items (id INTEGER, name TEXT);")
            .expect("create table");
        client
            .sql_exec(&sql, "INSERT INTO items VALUES (1, 'alpha'), (2, 'beta');")
            .expect("insert rows");
        let rows = client
            .sql_query(&sql, "SELECT id, name FROM items ORDER BY id;")
            .expect("query");
        assert_eq!(rows.len(), 2);
        let digest = client
            .sql_commit(&sql, "seed items", "tester", 1_000)
            .expect("commit");
        assert!(!digest.to_string().is_empty(), "commit returns a digest");
        assert!(client.sql_close(&sql));
        assert!(!client.sql_close(&sql), "second close is idempotent");

        // A BEGIN without a matching COMMIT in one exec is rejected (use a SqlBatch instead).
        let sql2 = client.sql_open("app", "main").expect("reopen");
        assert_eq!(
            client
                .sql_exec(&sql2, "BEGIN;")
                .expect_err("dangling transaction")
                .code,
            Code::InvalidArgument
        );

        // Reopen and confirm the rows persisted (each exec is an atomic save).
        let counted = client
            .sql_query(&sql2, "SELECT COUNT(*) FROM items;")
            .expect("count");
        assert_eq!(counted.len(), 1);
        client.sql_close(&sql2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sql_batch_commits_atomically_and_iterates_rows() {
        let dir = temp_dir("sqlbatch");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");

        let batch = client.sql_batch_begin("app", "main").expect("begin");
        client
            .sql_batch_exec(&batch, "CREATE TABLE t (id INTEGER);")
            .expect("create");
        client
            .sql_batch_exec(&batch, "INSERT INTO t VALUES (1), (2);")
            .expect("insert");
        // Abort reverts the un-persisted changes (durable state is still empty).
        client.sql_batch_abort(&batch).expect("abort");
        client
            .sql_batch_exec(&batch, "CREATE TABLE t (id INTEGER);")
            .expect("recreate after abort");
        client
            .sql_batch_exec(&batch, "INSERT INTO t VALUES (7);")
            .expect("insert after abort");
        client.sql_batch_commit(&batch).expect("commit");
        assert!(client.sql_batch_close(&batch));
        assert!(
            !client.sql_batch_close(&batch),
            "second close is idempotent"
        );

        // A batch left with an open SQL transaction cannot be committed.
        let dangling = client.sql_batch_begin("app", "main").expect("begin2");
        client
            .sql_batch_exec(&dangling, "BEGIN;")
            .expect("open txn");
        assert_eq!(
            client
                .sql_batch_commit(&dangling)
                .expect_err("open txn rejected")
                .code,
            Code::InvalidArgument
        );
        client.sql_batch_close(&dangling);

        // The committed row (only the post-abort insert) is visible through a RowIter.
        let sql = client.sql_open("app", "main").expect("open");
        let iter = client
            .sql_query_open(&sql, "SELECT id FROM t ORDER BY id;")
            .expect("iter");
        let mut rows = Vec::new();
        while let Some(row) = client.iter_next(&iter).expect("next") {
            rows.push(row);
        }
        assert_eq!(
            rows.len(),
            1,
            "only the committed (post-abort) row persists"
        );
        assert!(client.iter_free(&iter));
        assert!(!client.iter_free(&iter), "second free is idempotent");
        client.sql_close(&sql);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sql_auth_threading_and_keyed_validation() {
        let dir = temp_dir("sqlauth");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");

        // On an unauthenticated-root store the principal binding is a no-op, but it exercises the auth
        // threading (open -> attach_local_auth -> exec) end to end without breaking the session.
        let principal = mint_workspace_id().expect("principal");
        let sql = client
            .sql_open_authenticated("app", "main", principal, b"pw")
            .expect("open authenticated");
        client
            .sql_exec(&sql, "CREATE TABLE t (id INTEGER);")
            .expect("create");
        client
            .sql_authenticate_passphrase(&sql, principal, b"pw2")
            .expect("re-authenticate");
        client
            .sql_exec(&sql, "INSERT INTO t VALUES (1);")
            .expect("insert after auth");
        assert_eq!(
            client
                .sql_query(&sql, "SELECT id FROM t;")
                .expect("query")
                .len(),
            1
        );
        client.sql_close(&sql);

        // A batch also threads auth through its held write loom.
        let batch = client
            .sql_batch_begin_authenticated("app", "main", principal, b"pw")
            .expect("begin authenticated");
        client
            .sql_batch_exec(&batch, "INSERT INTO t VALUES (2);")
            .expect("batch insert");
        client.sql_batch_commit(&batch).expect("batch commit");
        client.sql_batch_close(&batch);

        // A non-utf-8 unlock passphrase is rejected before any store work.
        assert_eq!(
            client
                .sql_open_keyed("app", "main", &[0xff, 0xff])
                .expect_err("invalid utf-8 passphrase")
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            client
                .sql_batch_begin_keyed("app", "main", &[0xff])
                .expect_err("invalid utf-8 passphrase")
                .code,
            Code::InvalidArgument
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sql_direct_readers_and_list_databases() {
        let dir = temp_dir("sqlread");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");

        let sql = client.sql_open("app", "main").expect("sql open");
        client
            .sql_exec(&sql, "CREATE TABLE items (id INTEGER, name TEXT);")
            .expect("create");
        client
            .sql_exec(&sql, "INSERT INTO items VALUES (1, 'alpha');")
            .expect("insert alpha");
        let c1 = client
            .sql_commit(&sql, "c1", "tester", 1_000)
            .expect("commit c1");
        client
            .sql_exec(&sql, "INSERT INTO items VALUES (2, 'beta');")
            .expect("insert beta");
        let c2 = client
            .sql_commit(&sql, "c2", "tester", 2_000)
            .expect("commit c2");
        client.sql_close(&sql);

        let session = client.open().expect("open session");
        // SQL exec stages each table at the reserved SQL-facet path `<facet>/tables/<name>`; the direct
        // readers take that tabular path (mirroring the FFI `loom.read_table(ns, table)` contract).
        let items = format!(
            "{}/tables/items",
            loom_core::workspace::facet_path(FacetKind::Sql, "main")
        );

        // read_table reads the current staged table (both rows).
        let current = client
            .sql_read_table(&session, "app", &items)
            .expect("read table");
        let current = client.result_to_json(&current).expect("json");
        assert!(
            current.contains("alpha") && current.contains("beta"),
            "{current}"
        );

        // read_table_at reads a historical commit (only the first row).
        let at = client
            .sql_read_table_at(&session, "app", &items, &c1.to_string())
            .expect("read table at");
        let at = client.result_to_json(&at).expect("json");
        assert!(at.contains("alpha") && !at.contains("beta"), "{at}");

        // Row-level and schema-aware diffs between the two commits show the added row.
        let diff = client
            .sql_diff(&session, "app", &items, &c1.to_string(), &c2.to_string())
            .expect("diff");
        assert!(
            client
                .result_to_json(&diff)
                .expect("json")
                .contains("added")
        );
        let table_diff = client
            .sql_table_diff(&session, "app", &items, &c1.to_string(), &c2.to_string())
            .expect("table diff");
        assert!(
            client
                .result_to_json(&table_diff)
                .expect("json")
                .contains("added")
        );

        // Blame reports each current row with the commit that set it.
        let blame = client
            .sql_blame(&session, "app", "main", &items)
            .expect("blame");
        assert!(
            client
                .result_to_json(&blame)
                .expect("json")
                .contains("blake3")
        );

        // list_databases is a canonical-CBOR array of the workspace's SQL database names.
        let dbs = client
            .sql_list_databases(&session, "app")
            .expect("list dbs");
        let loom_codec::Value::Array(names) = loom_codec::decode(&dbs).unwrap() else {
            panic!("list_databases is not a CBOR array");
        };
        assert!(
            names
                .iter()
                .any(|v| matches!(v, loom_codec::Value::Text(t) if t == "main")),
            "expected database 'main' in {names:?}"
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sql_task_model_runs_async_ops() {
        fn status_text(bytes: &[u8]) -> String {
            match loom_codec::decode(bytes).unwrap() {
                loom_codec::Value::Text(s) => s,
                other => panic!("status is not text: {other:?}"),
            }
        }

        let dir = temp_dir("sqltask");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let sql = client.sql_open("app", "main").expect("sql open");

        // sql_exec_async is pending until polled, then ready; the result is the exec payload.
        let task = client
            .sql_exec_async(&sql, "CREATE TABLE t (id INTEGER);")
            .expect("exec async");
        assert_eq!(
            status_text(&client.task_status(&task).unwrap().expect("status")),
            "pending"
        );
        assert!(client.task_poll(&task).expect("poll"), "task is terminal");
        assert_eq!(
            status_text(&client.task_status(&task).unwrap().unwrap()),
            "ready"
        );
        client.task_result(&task).expect("result");
        assert_eq!(
            status_text(&client.task_status(&task).unwrap().unwrap()),
            "taken"
        );
        assert_eq!(
            client.task_result(&task).expect_err("already taken").code,
            Code::InvalidArgument
        );
        assert!(client.task_free(&task));
        assert!(!client.task_free(&task), "second free is idempotent");

        // task_wait polls to completion and takes the result in one call.
        let insert = client
            .sql_exec_async(&sql, "INSERT INTO t VALUES (1), (2);")
            .expect("insert async");
        client.task_wait(&insert).expect("wait");
        client.task_free(&insert);

        // A reader async form resolves through a LoomSession and yields the table payload.
        let session = client.open().expect("session");
        let items = format!(
            "{}/tables/t",
            loom_core::workspace::facet_path(FacetKind::Sql, "main")
        );
        let read = client
            .sql_read_table_async(&session, "app", &items)
            .expect("read async");
        let bytes = client.task_wait(&read).expect("wait read");
        assert!(
            client
                .result_to_json(&bytes)
                .expect("json")
                .contains("Rows")
        );
        client.task_free(&read);

        // A pending task can be cancelled; its result then errors and a poll leaves it terminal.
        let cancelled = client.sql_exec_async(&sql, "SELECT 1;").expect("async");
        client.task_cancel(&cancelled).expect("cancel");
        assert_eq!(
            status_text(&client.task_status(&cancelled).unwrap().unwrap()),
            "cancelled"
        );
        assert_eq!(
            client.task_result(&cancelled).expect_err("cancelled").code,
            Code::InvalidArgument
        );
        assert!(client.task_poll(&cancelled).expect("poll cancelled"));
        client.task_free(&cancelled);

        client.sql_close(&sql);
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn diagnostics_and_result_views_decode_a_sql_result() {
        let dir = temp_dir("diag");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let sql = client.sql_open("app", "main").expect("sql open");
        client
            .sql_exec(&sql, "CREATE TABLE items (id INTEGER, name TEXT);")
            .expect("create");
        client
            .sql_exec(&sql, "INSERT INTO items VALUES (1, 'alpha');")
            .expect("insert");
        let result = client
            .sql_exec(&sql, "SELECT id, name FROM items;")
            .expect("select");

        // Diagnostics: JSON and bridge-JSON rendering of the held result buffer.
        assert!(
            client
                .result_to_json(&result)
                .expect("json")
                .contains("alpha")
        );
        client.result_to_bridge_json(&result).expect("bridge json");
        assert!(client.last_error().is_none());

        // ResultViews: an indexed, typed view of the same buffer.
        let view = client.result_open(&result).expect("result open");
        assert!(view.is_statements());
        assert_eq!(view.len(), 1);
        assert_eq!(view.item_kind(0), Some("select"));
        assert_eq!(view.column_count(), 2);
        assert!(view.column_name(0).is_some());
        assert_eq!(view.row_count(), 1);
        assert_eq!(view.row_len(0), 2);
        assert!(view.cell(0, 1).is_some());

        // A decode failure is surfaced and recorded in last_error.
        assert!(client.result_to_json(b"not-canonical-cbor").is_err());
        assert!(client.last_error().is_some());

        client.sql_close(&sql);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn decode_arr(bytes: &[u8]) -> Vec<loom_codec::Value> {
        match loom_codec::decode(bytes).expect("decode canonical CBOR") {
            loom_codec::Value::Array(items) => items,
            other => panic!("expected a CBOR array, got {other:?}"),
        }
    }

    fn uint(v: &loom_codec::Value) -> u64 {
        match v {
            loom_codec::Value::Uint(n) => *n,
            other => panic!("expected a CBOR uint, got {other:?}"),
        }
    }

    fn text(v: &loom_codec::Value) -> String {
        match v {
            loom_codec::Value::Text(s) => s.clone(),
            other => panic!("expected CBOR text, got {other:?}"),
        }
    }

    #[test]
    fn filesystem_import_export_roundtrips_sync_and_async() {
        let dir = temp_dir("fs-io");
        let src = dir.join("src");
        std::fs::create_dir_all(&src).expect("src dir");
        std::fs::write(src.join("hello.txt"), b"hi there").expect("write source");
        let src_str = src.to_str().unwrap();

        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create");
        let session = client.open().expect("open");

        // Sync import: 8 bytes in, at least one operation applied.
        let report = loom_interchange::ImportReport::decode(
            &client
                .import_fs(&session, "w", src_str, None, None, true, false)
                .expect("import_fs"),
        )
        .expect("decode import report");
        assert_eq!(report.source_scope, src_str);
        assert_eq!(report.bytes_in, 8);
        assert!(report.operations_applied >= 1);

        // A dry run plans without applying and flags itself.
        let dry = loom_interchange::ImportReport::decode(
            &client
                .import_fs(&session, "w-dry", src_str, None, None, false, true)
                .expect("dry import"),
        )
        .expect("decode dry-run import report");
        assert_eq!(dry.operations_applied, 0);
        assert!(dry.dry_run);

        // Sync export writes the file back to the host with identical bytes.
        let out = dir.join("out");
        let export = decode_arr(
            &client
                .export_fs(&session, "w", out.to_str().unwrap(), None, false)
                .expect("export_fs"),
        );
        assert!(uint(&export[2]) >= 1, "files_written");
        assert_eq!(
            std::fs::read(out.join("hello.txt")).expect("exported file"),
            b"hi there"
        );

        // The async form is an immediate-complete task with the same report bytes.
        let task = client.import_fs_async(&session, "w-async", src_str, None, None, false, false);
        assert!(client.task_poll(&task).expect("poll"));
        let async_report =
            loom_interchange::ImportReport::decode(&client.task_result(&task).expect("result"))
                .expect("decode async import report");
        assert_eq!(async_report.source_scope, src_str);
        assert_eq!(async_report.bytes_in, 8);

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn filesystem_directory_and_stat_surface_round_trips() {
        use loom_core::FileKind;
        let dir = temp_dir("fs-dir");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create");
        let session = client.open().expect("open");

        // create_directory then write a file inside it; stat reports the file.
        client
            .create_directory(&session, "w", "docs", false)
            .expect("mkdir");
        client
            .write_file(&session, "w", "docs/readme.txt", b"hello", 0o100644)
            .expect("write file");

        let stat = loom_wire::fs::fs_stat_from_cbor(
            &client.stat(&session, "w", "docs/readme.txt").expect("stat"),
        )
        .expect("decode stat");
        assert_eq!(stat.path, "docs/readme.txt");
        assert_eq!(stat.kind, FileKind::File);
        assert_eq!(stat.size, 5);

        let dir_stat = loom_wire::fs::fs_stat_from_cbor(
            &client.stat(&session, "w", "docs").expect("stat dir"),
        )
        .expect("decode dir stat");
        assert_eq!(dir_stat.kind, FileKind::Directory);

        // list_directory returns the child, sorted by name.
        let entries = loom_wire::fs::dir_listing_from_cbor(
            &client.list_directory(&session, "w", "docs").expect("ls"),
        )
        .expect("decode listing");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "readme.txt");
        assert_eq!(entries[0].kind, FileKind::File);

        // A non-empty directory needs recursive removal; an empty one does not.
        assert!(
            client
                .remove_directory(&session, "w", "docs", false)
                .is_err(),
            "non-empty dir without recursive must error"
        );
        client
            .remove_directory(&session, "w", "docs", true)
            .expect("recursive rmdir");
        assert!(
            client.stat(&session, "w", "docs").is_err(),
            "removed dir must be NOT_FOUND"
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn archive_export_import_roundtrips_sync_and_async() {
        let dir = temp_dir("archive-io");
        let src = dir.join("src");
        std::fs::create_dir_all(&src).expect("src dir");
        std::fs::write(src.join("a.txt"), b"alpha").expect("write source");

        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create");
        let session = client.open().expect("open");
        client
            .import_fs(
                &session,
                "w",
                src.to_str().unwrap(),
                None,
                None,
                true,
                false,
            )
            .expect("seed import");

        // Sync export to a plain tar archive; the manifest reports the kind and entries.
        let archive = dir.join("w.tar");
        let export = decode_arr(
            &client
                .archive_export(&session, "w", archive.to_str().unwrap(), "tar", None, false)
                .expect("archive_export"),
        );
        let manifest = match &export[0] {
            loom_codec::Value::Array(items) => items.clone(),
            other => panic!("manifest not an array: {other:?}"),
        };
        assert_eq!(text(&manifest[1]), "tar", "kind");
        let export_entries = match &manifest[3] {
            loom_codec::Value::Array(items) => items,
            other => panic!("export manifest entries not an array: {other:?}"),
        };
        assert!(!export_entries.is_empty(), "entries");
        assert!(archive.is_file(), "archive written to host");

        // Sync import back into a fresh workspace preserves the kind.
        let import = decode_arr(
            &client
                .archive_import(
                    &session,
                    "w2",
                    archive.to_str().unwrap(),
                    "tar",
                    None,
                    true,
                    Some("archive-author"),
                    Some("archive message"),
                    false,
                )
                .expect("archive_import"),
        );
        let in_manifest = match &import[0] {
            loom_codec::Value::Array(items) => items.clone(),
            other => panic!("manifest not an array: {other:?}"),
        };
        assert_eq!(text(&in_manifest[1]), "tar");
        let in_entries = match &in_manifest[3] {
            loom_codec::Value::Array(items) => items,
            other => panic!("manifest entries not an array: {other:?}"),
        };
        assert!(!in_entries.is_empty(), "detailed manifest entries");
        let in_report =
            loom_interchange::ImportReport::from_value(import[1].clone()).expect("import report");
        assert_eq!(
            in_report.source_scope,
            archive.to_str().unwrap(),
            "archive import source_scope"
        );
        assert!(in_report.commit.is_some(), "archive import commit");

        let task = client.archive_import_async(
            &session,
            "w3",
            archive.to_str().unwrap(),
            "tar",
            None,
            false,
            None,
            None,
            false,
        );
        assert!(client.task_poll(&task).expect("poll archive import"));
        let async_import = decode_arr(&client.task_result(&task).expect("archive import result"));
        let async_manifest = match &async_import[0] {
            loom_codec::Value::Array(items) => items,
            other => panic!("async manifest not an array: {other:?}"),
        };
        assert!(matches!(async_manifest[3], loom_codec::Value::Array(_)));
        let async_report = loom_interchange::ImportReport::from_value(async_import[1].clone())
            .expect("async import report");
        assert_eq!(
            async_report.source_scope,
            archive.to_str().unwrap(),
            "async archive import source_scope"
        );

        // An unknown archive kind is rejected before touching the host.
        assert!(
            client
                .archive_export(&session, "w", archive.to_str().unwrap(), "rar", None, false)
                .is_err()
        );

        // The async form is an immediate-complete task with a well-formed result.
        let task = client.archive_export_async(
            &session,
            "w",
            dir.join("w2.tar").to_str().unwrap(),
            "tar",
            None,
            false,
        );
        assert!(client.task_poll(&task).expect("poll"));
        let _ = decode_arr(&client.task_result(&task).expect("result"));

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn security_administration_generated_contracts_persist_and_audit() {
        let dir = temp_dir("security-admin");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create");
        let session = client.open().expect("open");

        let audit_set: serde_json::Value = serde_json::from_str(
            &client
                .audit_config_set_json(&session, Some(30), Some(true))
                .expect("audit config set"),
        )
        .expect("audit set json");
        assert_eq!(audit_set["config"]["retention_days"], 30);
        assert_eq!(audit_set["config"]["legal_hold"], true);

        let rules = r#"[{"id":"local","action":"allow","source_cidr":"127.0.0.1/32","require_mtls":false}]"#;
        let network_set: serde_json::Value = serde_json::from_str(
            &client
                .network_access_set_json(&session, "office", Some("office network"), "deny", rules)
                .expect("network access set"),
        )
        .expect("network set json");
        assert_eq!(network_set["name"], "office");
        assert_eq!(network_set["rules"][0]["source_cidr"], "127.0.0.1/32");

        let cert: serde_json::Value = serde_json::from_str(
            &client
                .certificate_generate_self_signed_json(
                    &session,
                    "admin",
                    vec!["localhost".to_string()],
                    Vec::new(),
                    None,
                    1,
                    "p256",
                    true,
                )
                .expect("generate certificate"),
        )
        .expect("certificate json");
        assert_eq!(cert["name"], "admin");
        assert_eq!(cert["unencrypted_private_key_override"], true);

        let exported = client
            .certificate_export(&session, "admin", true, false, false, false)
            .expect("certificate export");
        let loom_codec::Value::Array(fields) =
            loom_codec::decode(&exported).expect("certificate export cbor")
        else {
            panic!("certificate export must be an array");
        };
        assert!(matches!(fields[2], loom_codec::Value::Bytes(_)));
        assert!(matches!(fields[3], loom_codec::Value::Null));

        client.close(&session);
        let reopened = client.open().expect("reopen");
        let audit_show: serde_json::Value = serde_json::from_str(
            &client
                .audit_config_show_json(&reopened)
                .expect("audit config show"),
        )
        .expect("audit show json");
        assert_eq!(audit_show["retention_days"], 30);
        assert_eq!(audit_show["legal_hold"], true);

        let certificates: serde_json::Value = serde_json::from_str(
            &client
                .certificate_list_json(&reopened)
                .expect("certificate list"),
        )
        .expect("certificate list json");
        assert_eq!(certificates["certificates"][0]["name"], "admin");

        let policies: serde_json::Value = serde_json::from_str(
            &client
                .network_access_list_json(&reopened)
                .expect("network access list"),
        )
        .expect("network list json");
        assert_eq!(policies["policies"][0]["name"], "office");

        let audit_list: serde_json::Value =
            serde_json::from_str(&client.audit_list_json(&reopened).expect("audit list"))
                .expect("audit list json");
        let actions = audit_list["records"]
            .as_array()
            .expect("audit records")
            .iter()
            .filter_map(|record| record["action"].as_str())
            .collect::<Vec<_>>();
        assert!(actions.contains(&"audit.config.set"));
        assert!(actions.contains(&"network-access.policy.set"));
        assert!(actions.contains(&"certificate.bundle.generate_self_signed.force"));

        client.close(&reopened);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn security_admin_rejects_untrusted_inputs_before_session_and_admin_authorization() {
        use loom_core::identity::IdentityStore;

        let dir = temp_dir("security-admin-auth-first");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create");
        let session = client.open().expect("open");
        let mut unknown_id = session.0.clone();
        unknown_id.id = 999_999u64.to_be_bytes().to_vec();
        let unknown = LoomSession(unknown_id);

        assert!(matches!(
            client.certificate_import_json(
                &unknown,
                "bad",
                b"not a certificate".to_vec(),
                b"not a private key".to_vec(),
                None,
                false,
            ),
            Err(err) if err.code == Code::NotFound
        ));
        assert!(matches!(
            client.certificate_generate_self_signed_json(
                &unknown,
                "bad",
                Vec::new(),
                Vec::new(),
                None,
                0,
                "not-an-algorithm",
                false,
            ),
            Err(err) if err.code == Code::NotFound
        ));
        assert!(matches!(
            client.network_access_set_json(&unknown, "bad", None, "not-an-action", "not-json"),
            Err(err) if err.code == Code::NotFound
        ));

        let root = mint_workspace_id().expect("root id");
        client
            .with_session(&session, |loom| {
                let mut identity = IdentityStore::new(root);
                identity.set_passphrase(root, "rootpw", b"root-salt-bytes")?;
                loom.store().save_identity_store(&identity)?;
                loom.set_identity_store(identity);
                Ok(())
            })
            .expect("seed identity");
        client
            .authenticate_passphrase(&session, root, b"rootpw")
            .expect("authenticate root");

        assert!(matches!(
            client.certificate_import_json(
                &session,
                "bad",
                b"not a certificate".to_vec(),
                b"not a private key".to_vec(),
                None,
                false,
            ),
            Err(err) if err.code == Code::PermissionDenied
        ));
        assert!(matches!(
            client.certificate_generate_self_signed_json(
                &session,
                "bad",
                Vec::new(),
                Vec::new(),
                None,
                0,
                "not-an-algorithm",
                false,
            ),
            Err(err) if err.code == Code::PermissionDenied
        ));
        assert!(matches!(
            client.network_access_set_json(&session, "bad", None, "not-an-action", "not-json"),
            Err(err) if err.code == Code::PermissionDenied
        ));

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn car_export_import_roundtrips_sync_and_async() {
        let dir = temp_dir("car-io");
        let src = dir.join("src");
        std::fs::create_dir_all(&src).expect("src dir");
        std::fs::write(src.join("c.txt"), b"gamma").expect("write source");

        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create");
        let session = client.open().expect("open");
        client
            .import_fs(
                &session,
                "w",
                src.to_str().unwrap(),
                None,
                None,
                false,
                false,
            )
            .expect("seed import");

        // Sync export to a CAR file; the result carries a root CID and block count.
        let car = dir.join("w.car");
        let export = decode_arr(
            &client
                .car_export(&session, "w", car.to_str().unwrap(), false)
                .expect("car_export"),
        );
        let root_cid = text(&export[0]);
        assert!(!root_cid.is_empty(), "root cid");
        assert!(uint(&export[1]) >= 1, "blocks_written");
        assert!(car.is_file(), "car written to host");

        // Sync import is store-wide: importing into a fresh store restores the blocks and
        // reports the same root CID the export produced.
        let other = LocalLoomClient::new(dir.join("other.loom"));
        other.create().expect("create other store");
        let other_session = other.open().expect("open other");
        let import = decode_arr(
            &other
                .car_import(&other_session, car.to_str().unwrap(), false)
                .expect("car_import"),
        );
        assert_eq!(
            text(&import[1]),
            root_cid,
            "import reports the same root cid"
        );
        assert!(uint(&import[2]) >= 1, "blocks_read");
        other.close(&other_session);

        // The async form is an immediate-complete task with a well-formed result.
        let task =
            client.car_export_async(&session, "w", dir.join("w2.car").to_str().unwrap(), false);
        assert!(client.task_poll(&task).expect("poll"));
        let _ = decode_arr(&client.task_result(&task).expect("result"));

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn metrics_descriptor_observation_and_query_roundtrip() {
        let dir = temp_dir("metrics");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create");
        let session = client.open().expect("open");

        let descriptor = MetricDescriptor::new(
            "requests".into(),
            String::new(),
            "1".into(),
            loom_core::MetricInstrumentKind::Counter,
            loom_core::MetricTemporality::Cumulative,
            vec!["method".into()],
            64,
            30_000,
        )
        .expect("descriptor");
        let descriptor_bytes = descriptor.encode().expect("encode descriptor");

        // An absent descriptor reads as None.
        assert_eq!(
            client
                .metrics_get_descriptor(&session, "w", "requests")
                .expect("get absent"),
            None
        );

        // Put then read the descriptor back verbatim.
        client
            .metrics_put_descriptor(&session, "w", &descriptor_bytes)
            .expect("put descriptor");
        assert_eq!(
            client
                .metrics_get_descriptor(&session, "w", "requests")
                .expect("get descriptor"),
            Some(descriptor_bytes)
        );

        // Append one observation.
        let observation = MetricObservation::new(
            descriptor.digest().expect("digest"),
            std::collections::BTreeMap::from([("method".to_string(), "GET".to_string())]),
            1,
            1.0,
        )
        .expect("observation");
        client
            .metrics_put_observation(
                &session,
                "w",
                "requests",
                &observation.encode().expect("encode observation"),
            )
            .expect("put observation");

        // Query the half-open window [0, 10) yields `[observations, partial, stale]`.
        let cbor = client
            .metrics_query(&session, "w", "requests", 0, 10, 16, 16, 64, 65536, 100)
            .expect("query");
        let arr = decode_arr(&cbor);
        assert_eq!(arr.len(), 3, "[observations, partial, stale]");
        let observations = match &arr[0] {
            loom_codec::Value::Array(items) => items.clone(),
            other => panic!("observations not an array: {other:?}"),
        };
        assert_eq!(observations.len(), 1, "one observation in the window");
        assert_eq!(arr[1], loom_codec::Value::Bool(false), "not partial");
        assert_eq!(arr[2], loom_codec::Value::Bool(false), "not stale");

        // An absent workspace yields an empty, non-partial result.
        let empty = decode_arr(
            &client
                .metrics_query(
                    &session, "absent", "requests", 0, 10, 16, 16, 64, 65536, 100,
                )
                .expect("query absent"),
        );
        match &empty[0] {
            loom_codec::Value::Array(items) => assert!(items.is_empty(), "no observations"),
            other => panic!("observations not an array: {other:?}"),
        }

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn program_lifecycle_roundtrips_through_the_engine() {
        let dir = temp_dir("program-lifecycle");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create");
        let session = client.open().expect("open");

        assert_eq!(
            client
                .program_inspect(&session, "w", "page-card")
                .expect("inspect absent"),
            None
        );

        let source = "Hello, {{ name }}";
        let manifest =
            Manifest::for_template("page-card", source, loom_compute::GrantSet::default());
        let manifest_bytes = manifest.encode();
        let stored = decode_arr(
            &client
                .program_put(
                    &session,
                    "w",
                    "page-card",
                    &manifest_bytes,
                    source.as_bytes(),
                )
                .expect("put program"),
        );
        assert_eq!(text(&stored[0]), "page-card");
        assert_eq!(uint(&stored[3]), source.len() as u64);
        assert_eq!(stored[4], loom_codec::Value::Bytes(manifest_bytes.clone()));

        let inspected = client
            .program_inspect(&session, "w", "page-card")
            .expect("inspect")
            .expect("record");
        assert_eq!(decode_arr(&inspected), stored);

        let body = decode_arr(
            &client
                .program_get(&session, "w", "page-card")
                .expect("get")
                .expect("body"),
        );
        assert_eq!(body[0], loom_codec::Value::Array(stored.clone()));
        assert_eq!(
            body[1],
            loom_codec::Value::Bytes(source.as_bytes().to_vec())
        );

        let list = decode_arr(&client.program_list(&session, "w").expect("list"));
        assert_eq!(list, vec![loom_codec::Value::Array(stored)]);

        assert!(
            client
                .program_remove(&session, "w", "page-card")
                .expect("remove")
        );
        assert!(
            !client
                .program_remove(&session, "w", "page-card")
                .expect("remove absent")
        );
        assert_eq!(
            client
                .program_get(&session, "w", "page-card")
                .expect("get removed"),
            None
        );
        assert!(
            decode_arr(
                &client
                    .program_list(&session, "absent")
                    .expect("list absent")
            )
            .is_empty()
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn program_lifecycle_respects_authenticated_acl() {
        use loom_core::acl::{AclEffect, AclScope};
        use loom_core::identity::IdentityStore;
        use std::collections::BTreeSet;

        let dir = temp_dir("program-acl");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create");
        let session = client.open().expect("open");
        let root = mint_workspace_id().expect("root id");
        let user = mint_workspace_id().expect("user id");
        client
            .with_session(&session, |loom| {
                let mut identity = IdentityStore::new(root);
                identity.add_principal(user, "user", PrincipalKind::User)?;
                identity.set_passphrase(user, "pw", b"salt-bytes")?;
                loom.store().save_identity_store(&identity)?;
                loom.set_identity_store(identity);
                Ok(())
            })
            .expect("seed identity");

        let source = "Hello, {{ name }}";
        let manifest =
            Manifest::for_template("page-card", source, loom_compute::GrantSet::default());
        let manifest_bytes = manifest.encode();
        assert!(matches!(
            client.program_put(
                &session,
                "w",
                "page-card",
                &manifest_bytes,
                source.as_bytes()
            ),
            Err(e) if matches!(e.code, Code::AuthenticationFailed | Code::PermissionDenied)
        ));

        let grant = AclGrant {
            subject: AclSubject::Principal(user),
            workspace: None,
            domain: Some(FacetKind::Program.into()),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: BTreeSet::from([AclRight::Read, AclRight::Write]),
            effect: AclEffect::Allow,
            predicate: None,
        };
        client.acl_grant(&session, grant.clone()).expect("grant");
        client
            .authenticate_passphrase(&session, user, b"pw")
            .expect("authenticate");
        client
            .program_put(
                &session,
                "w",
                "page-card",
                &manifest_bytes,
                source.as_bytes(),
            )
            .expect("put after grant");
        assert!(
            client
                .program_inspect(&session, "w", "page-card")
                .expect("read after grant")
                .is_some()
        );

        assert!(client.acl_revoke(&session, &grant).expect("revoke"));
        assert!(matches!(
            client.program_inspect(&session, "w", "page-card"),
            Err(e) if e.code == Code::PermissionDenied
        ));

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn document_text_binary_roundtrip_and_errors() {
        let dir = temp_dir("doc-text");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create");
        let session = client.open().expect("open");

        let reference_targets = |client: &LocalLoomClient, session: &LoomSession| {
            client
                .with_session(session, |loom| {
                    let ns = read_ns(loom, "w", FacetKind::Document)?
                        .ok_or_else(|| LoomError::not_found("document workspace missing"))?;
                    let source = ReferenceSource::new("document", "c", "greeting", "body")?;
                    let mut targets = loom_reference::references_from(loom, ns, &source)?
                        .into_iter()
                        .map(|edge| edge.target.as_str())
                        .collect::<Vec<_>>();
                    targets.sort();
                    Ok(targets)
                })
                .expect("reference targets")
        };

        // Absent reads are None (no workspace yet).
        assert_eq!(
            client
                .document_get_text(&session, "w", "c", "missing")
                .expect("absent text"),
            None
        );
        assert_eq!(
            client
                .document_get_binary(&session, "w", "c", "missing")
                .expect("absent binary"),
            None
        );

        let before_generation = client
            .with_session(&session, |loom| loom.store().mutable_overlay_generation())
            .expect("document before generation");
        let before_fsync = client
            .with_session(&session, |loom| {
                Ok(loom.store().group_commit_diagnostics()?.fsync_count)
            })
            .expect("document before fsync");

        // Text write/read round-trips with the returned content digest.
        let d_text = client
            .document_put_text(&session, "w", "c", "greeting", "hello !ticket:DOC-1", None)
            .expect("put text");
        let after_generation = client
            .with_session(&session, |loom| loom.store().mutable_overlay_generation())
            .expect("document after generation");
        let after_fsync = client
            .with_session(&session, |loom| {
                Ok(loom.store().group_commit_diagnostics()?.fsync_count)
            })
            .expect("document after fsync");
        assert_eq!(after_generation.as_u64(), before_generation.as_u64() + 1);
        assert_eq!(after_fsync, before_fsync + 2);
        let (d_text, text_entity_tag) = loom_wire::document::put_result_from_cbor(&d_text).unwrap();
        assert!(d_text.starts_with("blake3:"), "digest: {d_text}");
        let text_cbor = client
            .document_get_text(&session, "w", "c", "greeting")
            .expect("get text")
            .expect("present");
        let (text, digest, get_text_entity_tag) =
            loom_wire::document::text_result_from_cbor(&text_cbor).unwrap();
        assert_eq!(text, "hello !ticket:DOC-1");
        assert_eq!(digest, d_text, "get_text digest matches put_text");
        assert_eq!(get_text_entity_tag, text_entity_tag);
        assert_eq!(reference_targets(&client, &session), vec!["ticket:DOC-1"]);

        // Binary write/read round-trips with the returned content digest.
        let before_binary_generation = after_generation;
        let before_binary_fsync = after_fsync;
        let d_bin = client
            .document_put_binary(&session, "w", "c", "blob", &[0xFF, 0xFE, 0x00], None)
            .expect("put binary");
        let after_binary_generation = client
            .with_session(&session, |loom| loom.store().mutable_overlay_generation())
            .expect("document after binary generation");
        let after_binary_fsync = client
            .with_session(&session, |loom| {
                Ok(loom.store().group_commit_diagnostics()?.fsync_count)
            })
            .expect("document after binary fsync");
        assert_eq!(
            after_binary_generation.as_u64(),
            before_binary_generation.as_u64() + 1
        );
        assert_eq!(after_binary_fsync, before_binary_fsync + 2);
        let (d_bin, bin_entity_tag) = loom_wire::document::put_result_from_cbor(&d_bin).unwrap();
        let bin_cbor = client
            .document_get_binary(&session, "w", "c", "blob")
            .expect("get binary")
            .expect("present");
        let (bytes, bdigest, get_bin_entity_tag) =
            loom_wire::document::binary_result_from_cbor(&bin_cbor).unwrap();
        assert_eq!(bytes, vec![0xFF, 0xFE, 0x00]);
        assert_eq!(bdigest, d_bin, "get_binary digest matches put_binary");
        assert_eq!(get_bin_entity_tag, bin_entity_tag);

        // The collection lists non-empty after the two writes.
        assert!(
            !client
                .document_list_binary(&session, "w", "c")
                .expect("list binary")
                .is_empty()
        );
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen document store");
        let text_cbor = reopened
            .document_get_text(&session, "w", "c", "greeting")
            .expect("reopen get text")
            .expect("reopen text present");
        let (text, digest, get_text_entity_tag) =
            loom_wire::document::text_result_from_cbor(&text_cbor).unwrap();
        assert_eq!(text, "hello !ticket:DOC-1");
        assert_eq!(digest, d_text, "reopen get_text digest matches put_text");
        assert_eq!(get_text_entity_tag, text_entity_tag);
        let bin_cbor = reopened
            .document_get_binary(&session, "w", "c", "blob")
            .expect("reopen get binary")
            .expect("reopen binary present");
        let (bytes, bdigest, get_bin_entity_tag) =
            loom_wire::document::binary_result_from_cbor(&bin_cbor).unwrap();
        assert_eq!(bytes, vec![0xFF, 0xFE, 0x00]);
        assert_eq!(
            bdigest, d_bin,
            "reopen get_binary digest matches put_binary"
        );
        assert_eq!(get_bin_entity_tag, bin_entity_tag);
        assert_eq!(reference_targets(&reopened, &session), vec!["ticket:DOC-1"]);

        // A stale/mismatched expected entity tag guard is a CAS_MISMATCH.
        let before_conflict_generation = reopened
            .with_session(&session, |loom| loom.store().mutable_overlay_generation())
            .expect("document before conflict generation");
        let before_conflict_fsync = reopened
            .with_session(&session, |loom| {
                Ok(loom.store().group_commit_diagnostics()?.fsync_count)
            })
            .expect("document before conflict fsync");
        let before_conflict_text = reopened
            .document_get_text(&session, "w", "c", "greeting")
            .expect("before conflict text");
        let before_conflict_list = reopened
            .document_list_binary(&session, "w", "c")
            .expect("before conflict list");
        let before_conflict_refs = reference_targets(&reopened, &session);
        let conflict = reopened
            .document_put_text(
                &session,
                "w",
                "c",
                "greeting",
                "again !ticket:DOC-2",
                Some(&bin_entity_tag),
            )
            .expect_err("cas conflict");
        assert_eq!(conflict.code, Code::Conflict);
        assert_eq!(
            reopened
                .with_session(&session, |loom| loom.store().mutable_overlay_generation())
                .expect("document after conflict generation"),
            before_conflict_generation
        );
        assert_eq!(
            reopened
                .with_session(&session, |loom| {
                    Ok(loom.store().group_commit_diagnostics()?.fsync_count)
                })
                .expect("document after conflict fsync"),
            before_conflict_fsync
        );
        assert_eq!(
            reopened
                .document_get_text(&session, "w", "c", "greeting")
                .expect("after conflict text"),
            before_conflict_text
        );
        assert_eq!(
            reopened
                .document_list_binary(&session, "w", "c")
                .expect("after conflict list"),
            before_conflict_list
        );
        assert_eq!(reference_targets(&reopened, &session), before_conflict_refs);

        // The matching current entity tag as the guard succeeds and produces a new digest.
        let d_text2 = reopened
            .document_put_text(
                &session,
                "w",
                "c",
                "greeting",
                "again",
                Some(&text_entity_tag),
            )
            .expect("cas ok");
        let (d_text2, _) = loom_wire::document::put_result_from_cbor(&d_text2).unwrap();
        assert_ne!(d_text2, d_text, "content changed, so the digest changed");

        // Reading non-UTF-8 bytes as text is DOCUMENT_NOT_TEXT.
        let not_text = reopened
            .document_get_text(&session, "w", "c", "blob")
            .expect_err("not text");
        assert_eq!(not_text.code, Code::DocumentNotText);

        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn document_put_planning_publication_failure_preserves_live_and_durable_state() {
        let dir = temp_dir("doc-put-publication-failure");
        let path = dir.join("t.loom");
        let client = LocalLoomClient::new(&path);
        client.create().expect("create");
        let session = client.open().expect("open");
        client
            .document_put_text(&session, "w", "c", "greeting", "stable !ticket:DOC-1", None)
            .expect("seed text");
        let before_generation = client
            .with_session(&session, |loom| loom.store().mutable_overlay_generation())
            .expect("before failure generation");
        let before_fsync = client
            .with_session(&session, |loom| {
                Ok(loom.store().group_commit_diagnostics()?.fsync_count)
            })
            .expect("before failure fsync");
        let before_text = client
            .document_get_text(&session, "w", "c", "greeting")
            .expect("before failure text");
        let before_list = client
            .document_list_binary(&session, "w", "c")
            .expect("before failure list");
        let before_refs = client
            .with_session(&session, |loom| {
                let ns = read_ns(loom, "w", FacetKind::Document)?
                    .ok_or_else(|| LoomError::not_found("document workspace missing"))?;
                let source = ReferenceSource::new("document", "c", "greeting", "body")?;
                let mut targets = loom_reference::references_from(loom, ns, &source)?
                    .into_iter()
                    .map(|edge| edge.target.as_str())
                    .collect::<Vec<_>>();
                targets.sort();
                Ok(targets)
            })
            .expect("before failure refs");

        install_exec_apply_candidate_save_hook(
            path.clone(),
            Box::new(|| {
                Err(LoomError::new(
                    Code::Internal,
                    "injected document put publication failure",
                ))
            }),
        );
        let error = client
            .document_put_text(
                &session,
                "w",
                "c",
                "greeting",
                "changed !ticket:DOC-2",
                None,
            )
            .expect_err("publication failure");
        assert_eq!(error.code, Code::Internal);
        assert_eq!(
            client
                .with_session(&session, |loom| loom.store().mutable_overlay_generation())
                .expect("after failure generation"),
            before_generation
        );
        assert_eq!(
            client
                .with_session(&session, |loom| {
                    Ok(loom.store().group_commit_diagnostics()?.fsync_count)
                })
                .expect("after failure fsync"),
            before_fsync
        );
        assert_eq!(
            client
                .document_get_text(&session, "w", "c", "greeting")
                .expect("after failure text"),
            before_text
        );
        assert_eq!(
            client
                .document_list_binary(&session, "w", "c")
                .expect("after failure list"),
            before_list
        );
        assert_eq!(
            client
                .with_session(&session, |loom| {
                    let ns = read_ns(loom, "w", FacetKind::Document)?
                        .ok_or_else(|| LoomError::not_found("document workspace missing"))?;
                    let source = ReferenceSource::new("document", "c", "greeting", "body")?;
                    let mut targets = loom_reference::references_from(loom, ns, &source)?
                        .into_iter()
                        .map(|edge| edge.target.as_str())
                        .collect::<Vec<_>>();
                    targets.sort();
                    Ok(targets)
                })
                .expect("after failure refs"),
            before_refs
        );
        client.close(&session);

        let reopened = LocalLoomClient::new(&path);
        let session = reopened.open().expect("reopen document store");
        assert_eq!(
            reopened
                .document_get_text(&session, "w", "c", "greeting")
                .expect("reopen after failure text"),
            before_text
        );
        assert_eq!(
            reopened
                .document_list_binary(&session, "w", "c")
                .expect("reopen after failure list"),
            before_list
        );
        assert_eq!(
            reopened
                .with_session(&session, |loom| {
                    let ns = read_ns(loom, "w", FacetKind::Document)?
                        .ok_or_else(|| LoomError::not_found("document workspace missing"))?;
                    let source = ReferenceSource::new("document", "c", "greeting", "body")?;
                    let mut targets = loom_reference::references_from(loom, ns, &source)?
                        .into_iter()
                        .map(|edge| edge.target.as_str())
                        .collect::<Vec<_>>();
                    targets.sort();
                    Ok(targets)
                })
                .expect("reopen after failure refs"),
            before_refs
        );
        reopened.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pim_search_vcs_growth_methods_are_wired() {
        let dir = temp_dir("growth");
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = client.open().expect("open session");

        // Contacts put_vcard imports a vCard into an existing book and is content-addressed
        // (identical input -> same etag).
        let meta = loom_core::contacts::BookMeta {
            display_name: "Personal".into(),
        };
        client
            .contacts_create_book(&session, "con", "alice", "personal", &meta.encode())
            .expect("create_book");
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:imported\r\nFN:Imported Person\r\nEMAIL:i@x.io\r\nEND:VCARD\r\n";
        let dv1 = client
            .contacts_put_vcard(&session, "con", "alice", "personal", vcard)
            .expect("put_vcard");
        let dv2 = client
            .contacts_put_vcard(&session, "con", "alice", "personal", vcard)
            .expect("put_vcard again");
        assert_eq!(dv1, dv2, "vCard import is content-addressed");
        assert!(dv1.to_string().starts_with("blake3:"), "etag: {dv1}");

        // Calendar put_ics imports a valid iCalendar into an existing collection, returns a blake3 etag,
        // and is content-addressed (identical input -> same etag).
        let cal_meta = calendar::CollectionMeta {
            display_name: "Work".into(),
            component_set: Vec::new(),
        };
        client
            .calendar_create_collection(&session, "cal", "alice", "work", &cal_meta.encode())
            .expect("create calendar collection");
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:evt-1\r\nSUMMARY:Standup\r\nDTSTART:20240115T100000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let dc1 = client
            .calendar_put_ics(&session, "cal", "alice", "work", ics)
            .expect("put_ics");
        let dc2 = client
            .calendar_put_ics(&session, "cal", "alice", "work", ics)
            .expect("put_ics again");
        assert_eq!(dc1, dc2, "ICS import is content-addressed");
        assert!(dc1.to_string().starts_with("blake3:"), "etag: {dc1}");
        // And an invalid iCalendar payload is rejected.
        assert!(
            client
                .calendar_put_ics(&session, "cal", "alice", "work", "not an ics")
                .is_err(),
            "invalid ICS must be rejected"
        );

        // VCS head_branch resolves the HEAD of a committed workspace, and vcs log over that head returns
        // the commit history.
        client
            .write_file(&session, "repo", "f.txt", b"hi", 0)
            .expect("write file");
        client.stage_all(&session, "repo").expect("stage all");
        client
            .commit(&session, "repo", "alice", "init", 1_000)
            .expect("commit");
        let head = client
            .vcs_head_branch(&session, "repo")
            .expect("head_branch on a committed workspace");
        assert_eq!(head, "main", "default HEAD branch after first commit");
        assert_eq!(
            client.log(&session, "repo", &head).expect("log").len(),
            1,
            "one commit on HEAD"
        );

        // Search source_digest and VCS head_branch are wired; an absent workspace is a clear error.
        assert!(
            client
                .search_source_digest(&session, "absent", "idx")
                .is_err(),
            "source_digest on an absent workspace errors"
        );
        assert!(
            client.vcs_head_branch(&session, "absent").is_err(),
            "head_branch on an absent workspace errors"
        );

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }
}
