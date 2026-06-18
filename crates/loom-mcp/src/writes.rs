//! Write-tool engine facade.
//!
//! Each method projects one write tool from [`crate::tools`] onto the engine, routed through
//! [`crate::StoreAccess::write`] - which opens (or locks) the loom, runs the mutation, and persists.
//! Every core write function called here authorizes through its owning facet before mutating state, so
//! the policy enforcement point is crossed on every call. A passwordless loom resolves the caller to
//! the owner; an enforced loom runs `AclStore::authorize` before any state changes or is saved.
//!
//! The facade takes/returns plain Rust values so it is unit-testable on the default build; the `server`
//! feature's rmcp layer adapts MCP tool arguments and results to these.
//!
//! Licensed under BUSL-1.1.

use loom_coordination::with_local_store_write_lock;
use loom_core::calendar::{self, CalendarEntry, CollectionMeta, Component};
use loom_core::contacts::{self, BookMeta, ContactEntry};
use loom_core::error::{Code, LoomError, Result};
use loom_core::inference::EmbeddingModel;
use loom_core::mail::{self, MailboxMeta};
use loom_core::vcs::{ConflictResolution, MergeOutcome, ReplayOutcome};
use loom_core::workspace::WsSelector;
use loom_core::workspace::{FacetKind, WorkspaceId};
use loom_core::{
    AclDomain, AclRight, Algo, DataframePlan, Digest, LogRecord, Loom, MetricDescriptor,
    MetricObservation, SpanRecord, cas_delete, cas_put, columnar_append, columnar_compact,
    columnar_create, dataframe_create, dataframe_materialize, graph_remove_edge, graph_remove_node,
    graph_upsert_edge, graph_upsert_node, key_from_cbor, ledger_append, logs_put_record,
    metrics_put_descriptor, metrics_put_observation, search_create, search_delete, search_index,
    search_remap, traces_put_span, ts_put, vector_create, vector_create_metadata_index,
    vector_delete, vector_drop_metadata_index, vector_upsert, vector_upsert_text,
    vector_upsert_with_source,
};
use loom_interchange::Coverage;
use loom_interchange_io::{
    TicketImportFieldPolicy, execute_import_execution_batch, import_meetings_bytes,
    import_redmine_bytes, import_redmine_bytes_with_field_policy, input_profile_label,
    parse_meetings_input_profile, persist_import_batch_submission,
};
use loom_reference::{
    ReferenceArtifactCreateRequest, ReferenceArtifactSummary, remove_graph_edge_refs,
    update_document_refs, update_graph_edge_refs,
};
use loom_sql::LoomSqlStore;
use loom_store::{
    FileStore, LocalOpenAuth, attach_local_auth, daemon, local_auth_requires_write,
    open_loom_daemon_authorized_unlocked, open_loom_unlocked, save_loom,
};
use loom_substrate::admission::{
    WriteAdmissionMode, WriteAdmissionPolicy, WriteAdmissionTarget, write_admission_policy_key,
};
use loom_substrate::meetings::{
    AnnotationRecord, AnnotationStatus, EntityMergeInput, EntityMergeRecord,
    MeetingsProfileSnapshot, ProjectionAction, ProjectionKind, ProjectionOutput,
    ProjectionOutputSet, PromotionInput, VocabularyTermInput, VocabularyTermRecord,
    VocabularyTermStatus, meetings_profile_key,
};
use loom_substrate::refs::ReferenceArtifactKind;
use loom_substrate::search::{
    EMBEDDING_PROJECTION_JOBS_DIR, EmbeddingProjectionJob, EmbeddingProjectionKey,
    EmbeddingProjectionStamp,
};
use loom_substrate::versioning::{
    BodyRef, ProfileRevisionUpdate, ProfileTransaction, ProfileTransactionState, RevisionIndex,
};
use loom_substrate::view::{FreshnessPolicy, ViewDefinition, ViewDefinitionInput};
use loom_substrate::workgraph::{
    WorkgraphFact, WorkgraphFactKind, WorkgraphOperationLog, WorkgraphOperationRecord,
    workgraph_operation_cursor_scope, workgraph_operation_kind, workgraph_operation_log_key,
};
use loom_substrate::{OperationEnvelope, OperationEnvelopeInput};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::apps;
use crate::chat::ChatWriteSummary;
use crate::drive::{
    DriveConflictResolutionRequest, DriveConflictSummary, DriveCreateUploadRequest,
    DriveGrantShareRequest, DriveLeaseBreakSummary, DriveLeaseTokenSummary,
    DrivePinRetentionRequest, DriveUploadSessionSummary, DriveWriteSummary, FenceSummary,
};
use crate::facet_cbor::{
    columnar_columns_from_cbor, columnar_row_from_cbor, props_from_cbor, search_document_from_cbor,
    search_mapping_from_cbor, vector_from_bytes, vector_metadata_from_cbor, vector_metric,
};
use crate::pages::{
    PageCreateRequest, PagePublishSummary, PageSummary, PageUpdateSummary, SpaceSummary,
    StructureBindRequest, StructureCreateRequest, StructureDecomposeRequest,
    StructureDecomposeSummary, StructureEdgeSummary, StructureLinkRequest, StructureMoveRequest,
    StructureMoveSummary, StructureNodeRequest, StructureNodeSummary, StructureRenderSummary,
};
use crate::reads::{SubstrateAliasSummary, resolve_ns};
use crate::substrate_refs::{bind_alias, release_alias};
use crate::substrate_revisions::{REVISION_INDEX_DIR, revision_index_path};
use crate::substrate_views::{VIEW_DIR, ViewDefinitionSummary, view_path};
use crate::{authorize_workgraph_task, now_ms, reject_stateless_ephemeral_kv};
use loom_lanes::{Lane, LaneInput, LaneKind, LaneStatus, LaneTicket, LaneTicketPlacement};
use loom_lifecycle::{
    LifecycleDefinitionSummary, LifecycleInstanceSummary, LifecycleTransitionRequest,
    LifecycleTransitionResult, StandardLifecycleRequest,
};
use loom_tickets::{
    BoardCardMoveRequest, BoardColumnConfigureRequest, BoardCreateRequest, BoardSummary,
    BoardUpdateRequest, TicketComment, TicketCommentDeleteRequest, TicketCommentRequest,
    TicketCommentUpdateRequest, TicketCreateRequest, TicketDeleteRequest, TicketFieldCatalog,
    TicketFieldDefinitionRetireRequest, TicketFieldDefinitionWriteRequest, TicketKey,
    TicketProfileReader, TicketProjectSummary, TicketRelationRemoveRequest, TicketRelationRequest,
    TicketRelationSummary, TicketSummary, TicketUpdateRequest,
};
use loom_types::{MutationChange, MutationEnvelope, MutationReceipt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteAdmission {
    pub target_kind: String,
    pub target_id: String,
    pub fence: loom_core::Fence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteAdmissionPolicyRequest<'a> {
    pub workspace: &'a str,
    pub surface: &'a str,
    pub scope_id: &'a str,
    pub default_mode: &'a str,
    pub mandatory_targets: &'a [WriteAdmissionTarget],
}

fn authorization_domain_for_surface(surface: &str) -> Result<AclDomain> {
    match surface {
        "drive" | "fs" => Ok(AclDomain::Files),
        "lanes" | "workgraph" => Ok(AclDomain::Tickets),
        "spaces" | "structures" => Ok(AclDomain::Pages),
        other => AclDomain::parse(other),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct WriteAdmissionTargetSummary {
    pub target_kind: String,
    pub target_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct WriteAdmissionPolicySummary {
    pub workspace: String,
    pub surface: String,
    pub scope_id: String,
    pub default_mode: String,
    pub mandatory_targets: Vec<WriteAdmissionTargetSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MeetingsAnnotationReviewSummary {
    pub workspace_id: String,
    pub annotation_id: String,
    pub status: String,
    pub accepted_by: Option<String>,
    pub accepted_at_ms: Option<u64>,
    pub record_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MeetingsVocabularyReviewSummary {
    pub workspace_id: String,
    pub term_id: String,
    pub status: String,
    pub reviewed_by: Option<String>,
    pub reviewed_at_ms: Option<u64>,
    pub record_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MeetingsEntityMergeWriteSummary {
    pub workspace_id: String,
    pub merge_id: String,
    pub canonical_entity_id: String,
    pub merged_entity_ids: Vec<String>,
    pub record_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MeetingsPromotionWriteSummary {
    pub workspace_id: String,
    pub promotion_id: String,
    pub operation_kind: String,
    pub source_annotation_id: String,
    pub target_profile: String,
    pub target_entity_ref: String,
    pub record_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MeetingsTicketPromotionWriteSummary {
    pub promotion: MeetingsPromotionWriteSummary,
    pub ticket: TicketSummary,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MeetingsDecisionPromotionWriteSummary {
    pub promotion: MeetingsPromotionWriteSummary,
    pub decision_ledger: String,
    pub ledger_sequence: u64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MeetingsLifecyclePromotionWriteSummary {
    pub promotion: MeetingsPromotionWriteSummary,
    pub lifecycle: LifecycleInstanceSummary,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MeetingsReferenceArtifactPromotionWriteSummary {
    pub promotion: MeetingsPromotionWriteSummary,
    pub reference_artifact: ReferenceArtifactSummary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeetingsPromoteTaskToTicketRequest<'a> {
    pub promotion_id: &'a str,
    pub source_annotation_id: &'a str,
    pub project_id: &'a str,
    pub ticket_type: &'a str,
    pub policy_labels: &'a [String],
    pub expected_ticket_root: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneCreateRequest<'a> {
    pub lane_id: &'a str,
    pub lane_key: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub lane_kind: &'a str,
    pub owner_principal: Option<&'a str>,
    pub lane_status: &'a str,
    pub lane_tickets: &'a [LaneTicket],
    pub active_ticket_id: Option<&'a str>,
    pub status_report: &'a str,
    pub reviewer_feedback: &'a str,
    /// optional explicit actor override; `None` derives from the effective principal.
    pub updated_by: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneUpdateRequest<'a> {
    pub lane_id: &'a str,
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub lane_status: Option<&'a str>,
    pub status_report: Option<&'a str>,
    pub reviewer_feedback: Option<&'a str>,
    /// optional explicit actor override; `None` derives from the effective principal.
    pub updated_by: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneTicketUpdateRequest<'a> {
    pub lane_id: &'a str,
    pub ticket_id: &'a str,
    /// where the ticket lands. Ignored by ticket-remove; defaults to append for add callers.
    pub placement: LaneTicketPlacement<'a>,
    /// optional explicit actor override; `None` derives from the effective principal.
    pub updated_by: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneTicketTransferRequest<'a> {
    pub source_lane_id: &'a str,
    pub target_lane_id: &'a str,
    pub ticket_id: &'a str,
    pub updated_by: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneDeleteRequest<'a> {
    pub lane_id: &'a str,
    pub updated_by: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeetingsPromoteDecisionToDecisionLogRequest<'a> {
    pub promotion_id: &'a str,
    pub source_annotation_id: &'a str,
    pub decision_id: &'a str,
    pub ledger_name: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeetingsPromoteQuestionToLifecycleRequest<'a> {
    pub promotion_id: &'a str,
    pub source_annotation_id: &'a str,
    pub instance_id: &'a str,
    pub definition_id: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeetingsPromoteArtifactToReferenceArtifactRequest<'a> {
    pub promotion_id: &'a str,
    pub source_annotation_id: &'a str,
    pub artifact_id: &'a str,
    pub target_ref: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeetingsPromoteReferenceToReferenceArtifactRequest<'a> {
    pub promotion_id: &'a str,
    pub source_annotation_id: &'a str,
    pub reference_id: &'a str,
    pub target_ref: Option<&'a str>,
}

struct MeetingsReferenceArtifactPromotionInput<'a> {
    source_kind: &'a str,
    operation_kind: &'a str,
    artifact_kind: ReferenceArtifactKind,
    record_id: &'a str,
    promotion_id: &'a str,
    source_annotation_id: &'a str,
    target_ref: Option<&'a str>,
}

impl From<WriteAdmissionPolicy> for WriteAdmissionPolicySummary {
    fn from(policy: WriteAdmissionPolicy) -> Self {
        Self {
            workspace: policy.workspace.to_string(),
            surface: policy.surface,
            scope_id: policy.scope_id,
            default_mode: policy.default_mode.as_str().to_string(),
            mandatory_targets: policy
                .mandatory_targets
                .into_iter()
                .map(|target| WriteAdmissionTargetSummary {
                    target_kind: target.target_kind,
                    target_id: target.target_id,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct StudioReindexSummary {
    pub workspace: String,
    pub profile: String,
    pub job_path: String,
    pub state: String,
    pub source_digest: String,
    pub model_id: String,
    pub vector_records_indexed: usize,
    pub vector_records_deleted: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ImportBatchSubmitSummary {
    pub workspace: String,
    pub workspace_id: String,
    pub profile: String,
    pub source_system: String,
    pub source_scope: String,
    pub coverage: String,
    pub observed_at_ms: u64,
    pub item_count: usize,
    pub batch_digest: String,
    pub control_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ImportBatchExecuteSummary {
    pub workspace: String,
    pub workspace_id: String,
    pub profile: String,
    pub source_system: String,
    pub source_scope: String,
    pub coverage: String,
    pub observed_at_ms: u64,
    pub payload_count: usize,
    pub execution_digest: String,
    pub control_key: String,
    pub changed: bool,
    pub dry_run: bool,
    pub rows_imported: u64,
    pub operations_planned: u64,
    pub operations_applied: u64,
    pub skipped: u64,
    pub bytes_in: u64,
    pub bytes_stored: u64,
    pub warnings: Vec<String>,
    pub fidelity_issues: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MeetingsImportSnapshotSummary {
    pub workspace: String,
    pub workspace_id: String,
    pub input_profile: String,
    pub source_scope: String,
    pub changed: bool,
    pub dry_run: bool,
    pub rows_imported: u64,
    pub operations_planned: u64,
    pub operations_applied: u64,
    pub bytes_in: u64,
    pub bytes_stored: u64,
    pub payload_bytes: u64,
    pub warnings: Vec<String>,
    pub fidelity_issues: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RedmineImportSnapshotSummary {
    pub workspace: String,
    pub workspace_id: String,
    pub profile: String,
    pub source_scope: String,
    pub dry_run: bool,
    pub rows_imported: u64,
    pub operations_planned: u64,
    pub operations_applied: u64,
    pub skipped: u64,
    pub bytes_in: u64,
    pub bytes_stored: u64,
    pub warnings: Vec<String>,
    pub fidelity_issues: usize,
}

struct ResolvedTextEmbeddingInstance {
    instance: loom_types::InferenceInstanceDescriptor,
    handle: loom_inference::TextEmbeddingHandle,
}

struct StudioVectorDrainSummary {
    indexed: usize,
    deleted: usize,
}

fn inference_cache_dir() -> Result<PathBuf> {
    if let Some(hf_home) = std::env::var_os("HF_HOME") {
        return Ok(PathBuf::from(hf_home).join("hub"));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| LoomError::invalid("home directory is unavailable"))?;
    Ok(home.join(".cache").join("huggingface").join("hub"))
}

fn resolve_optional_vector_binding(
    loom: &Loom<FileStore>,
    workspace: &str,
) -> Result<Option<ResolvedTextEmbeddingInstance>> {
    let cache_dir = inference_cache_dir()?;
    let mut hardware = loom_inference::probe_hardware()?;
    hardware.hf_cache_dir = Some(cache_dir.to_string_lossy().into_owned());
    let workspace_id = resolve_ns(loom, workspace)?;
    let state = loom_core::inference_instance_state(loom, workspace_id)?;
    let Some(binding) = state
        .vector_bindings
        .iter()
        .find(|binding| binding.workspace == workspace_id.to_string())
    else {
        return Ok(None);
    };
    let instance = state
        .find_instance(&binding.embedding_instance)
        .cloned()
        .ok_or_else(|| {
            LoomError::not_found(format!(
                "inference instance {:?} not found",
                binding.embedding_instance
            ))
        })?;
    if instance.kind != loom_types::InferenceModelKind::TextEmbedding {
        return Err(LoomError::invalid(format!(
            "inference instance {:?} is not a text-embedding instance",
            binding.embedding_instance
        )));
    }
    let record =
        loom_inference::discover_installed_model(&cache_dir, &instance.model, instance.runtime)?
            .ok_or_else(|| {
                LoomError::not_found(format!(
                    "model {:?} is not installed for runtime {}",
                    instance.model.repo_id,
                    instance.runtime.as_str()
                ))
            })?;
    let handle = loom_inference::activate_text_embedding(&record, &hardware, &cache_dir)?;
    Ok(Some(ResolvedTextEmbeddingInstance { instance, handle }))
}

fn studio_reindex_source_digest(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    profile: &str,
) -> Result<Digest> {
    let head = loom.registry().head_branch(ns)?;
    if let Some(tip) = loom.registry().branch_tip(ns, &head)? {
        Ok(tip)
    } else {
        Ok(Digest::hash(
            loom.store().digest_algo(),
            format!("studio-reindex:{ns}:{profile}").as_bytes(),
        ))
    }
}

fn studio_reindex_stamp_for_instance(
    source_digest: Digest,
    instance: &loom_types::InferenceInstanceDescriptor,
) -> Result<EmbeddingProjectionStamp> {
    let descriptor_bytes = serde_json::to_vec(instance).map_err(|e| {
        LoomError::invalid(format!("embedding instance descriptor encode failed: {e}"))
    })?;
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

fn studio_reindex_job(
    ns: WorkspaceId,
    profile: &str,
    source_digest: Digest,
    instance: Option<&loom_types::InferenceInstanceDescriptor>,
) -> Result<EmbeddingProjectionJob> {
    let key = EmbeddingProjectionKey::new(ns.to_string(), "studio", profile, "reindex")?;
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
        None => job.no_engine("built-in embedding inference is not configured"),
    }
}

fn studio_meetings_profile_ids(ns: WorkspaceId, profile: &str) -> Vec<String> {
    match profile {
        "all" | "meetings" => vec![ns.to_string()],
        profile => vec![profile.to_string()],
    }
}

fn coverage_label(coverage: Coverage) -> &'static str {
    match coverage {
        Coverage::Complete => "complete",
        Coverage::Partial => "partial",
        Coverage::Degraded => "degraded",
    }
}

fn load_meetings_snapshot(
    loom: &Loom<FileStore>,
    profile_id: &str,
) -> Result<Option<MeetingsProfileSnapshot>> {
    let key = meetings_profile_key(profile_id)?;
    loom.store()
        .control_get(&key)?
        .map(|bytes| MeetingsProfileSnapshot::decode(&bytes))
        .transpose()
}

fn require_meetings_snapshot(
    loom: &Loom<FileStore>,
    profile_id: &str,
) -> Result<MeetingsProfileSnapshot> {
    load_meetings_snapshot(loom, profile_id)?
        .ok_or_else(|| LoomError::not_found("meetings snapshot not found"))
}

fn save_meetings_snapshot(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    profile_id: &str,
    snapshot: &MeetingsProfileSnapshot,
    action: &str,
    target_id: &str,
) -> Result<()> {
    let target = format!("{profile_id}/{target_id}");
    loom.store().control_set_audited(
        &meetings_profile_key(profile_id)?,
        snapshot.encode()?,
        loom.effective_principal()?.or(Some(workspace)),
        action,
        Some(&target),
    )?;
    Ok(())
}

fn update_meetings_review_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    profile_id: &str,
    entity_id: String,
    operation_id: String,
    body: &[u8],
    media_type: &str,
    timestamp_ms: u64,
) -> Result<()> {
    let index_path = revision_index_path(profile_id)?;
    let index = match loom.read_file_reserved(workspace, &index_path) {
        Ok(bytes) => RevisionIndex::decode(&bytes)?,
        Err(err) if err.code == Code::NotFound => RevisionIndex::new(),
        Err(err) => return Err(err),
    };
    let expected_latest_revision = index
        .latest(&entity_id)
        .map(|entry| entry.revision)
        .unwrap_or(0);
    let revision = expected_latest_revision.saturating_add(1);
    let root = Digest::hash(loom.store().digest_algo(), body);
    let logical_path = format!("meetings:{profile_id}:{entity_id}:{revision}");
    let mut state = ProfileTransactionState::new(root, index);
    let update = ProfileRevisionUpdate::new(
        entity_id,
        operation_id,
        BodyRef::new(
            Digest::hash(loom.store().digest_algo(), body),
            body.len() as u64,
            media_type,
        )?,
        timestamp_ms,
        logical_path,
        Some(expected_latest_revision),
    )?;
    state.apply(ProfileTransaction::new(
        profile_id,
        None,
        root,
        vec![update],
    )?)?;
    let index = state.into_revision_index();
    loom.create_directory_reserved(workspace, REVISION_INDEX_DIR, true)?;
    loom.write_file_reserved(workspace, &index_path, &index.encode()?, 0o100644)
}

fn annotation_status(status: AnnotationStatus) -> &'static str {
    match status {
        AnnotationStatus::Observed => "observed",
        AnnotationStatus::Suggested => "suggested",
        AnnotationStatus::Accepted => "accepted",
        AnnotationStatus::Rejected => "rejected",
        AnnotationStatus::Superseded => "superseded",
        AnnotationStatus::Merged => "merged",
    }
}

fn vocabulary_status(status: VocabularyTermStatus) -> &'static str {
    match status {
        VocabularyTermStatus::Proposed => "proposed",
        VocabularyTermStatus::Accepted => "accepted",
        VocabularyTermStatus::Rejected => "rejected",
    }
}

fn annotation_review_summary(
    profile_id: &str,
    annotation: AnnotationRecord,
) -> Result<MeetingsAnnotationReviewSummary> {
    Ok(MeetingsAnnotationReviewSummary {
        workspace_id: profile_id.to_string(),
        annotation_id: annotation.annotation_id.clone(),
        status: annotation_status(annotation.status).to_string(),
        accepted_by: annotation.accepted_by.clone(),
        accepted_at_ms: annotation.accepted_at_ms,
        record_cbor_hex: hex::encode(annotation.encode()?),
    })
}

fn vocabulary_review_summary(
    profile_id: &str,
    term: VocabularyTermRecord,
) -> Result<MeetingsVocabularyReviewSummary> {
    Ok(MeetingsVocabularyReviewSummary {
        workspace_id: profile_id.to_string(),
        term_id: term.term_id.clone(),
        status: vocabulary_status(term.status).to_string(),
        reviewed_by: term.reviewed_by.clone(),
        reviewed_at_ms: term.reviewed_at_ms,
        record_cbor_hex: hex::encode(term.encode()?),
    })
}

fn entity_merge_summary(
    profile_id: &str,
    merge: EntityMergeRecord,
) -> Result<MeetingsEntityMergeWriteSummary> {
    Ok(MeetingsEntityMergeWriteSummary {
        workspace_id: profile_id.to_string(),
        merge_id: merge.merge_id.clone(),
        canonical_entity_id: merge.canonical_entity_id.clone(),
        merged_entity_ids: merge.merged_entity_ids.clone(),
        record_cbor_hex: hex::encode(merge.encode()?),
    })
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

fn meetings_vector_metadata(output: &ProjectionOutput) -> BTreeMap<String, loom_core::Value> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "entity_kind".to_string(),
        loom_core::Value::Text(output.entity_kind.clone()),
    );
    metadata.insert(
        "entity_id".to_string(),
        loom_core::Value::Text(output.entity_id.clone()),
    );
    metadata.insert(
        "output_ref".to_string(),
        loom_core::Value::Text(output.output_ref.clone()),
    );
    metadata.insert(
        "output_id".to_string(),
        loom_core::Value::Text(output.output_id.clone()),
    );
    metadata.insert(
        "source_ids".to_string(),
        loom_core::Value::List(
            output
                .source_ids
                .iter()
                .cloned()
                .map(loom_core::Value::Text)
                .collect(),
        ),
    );
    metadata
}

fn meetings_vector_projection_job(
    ns: WorkspaceId,
    profile_id: &str,
    source_digest: Digest,
    output: &ProjectionOutput,
    resolved: &ResolvedTextEmbeddingInstance,
) -> Result<EmbeddingProjectionJob> {
    let key =
        EmbeddingProjectionKey::new(ns.to_string(), "meetings", profile_id, &output.output_id)?;
    let stamp = studio_reindex_stamp_for_instance(source_digest, &resolved.instance)?;
    Ok(EmbeddingProjectionJob::queued(key, stamp))
}

fn drain_meetings_vector_outputs(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    profile: &str,
    resolved: &ResolvedTextEmbeddingInstance,
) -> Result<StudioVectorDrainSummary> {
    let model = resolved
        .handle
        .model()
        .ok_or_else(|| LoomError::unsupported("text embedding provider did not expose a model"))?;
    let mut summary = StudioVectorDrainSummary {
        indexed: 0,
        deleted: 0,
    };
    for profile_id in studio_meetings_profile_ids(ns, profile) {
        let Some(snapshot) = load_meetings_snapshot(loom, &profile_id)? else {
            continue;
        };
        let snapshot_bytes = snapshot.encode()?;
        let profile_root = Digest::hash(loom.store().digest_algo(), &snapshot_bytes);
        let output_set = ProjectionOutputSet::from_snapshot(&snapshot)?;
        let collection = meetings_vector_collection(&profile_id);
        match vector_create(
            loom,
            ns,
            &collection,
            model.dimension,
            loom_core::Metric::Cosine,
        ) {
            Ok(()) => {}
            Err(err) if err.code == Code::Conflict => {}
            Err(err) => return Err(err),
        }
        for output in output_set.outputs_for(ProjectionKind::Vector) {
            let job =
                meetings_vector_projection_job(ns, &profile_id, profile_root, output, resolved)?;
            let path = job.job_path(loom.store().digest_algo())?;
            match output.action {
                ProjectionAction::Upsert | ProjectionAction::Append => {
                    vector_upsert_text(
                        loom,
                        ns,
                        &collection,
                        &meetings_vector_id(output),
                        &output.text_body(),
                        meetings_vector_metadata(output),
                        &resolved.handle,
                    )?;
                    summary.indexed = summary.indexed.saturating_add(1);
                }
                ProjectionAction::Invalidate | ProjectionAction::RetainMetadata => {
                    if vector_delete(loom, ns, &collection, &meetings_vector_id(output))? {
                        summary.deleted = summary.deleted.saturating_add(1);
                    }
                }
            }
            loom.create_directory_reserved(ns, EMBEDDING_PROJECTION_JOBS_DIR, true)?;
            loom.write_file_reserved(ns, &path, &job.ready().encode()?, 0o100644)?;
        }
    }
    Ok(summary)
}

fn apply_studio_reindex(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    profile: &str,
    resolved: Option<&ResolvedTextEmbeddingInstance>,
) -> Result<StudioReindexSummary> {
    let ns = resolve_ns(loom, workspace)?;
    let source_digest = studio_reindex_source_digest(loom, ns, profile)?;
    let instance = resolved.map(|resolved| &resolved.instance);
    let job = studio_reindex_job(ns, profile, source_digest, instance)?;
    let job_path = job.job_path(loom.store().digest_algo())?;
    loom.create_directory_reserved(ns, EMBEDDING_PROJECTION_JOBS_DIR, true)?;
    loom.write_file_reserved(ns, &job_path, &job.encode()?, 0o100644)?;
    let mut vector_records_indexed = 0usize;
    let mut vector_records_deleted = 0usize;
    if let Some(resolved) = resolved {
        let summary = drain_meetings_vector_outputs(loom, ns, profile, resolved)?;
        vector_records_indexed = summary.indexed;
        vector_records_deleted = summary.deleted;
    }
    Ok(StudioReindexSummary {
        workspace: ns.to_string(),
        profile: profile.to_string(),
        job_path,
        state: job.state.as_str().to_string(),
        source_digest: source_digest.to_string(),
        model_id: job.stamp.model_id,
        vector_records_indexed,
        vector_records_deleted,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DocumentReplaceTextResult {
    pub replacements: u64,
    pub digest: String,
    pub entity_tag: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentReplaceTextRequest<'a> {
    pub workspace: &'a str,
    pub name: &'a str,
    pub id: &'a str,
    pub base_digest: &'a str,
    pub find: &'a str,
    pub replace: &'a str,
    pub replace_all: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubstrateViewDefineRequest<'a> {
    pub workspace: &'a str,
    pub view_id: &'a str,
    pub source_scopes: &'a [&'a str],
    pub source_facets: &'a [&'a str],
    pub projection_ref: &'a str,
    pub output_facet: Option<&'a str>,
    pub media_type: &'a str,
    pub freshness_policy: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubstrateViewDefineOwned {
    pub workspace: String,
    pub view_id: String,
    pub source_scopes: Vec<String>,
    pub source_facets: Vec<String>,
    pub projection_ref: String,
    pub output_facet: Option<String>,
    pub media_type: String,
    pub freshness_policy: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubstrateTransactOp {
    CasPut {
        workspace: String,
        content: Vec<u8>,
    },
    CasDelete {
        workspace: String,
        digest: String,
    },
    DocumentPut {
        workspace: String,
        collection: String,
        id: String,
        doc: Vec<u8>,
    },
    DocumentDelete {
        workspace: String,
        collection: String,
        id: String,
    },
    DocumentReplaceText {
        workspace: String,
        collection: String,
        id: String,
        base_digest: String,
        find: String,
        replace: String,
        replace_all: bool,
    },
    GraphUpsertNode {
        workspace: String,
        collection: String,
        id: String,
        props: Vec<u8>,
    },
    GraphRemoveNode {
        workspace: String,
        collection: String,
        id: String,
        cascade: bool,
    },
    GraphUpsertEdge {
        workspace: String,
        collection: String,
        id: String,
        src: String,
        dst: String,
        label: String,
        props: Vec<u8>,
    },
    GraphRemoveEdge {
        workspace: String,
        collection: String,
        id: String,
    },
    SubstrateViewDefine(SubstrateViewDefineOwned),
}

impl SubstrateTransactOp {
    fn kind(&self) -> &'static str {
        match self {
            Self::CasPut { .. } => "cas.put",
            Self::CasDelete { .. } => "cas.delete",
            Self::DocumentPut { .. } => "document.put",
            Self::DocumentDelete { .. } => "document.delete",
            Self::DocumentReplaceText { .. } => "document.replace_text",
            Self::GraphUpsertNode { .. } => "graph.upsert_node",
            Self::GraphRemoveNode { .. } => "graph.remove_node",
            Self::GraphUpsertEdge { .. } => "graph.upsert_edge",
            Self::GraphRemoveEdge { .. } => "graph.remove_edge",
            Self::SubstrateViewDefine(_) => "substrate.view_define",
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct SubstrateTransactResult {
    pub applied: u64,
    pub results: Vec<SubstrateTransactOpResult>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct SubstrateTransactOpResult {
    pub kind: &'static str,
    pub value: Value,
}

/// Parse a conflict-resolution wire token (`ours`/`theirs`/`working`).
fn parse_resolution(resolution: &str) -> Result<ConflictResolution> {
    match resolution {
        "ours" => Ok(ConflictResolution::Ours),
        "theirs" => Ok(ConflictResolution::Theirs),
        "working" => Ok(ConflictResolution::Working),
        other => Err(LoomError::invalid(format!(
            "unknown conflict resolution {other:?} (want ours/theirs/working)"
        ))),
    }
}

/// Parse a comma-separated calendar component set (`event`/`todo`), mirroring the C ABI.
fn parse_component_set(components: &str) -> Result<Vec<Component>> {
    let mut out = Vec::new();
    for tok in components.split(',') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        match tok {
            "event" => out.push(Component::Event),
            "todo" => out.push(Component::Todo),
            other => return Err(LoomError::invalid(format!("unknown component {other:?}"))),
        }
    }
    Ok(out)
}

pub(crate) fn fresh_workspace_id() -> Result<WorkspaceId> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| {
        LoomError::new(
            Code::Internal,
            format!("failed to generate identifier: {error}"),
        )
    })?;
    Ok(WorkspaceId::v4_from_bytes(bytes))
}

fn parse_workspace_id(value: &str) -> Result<WorkspaceId> {
    WorkspaceId::parse(value).map_err(|err| LoomError::invalid(err.message))
}

/// Arguments for `graph.upsert_edge`.
pub struct GraphEdgeWrite<'a> {
    pub id: &'a str,
    pub src: &'a str,
    pub dst: &'a str,
    pub label: &'a str,
    pub props: &'a [u8],
}

/// Arguments for `vector.upsert_source`.
pub struct VectorSourceWrite<'a> {
    pub id: &'a str,
    pub vector: &'a [u8],
    pub metadata: &'a [u8],
    pub source_text: &'a [u8],
    pub model_id: Option<&'a str>,
    pub weights_digest: Option<&'a str>,
}

fn apply_cas_put(loom: &mut Loom<FileStore>, workspace: &str, content: &[u8]) -> Result<String> {
    let ns = resolve_ns(loom, workspace)?;
    Ok(cas_put(loom, ns, content)?.to_string())
}

fn apply_cas_delete(loom: &mut Loom<FileStore>, workspace: &str, digest: &str) -> Result<bool> {
    let digest = Digest::parse(digest)?;
    let ns = resolve_ns(loom, workspace)?;
    cas_delete(loom, ns, &digest)
}

// The document write + reference-index overlay lives in `loom_reference` (shared by the local MCP host
// and the remote server dispatch); these thin wrappers resolve the workspace and delegate.
fn apply_document_put(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    name: &str,
    id: &str,
    doc: Vec<u8>,
) -> Result<()> {
    let ns = resolve_ns(loom, workspace)?;
    loom_reference::put_document_indexed(loom, ns, name, id, doc)
}

fn apply_document_put_binary(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    name: &str,
    id: &str,
    bytes: Vec<u8>,
    expected_entity_tag: Option<&str>,
) -> Result<loom_core::document::DocumentPutResult> {
    let ns = resolve_ns(loom, workspace)?;
    let text = std::str::from_utf8(&bytes).ok().map(str::to_string);
    let result = loom_core::document::document_put_binary_with_entity_tag(
        loom,
        ns,
        name,
        id,
        bytes,
        expected_entity_tag,
    )?;
    update_document_refs(loom, ns, name, id, text.as_deref())?;
    Ok(result)
}

fn apply_document_put_text(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    name: &str,
    id: &str,
    text: &str,
    expected_entity_tag: Option<&str>,
) -> Result<loom_core::document::DocumentPutResult> {
    let ns = resolve_ns(loom, workspace)?;
    let result = loom_core::document::document_put_text_with_entity_tag(
        loom,
        ns,
        name,
        id,
        text,
        expected_entity_tag,
    )?;
    update_document_refs(loom, ns, name, id, Some(text))?;
    Ok(result)
}

fn apply_document_delete(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    name: &str,
    id: &str,
) -> Result<bool> {
    let ns = resolve_ns(loom, workspace)?;
    loom_reference::delete_document_indexed(loom, ns, name, id)
}

fn apply_document_replace_text(
    loom: &mut Loom<FileStore>,
    request: DocumentReplaceTextRequest<'_>,
) -> Result<DocumentReplaceTextResult> {
    let ns = resolve_ns(loom, request.workspace)?;
    let outcome = loom_reference::replace_text_indexed(
        loom,
        ns,
        request.name,
        request.id,
        request.find,
        request.replace,
        request.replace_all,
        request.base_digest,
    )?;
    let digest = Digest::parse(&outcome.digest)?;
    Ok(DocumentReplaceTextResult {
        replacements: outcome.replacements,
        entity_tag: loom_core::document_entity_tag_string_from_digest(digest),
        digest: outcome.digest,
    })
}

fn apply_workgraph_fact_put(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    workspace_id: &str,
    fact_bytes: Vec<u8>,
) -> Result<()> {
    let ns = resolve_ns(loom, workspace)?;
    let fact = WorkgraphFact::decode(&fact_bytes)?;
    authorize_workgraph_task(loom, ns, &fact.task_id, AclRight::Write)?;
    if fact.kind == WorkgraphFactKind::BoardReadObserved {
        return Err(LoomError::invalid(
            "board_read_observed facts are recorded by the server",
        ));
    }
    let key = workgraph_operation_log_key(workspace_id)?;
    let mut log = match loom.store().control_get(&key)? {
        Some(bytes) => WorkgraphOperationLog::decode(&bytes)?,
        None => WorkgraphOperationLog::new(workspace_id, Vec::new())?,
    };
    let sequence = log
        .records
        .last()
        .map(|record| record.sequence + 1)
        .unwrap_or(1);
    let previous = log.encode()?;
    let base_root = Digest::hash(Algo::Blake3, &previous);
    let mut root_input = previous;
    root_input.extend_from_slice(&fact_bytes);
    let root_after = Digest::hash(Algo::Blake3, &root_input);
    let operation_kind = workgraph_operation_kind(fact.kind);
    let envelope = OperationEnvelope::new(
        Algo::Blake3,
        OperationEnvelopeInput {
            workspace_id,
            app_id: "workgraph",
            scope_id: &workgraph_operation_cursor_scope(workspace_id),
            operation_id: &fact.event_id,
            operation_kind: &operation_kind,
            sequence,
            actor_principal: ns,
            actor_kind: fact.actor_kind,
            timestamp_ms: now_ms(),
            idempotency_key: &fact.event_id,
            base_root,
            base_entity_version: None,
            target_entity_id: Some(&format!("workgraph:{}", fact.task_id)),
            payload: &fact_bytes,
            policy_labels: &[],
            signature: None,
            agent: None,
        },
    )?;
    let record = WorkgraphOperationRecord::fact(sequence, fact, root_after, envelope.encode()?)?;
    log.append(record)?;
    loom.store().control_set(&key, log.encode()?)
}

fn apply_substrate_view_define(
    loom: &mut Loom<FileStore>,
    request: SubstrateViewDefineRequest<'_>,
) -> Result<ViewDefinitionSummary> {
    let freshness_policy = FreshnessPolicy::parse(request.freshness_policy)?;
    let ns = resolve_ns(loom, request.workspace)?;
    for facet in request.source_facets {
        let _ = FacetKind::parse(facet)?;
    }
    let source_digests = current_workspace_source_digests(loom, ns)?;
    let view = ViewDefinition::new(ViewDefinitionInput {
        view_id: request.view_id,
        source_scopes: request.source_scopes,
        source_facets: request.source_facets,
        projection_ref: request.projection_ref,
        output_facet: request.output_facet,
        media_type: request.media_type,
        freshness_policy,
        output_digest: None,
        source_digests: &source_digests,
    })?;
    let bytes = view.encode()?;
    let path = view_path(&view.view_id)?;
    loom.authorize_file_path(ns, &path, AclRight::Write)?;
    loom.create_directory_reserved(ns, VIEW_DIR, true)?;
    loom.write_file_reserved(ns, &path, &bytes, 0o100644)?;
    Ok(ViewDefinitionSummary::from(view))
}

fn current_workspace_source_digests(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
) -> Result<Vec<Digest>> {
    let branch = loom.registry().head_branch(ns)?;
    Ok(loom
        .registry()
        .branch_tip(ns, &branch)?
        .into_iter()
        .collect())
}

fn apply_substrate_transact_op(
    loom: &mut Loom<FileStore>,
    op: SubstrateTransactOp,
) -> Result<SubstrateTransactOpResult> {
    let kind = op.kind();
    let value = match op {
        SubstrateTransactOp::CasPut { workspace, content } => {
            json!(apply_cas_put(loom, &workspace, &content)?)
        }
        SubstrateTransactOp::CasDelete { workspace, digest } => {
            json!(apply_cas_delete(loom, &workspace, &digest)?)
        }
        SubstrateTransactOp::DocumentPut {
            workspace,
            collection,
            id,
            doc,
        } => {
            apply_document_put(loom, &workspace, &collection, &id, doc)?;
            Value::Null
        }
        SubstrateTransactOp::DocumentDelete {
            workspace,
            collection,
            id,
        } => json!(apply_document_delete(loom, &workspace, &collection, &id)?),
        SubstrateTransactOp::DocumentReplaceText {
            workspace,
            collection,
            id,
            base_digest,
            find,
            replace,
            replace_all,
        } => json!(apply_document_replace_text(
            loom,
            DocumentReplaceTextRequest {
                workspace: &workspace,
                name: &collection,
                id: &id,
                base_digest: &base_digest,
                find: &find,
                replace: &replace,
                replace_all,
            },
        )?),
        SubstrateTransactOp::GraphUpsertNode {
            workspace,
            collection,
            id,
            props,
        } => {
            let props = props_from_cbor(&props)?;
            let ns = resolve_ns(loom, &workspace)?;
            graph_upsert_node(loom, ns, &collection, &id, props)?;
            Value::Null
        }
        SubstrateTransactOp::GraphRemoveNode {
            workspace,
            collection,
            id,
            cascade,
        } => {
            let ns = resolve_ns(loom, &workspace)?;
            json!(graph_remove_node(loom, ns, &collection, &id, cascade)?)
        }
        SubstrateTransactOp::GraphUpsertEdge {
            workspace,
            collection,
            id,
            src,
            dst,
            label,
            props,
        } => {
            let props = props_from_cbor(&props)?;
            let ns = resolve_ns(loom, &workspace)?;
            graph_upsert_edge(loom, ns, &collection, &id, &src, &dst, &label, props)?;
            update_graph_edge_refs(loom, ns, &collection, &id, &src, &dst, &label)?;
            Value::Null
        }
        SubstrateTransactOp::GraphRemoveEdge {
            workspace,
            collection,
            id,
        } => {
            let ns = resolve_ns(loom, &workspace)?;
            let removed = graph_remove_edge(loom, ns, &collection, &id)?;
            if removed {
                remove_graph_edge_refs(loom, ns, &collection, &id)?;
            }
            json!(removed)
        }
        SubstrateTransactOp::SubstrateViewDefine(request) => {
            let source_scopes = request
                .source_scopes
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            let source_facets = request
                .source_facets
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            json!(apply_substrate_view_define(
                loom,
                SubstrateViewDefineRequest {
                    workspace: &request.workspace,
                    view_id: &request.view_id,
                    source_scopes: &source_scopes,
                    source_facets: &source_facets,
                    projection_ref: &request.projection_ref,
                    output_facet: request.output_facet.as_deref(),
                    media_type: &request.media_type,
                    freshness_policy: &request.freshness_policy,
                },
            )?)
        }
    };
    Ok(SubstrateTransactOpResult { kind, value })
}

impl crate::LoomMcp {
    pub fn write_import_submit_batch(
        &self,
        workspace: &str,
        batch_bytes: &[u8],
    ) -> Result<ImportBatchSubmitSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let submitted = persist_import_batch_submission(
                loom,
                ns,
                batch_bytes,
                loom.effective_principal()?,
            )?;
            let batch = submitted.batch;
            Ok(ImportBatchSubmitSummary {
                workspace: workspace.to_string(),
                workspace_id: ns.to_string(),
                profile: batch.profile,
                source_system: batch.source_system,
                source_scope: batch.source_scope,
                coverage: coverage_label(batch.coverage).to_string(),
                observed_at_ms: batch.observed_at_ms,
                item_count: batch.items.len(),
                batch_digest: submitted.batch_digest.to_string(),
                control_key: String::from_utf8_lossy(&submitted.control_key).to_string(),
            })
        })
    }

    pub fn write_import_execute_batch(
        &self,
        workspace: &str,
        batch_bytes: &[u8],
        dry_run: bool,
    ) -> Result<ImportBatchExecuteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let result = execute_import_execution_batch(
                loom,
                ns,
                batch_bytes,
                dry_run,
                loom.effective_principal()?,
            )?;
            Ok(ImportBatchExecuteSummary {
                workspace: workspace.to_string(),
                workspace_id: ns.to_string(),
                profile: result.batch.profile,
                source_system: result.batch.source_system,
                source_scope: result.batch.source_scope,
                coverage: coverage_label(result.batch.coverage).to_string(),
                observed_at_ms: result.batch.observed_at_ms,
                payload_count: result.batch.payloads.len(),
                execution_digest: result.batch_digest.to_string(),
                control_key: String::from_utf8_lossy(&result.control_key).to_string(),
                changed: result.changed,
                dry_run: result.report.dry_run,
                rows_imported: result.report.rows_imported,
                operations_planned: result.report.operations_planned,
                operations_applied: result.report.operations_applied,
                skipped: result.report.skipped,
                bytes_in: result.report.bytes_in,
                bytes_stored: result.report.bytes_stored,
                warnings: result.report.warnings,
                fidelity_issues: result.report.fidelity_issues.len(),
            })
        })
    }

    pub fn write_meetings_import_snapshot(
        &self,
        workspace: &str,
        input_profile: &str,
        snapshot_bytes: &[u8],
        dry_run: bool,
    ) -> Result<MeetingsImportSnapshotSummary> {
        let input_profile = parse_meetings_input_profile(input_profile)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let result = import_meetings_bytes(loom, ns, input_profile, snapshot_bytes, dry_run)?;
            Ok(MeetingsImportSnapshotSummary {
                workspace: workspace.to_string(),
                workspace_id: ns.to_string(),
                input_profile: input_profile_label(input_profile).to_string(),
                source_scope: result.report.source_scope,
                changed: result.changed,
                dry_run: result.report.dry_run,
                rows_imported: result.report.rows_imported,
                operations_planned: result.report.operations_planned,
                operations_applied: result.report.operations_applied,
                bytes_in: result.report.bytes_in,
                bytes_stored: result.report.bytes_stored,
                payload_bytes: result.payload_bytes,
                warnings: result.report.warnings,
                fidelity_issues: result.report.fidelity_issues.len(),
            })
        })
    }

    pub fn write_redmine_import_snapshot(
        &self,
        workspace: &str,
        profile: &str,
        source_path: Option<&str>,
        snapshot_bytes: &[u8],
        field_policy: Option<&str>,
        dry_run: bool,
    ) -> Result<RedmineImportSnapshotSummary> {
        let source_path = source_path.unwrap_or("mcp:redmine");
        let field_policy = field_policy
            .map(TicketImportFieldPolicy::parse)
            .transpose()?
            .unwrap_or(TicketImportFieldPolicy::Strict);
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let report = match field_policy {
                TicketImportFieldPolicy::Strict => {
                    import_redmine_bytes(loom, ns, profile, source_path, snapshot_bytes, dry_run)?
                }
                TicketImportFieldPolicy::Infer => import_redmine_bytes_with_field_policy(
                    loom,
                    ns,
                    profile,
                    source_path,
                    snapshot_bytes,
                    dry_run,
                    field_policy,
                )?,
            };
            Ok(RedmineImportSnapshotSummary {
                workspace: workspace.to_string(),
                workspace_id: ns.to_string(),
                profile: profile.to_string(),
                source_scope: report.source_scope,
                dry_run: report.dry_run,
                rows_imported: report.rows_imported,
                operations_planned: report.operations_planned,
                operations_applied: report.operations_applied,
                skipped: report.skipped,
                bytes_in: report.bytes_in,
                bytes_stored: report.bytes_stored,
                warnings: report.warnings,
                fidelity_issues: report.fidelity_issues.len(),
            })
        })
    }

    pub fn write_studio_reindex(
        &self,
        workspace: &str,
        profile: Option<&str>,
    ) -> Result<StudioReindexSummary> {
        let profile = profile.unwrap_or("all");
        self.store.write(|loom| {
            let local_binding = resolve_optional_vector_binding(loom, workspace)?;
            apply_studio_reindex(loom, workspace, profile, local_binding.as_ref())
        })
    }

    pub fn write_meetings_accept_annotation(
        &self,
        workspace: &str,
        profile_id: &str,
        annotation_id: &str,
    ) -> Result<MeetingsAnnotationReviewSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Meetings, AclRight::Write)?;
            let principal = loom.effective_principal()?.unwrap_or(ns).to_string();
            let mut snapshot = require_meetings_snapshot(loom, profile_id)?;
            let reviewed_at_ms = now_ms();
            let annotation =
                snapshot.accept_annotation(annotation_id, principal, reviewed_at_ms)?;
            save_meetings_snapshot(
                loom,
                ns,
                profile_id,
                &snapshot,
                "meetings.annotation.accept",
                annotation_id,
            )?;
            let body = annotation.encode()?;
            update_meetings_review_revision_index(
                loom,
                ns,
                profile_id,
                format!("meetings:annotation:{annotation_id}"),
                format!("meetings.annotation.accept:{profile_id}:{annotation_id}"),
                &body,
                "application/vnd.uldren.loom.meetings.annotation+cbor",
                reviewed_at_ms,
            )?;
            annotation_review_summary(profile_id, annotation)
        })
    }

    pub fn write_meetings_reject_annotation(
        &self,
        workspace: &str,
        profile_id: &str,
        annotation_id: &str,
    ) -> Result<MeetingsAnnotationReviewSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Meetings, AclRight::Write)?;
            let mut snapshot = require_meetings_snapshot(loom, profile_id)?;
            let reviewed_at_ms = now_ms();
            let annotation = snapshot.reject_annotation(annotation_id)?;
            save_meetings_snapshot(
                loom,
                ns,
                profile_id,
                &snapshot,
                "meetings.annotation.reject",
                annotation_id,
            )?;
            let body = annotation.encode()?;
            update_meetings_review_revision_index(
                loom,
                ns,
                profile_id,
                format!("meetings:annotation:{annotation_id}"),
                format!("meetings.annotation.reject:{profile_id}:{annotation_id}"),
                &body,
                "application/vnd.uldren.loom.meetings.annotation+cbor",
                reviewed_at_ms,
            )?;
            annotation_review_summary(profile_id, annotation)
        })
    }

    pub fn write_meetings_propose_vocabulary(
        &self,
        workspace: &str,
        profile_id: &str,
        input: VocabularyTermInput<'_>,
        aliases: Vec<String>,
    ) -> Result<MeetingsVocabularyReviewSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Meetings, AclRight::Write)?;
            let mut term = VocabularyTermRecord::new(input)?;
            term.aliases = aliases;
            let mut snapshot = require_meetings_snapshot(loom, profile_id)?;
            let term = snapshot.add_vocabulary_term(term)?;
            save_meetings_snapshot(
                loom,
                ns,
                profile_id,
                &snapshot,
                "meetings.vocabulary.propose",
                &term.term_id,
            )?;
            let body = term.encode()?;
            update_meetings_review_revision_index(
                loom,
                ns,
                profile_id,
                format!("meetings:vocabulary:{}", term.term_id),
                format!("meetings.vocabulary.propose:{profile_id}:{}", term.term_id),
                &body,
                "application/vnd.uldren.loom.meetings.vocabulary-term+cbor",
                term.created_at_ms,
            )?;
            vocabulary_review_summary(profile_id, term)
        })
    }

    pub fn write_meetings_accept_vocabulary(
        &self,
        workspace: &str,
        profile_id: &str,
        term_id: &str,
    ) -> Result<MeetingsVocabularyReviewSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Meetings, AclRight::Write)?;
            let reviewer = loom.effective_principal()?.unwrap_or(ns).to_string();
            let mut snapshot = require_meetings_snapshot(loom, profile_id)?;
            let reviewed_at_ms = now_ms();
            let term = snapshot.accept_vocabulary_term(term_id, reviewer, reviewed_at_ms)?;
            save_meetings_snapshot(
                loom,
                ns,
                profile_id,
                &snapshot,
                "meetings.vocabulary.accept",
                term_id,
            )?;
            let body = term.encode()?;
            update_meetings_review_revision_index(
                loom,
                ns,
                profile_id,
                format!("meetings:vocabulary:{term_id}"),
                format!("meetings.vocabulary.accept:{profile_id}:{term_id}"),
                &body,
                "application/vnd.uldren.loom.meetings.vocabulary-term+cbor",
                reviewed_at_ms,
            )?;
            vocabulary_review_summary(profile_id, term)
        })
    }

    pub fn write_meetings_reject_vocabulary(
        &self,
        workspace: &str,
        profile_id: &str,
        term_id: &str,
    ) -> Result<MeetingsVocabularyReviewSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Meetings, AclRight::Write)?;
            let reviewer = loom.effective_principal()?.unwrap_or(ns).to_string();
            let mut snapshot = require_meetings_snapshot(loom, profile_id)?;
            let reviewed_at_ms = now_ms();
            let term = snapshot.reject_vocabulary_term(term_id, reviewer, reviewed_at_ms)?;
            save_meetings_snapshot(
                loom,
                ns,
                profile_id,
                &snapshot,
                "meetings.vocabulary.reject",
                term_id,
            )?;
            let body = term.encode()?;
            update_meetings_review_revision_index(
                loom,
                ns,
                profile_id,
                format!("meetings:vocabulary:{term_id}"),
                format!("meetings.vocabulary.reject:{profile_id}:{term_id}"),
                &body,
                "application/vnd.uldren.loom.meetings.vocabulary-term+cbor",
                reviewed_at_ms,
            )?;
            vocabulary_review_summary(profile_id, term)
        })
    }

    pub fn write_meetings_add_entity_merge(
        &self,
        workspace: &str,
        profile_id: &str,
        merge_id: &str,
        canonical_entity_id: &str,
        merged_entity_ids: Vec<String>,
        evidence_annotation_ids: Vec<String>,
    ) -> Result<MeetingsEntityMergeWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Meetings, AclRight::Write)?;
            let decided_by = loom.effective_principal()?.unwrap_or(ns).to_string();
            let merge = EntityMergeRecord::new(EntityMergeInput {
                merge_id,
                canonical_entity_id,
                merged_entity_ids,
                evidence_annotation_ids,
                decided_by: &decided_by,
                decided_at_ms: now_ms(),
            })?;
            let mut snapshot = require_meetings_snapshot(loom, profile_id)?;
            let merge = snapshot.add_entity_merge(merge)?;
            save_meetings_snapshot(
                loom,
                ns,
                profile_id,
                &snapshot,
                "meetings.entity.merge",
                &merge.merge_id,
            )?;
            let body = merge.encode()?;
            update_meetings_review_revision_index(
                loom,
                ns,
                profile_id,
                format!("meetings:entity-merge:{}", merge.merge_id),
                format!("meetings.entity.merge:{profile_id}:{}", merge.merge_id),
                &body,
                "application/vnd.uldren.loom.meetings.entity-merge+cbor",
                merge.decided_at_ms,
            )?;
            entity_merge_summary(profile_id, merge)
        })
    }

    pub fn write_meetings_add_promotion(
        &self,
        workspace: &str,
        profile_id: &str,
        promotion_id: &str,
        operation_kind: &str,
        source_annotation_id: &str,
        target_profile: &str,
        target_entity_ref: &str,
    ) -> Result<MeetingsPromotionWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Meetings, AclRight::Write)?;
            let promoted_by = loom.effective_principal()?.unwrap_or(ns).to_string();
            let promoted_at_ms = now_ms();
            let mut snapshot = require_meetings_snapshot(loom, profile_id)?;
            let promotion = snapshot.add_promotion(
                loom_substrate::meetings::PromotionRecord::new(PromotionInput {
                    promotion_id,
                    operation_kind,
                    source_annotation_id,
                    target_profile,
                    target_entity_ref,
                    promoted_by: &promoted_by,
                    promoted_at_ms,
                })?,
            )?;
            save_meetings_snapshot(
                loom,
                ns,
                profile_id,
                &snapshot,
                "meetings.promotion.add",
                &promotion.promotion_id,
            )?;
            let body = promotion.encode()?;
            update_meetings_review_revision_index(
                loom,
                ns,
                profile_id,
                format!("meetings:promotion:{}", promotion.promotion_id),
                format!(
                    "meetings.promotion.add:{profile_id}:{}",
                    promotion.promotion_id
                ),
                &body,
                "application/vnd.uldren.loom.meetings.promotion+cbor",
                promotion.promoted_at_ms,
            )?;
            Ok(MeetingsPromotionWriteSummary {
                workspace_id: profile_id.to_string(),
                promotion_id: promotion.promotion_id,
                operation_kind: promotion.operation_kind,
                source_annotation_id: promotion.source_annotation_id,
                target_profile: promotion.target_profile,
                target_entity_ref: promotion.target_entity_ref,
                record_cbor_hex: hex::encode(body),
            })
        })
    }

    pub fn write_meetings_promote_task_to_ticket(
        &self,
        workspace: &str,
        profile_id: &str,
        request: MeetingsPromoteTaskToTicketRequest<'_>,
    ) -> Result<MeetingsTicketPromotionWriteSummary> {
        let (summary, queued) = self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Meetings, AclRight::Write)?;
            let promoted_by = loom.effective_principal()?.unwrap_or(ns).to_string();
            let promoted_at_ms = now_ms();
            let mut snapshot = require_meetings_snapshot(loom, profile_id)?;
            if snapshot
                .promotions
                .iter()
                .any(|promotion| promotion.promotion_id == request.promotion_id)
            {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "meeting promotion already exists",
                ));
            }
            let annotation = snapshot
                .annotations
                .iter()
                .find(|annotation| annotation.annotation_id == request.source_annotation_id)
                .cloned()
                .ok_or_else(|| LoomError::not_found("meeting promotion annotation not found"))?;
            if annotation.kind != "Task" {
                return Err(LoomError::invalid(
                    "only Task annotations can be promoted to tickets",
                ));
            }
            if !matches!(
                annotation.status,
                AnnotationStatus::Observed | AnnotationStatus::Accepted
            ) {
                return Err(LoomError::invalid(
                    "meeting promotion requires observed or accepted annotation evidence",
                ));
            }
            loom_substrate::promotion::validate_studio_promotion(
                &annotation.kind,
                "task.promoted",
                "tickets",
                "ticket:pending",
            )?;
            loom_substrate::meetings::PromotionRecord::new(PromotionInput {
                promotion_id: request.promotion_id,
                operation_kind: "task.promoted",
                source_annotation_id: request.source_annotation_id,
                target_profile: "tickets",
                target_entity_ref: "ticket:pending",
                promoted_by: &promoted_by,
                promoted_at_ms,
            })?;
            let fields = json!({
                "title": annotation.label,
                "description": format!(
                    "Promoted from Meetings annotation {} in meeting {}.",
                    annotation.annotation_id, annotation.meeting_id
                ),
                "meeting_id": annotation.meeting_id,
                "meeting_annotation_id": annotation.annotation_id
            });
            let ticket = loom_tickets::create_ticket(
                loom,
                ns,
                TicketCreateRequest {
                    workspace_id: profile_id,
                    project_id: request.project_id,
                    ticket_type: request.ticket_type,
                    external_source: Some("meetings"),
                    external_id: Some(request.source_annotation_id),
                    fields: &fields,
                    policy_labels: request.policy_labels,
                    expected_root: request.expected_ticket_root,
                },
            )?;
            loom_tickets::update_ticket_field_references(
                loom,
                ns,
                &ticket.workspace_id,
                &ticket.ticket_id,
                &ticket.fields,
            )?;
            let queued = if let Some(operation_id) = ticket.operation_id.as_deref() {
                loom_tickets::enqueue_ticket_reference_candidates(
                    loom,
                    ns,
                    loom_tickets::TicketReferenceCandidateRequest {
                        workspace_id: &ticket.workspace_id,
                        ticket_id: &ticket.ticket_id,
                        operation_id,
                        source_root: Digest::parse(&ticket.profile_root)?,
                        fields: &ticket.fields,
                        now_ms: now_ms(),
                    },
                )?
            } else {
                false
            };
            let target_entity_ref = format!("ticket:{}", ticket.ticket_id);
            let promotion = snapshot.add_promotion(
                loom_substrate::meetings::PromotionRecord::new(PromotionInput {
                    promotion_id: request.promotion_id,
                    operation_kind: "task.promoted",
                    source_annotation_id: request.source_annotation_id,
                    target_profile: "tickets",
                    target_entity_ref: &target_entity_ref,
                    promoted_by: &promoted_by,
                    promoted_at_ms,
                })?,
            )?;
            save_meetings_snapshot(
                loom,
                ns,
                profile_id,
                &snapshot,
                "meetings.promotion.add",
                &promotion.promotion_id,
            )?;
            let body = promotion.encode()?;
            update_meetings_review_revision_index(
                loom,
                ns,
                profile_id,
                format!("meetings:promotion:{}", promotion.promotion_id),
                format!(
                    "meetings.promotion.add:{profile_id}:{}",
                    promotion.promotion_id
                ),
                &body,
                "application/vnd.uldren.loom.meetings.promotion+cbor",
                promotion.promoted_at_ms,
            )?;
            let promotion = MeetingsPromotionWriteSummary {
                workspace_id: profile_id.to_string(),
                promotion_id: promotion.promotion_id,
                operation_kind: promotion.operation_kind,
                source_annotation_id: promotion.source_annotation_id,
                target_profile: promotion.target_profile,
                target_entity_ref: promotion.target_entity_ref,
                record_cbor_hex: hex::encode(body),
            };
            Ok((
                MeetingsTicketPromotionWriteSummary { promotion, ticket },
                queued,
            ))
        })?;
        if queued {
            self.store.signal_reference_reconcile()?;
        }
        Ok(summary)
    }

    pub fn write_meetings_promote_decision_to_decision_log(
        &self,
        workspace: &str,
        profile_id: &str,
        request: MeetingsPromoteDecisionToDecisionLogRequest<'_>,
    ) -> Result<MeetingsDecisionPromotionWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Meetings, AclRight::Write)?;
            let promoted_by = loom.effective_principal()?.unwrap_or(ns).to_string();
            let promoted_at_ms = now_ms();
            let mut snapshot = require_meetings_snapshot(loom, profile_id)?;
            if snapshot
                .promotions
                .iter()
                .any(|promotion| promotion.promotion_id == request.promotion_id)
            {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "meeting promotion already exists",
                ));
            }
            let annotation = snapshot
                .annotations
                .iter()
                .find(|annotation| annotation.annotation_id == request.source_annotation_id)
                .cloned()
                .ok_or_else(|| LoomError::not_found("meeting promotion annotation not found"))?;
            if annotation.kind != "Decision" {
                return Err(LoomError::invalid(
                    "only Decision annotations can be promoted to the decision log",
                ));
            }
            if !matches!(
                annotation.status,
                AnnotationStatus::Observed | AnnotationStatus::Accepted
            ) {
                return Err(LoomError::invalid(
                    "meeting promotion requires observed or accepted annotation evidence",
                ));
            }
            let target_entity_ref = format!("decision:{}", request.decision_id);
            loom_substrate::promotion::validate_studio_promotion(
                &annotation.kind,
                "decision.promoted",
                "decision-log",
                &target_entity_ref,
            )?;
            loom_substrate::meetings::PromotionRecord::new(PromotionInput {
                promotion_id: request.promotion_id,
                operation_kind: "decision.promoted",
                source_annotation_id: request.source_annotation_id,
                target_profile: "decision-log",
                target_entity_ref: &target_entity_ref,
                promoted_by: &promoted_by,
                promoted_at_ms,
            })?;
            let payload = serde_json::to_vec(&json!({
                "schema": "loom.studio.decision-log.entry.v1",
                "decision_id": request.decision_id,
                "label": annotation.label,
                "meeting_id": annotation.meeting_id,
                "meeting_annotation_id": annotation.annotation_id,
                "promoted_by": promoted_by.as_str(),
                "promoted_at_ms": promoted_at_ms
            }))
            .map_err(|e| LoomError::invalid(format!("decision log payload: {e}")))?;
            let ledger_sequence = ledger_append(loom, ns, request.ledger_name, payload)?;
            let promotion = snapshot.add_promotion(
                loom_substrate::meetings::PromotionRecord::new(PromotionInput {
                    promotion_id: request.promotion_id,
                    operation_kind: "decision.promoted",
                    source_annotation_id: request.source_annotation_id,
                    target_profile: "decision-log",
                    target_entity_ref: &target_entity_ref,
                    promoted_by: &promoted_by,
                    promoted_at_ms,
                })?,
            )?;
            save_meetings_snapshot(
                loom,
                ns,
                profile_id,
                &snapshot,
                "meetings.promotion.add",
                &promotion.promotion_id,
            )?;
            let body = promotion.encode()?;
            update_meetings_review_revision_index(
                loom,
                ns,
                profile_id,
                format!("meetings:promotion:{}", promotion.promotion_id),
                format!(
                    "meetings.promotion.add:{profile_id}:{}",
                    promotion.promotion_id
                ),
                &body,
                "application/vnd.uldren.loom.meetings.promotion+cbor",
                promotion.promoted_at_ms,
            )?;
            let promotion = MeetingsPromotionWriteSummary {
                workspace_id: profile_id.to_string(),
                promotion_id: promotion.promotion_id,
                operation_kind: promotion.operation_kind,
                source_annotation_id: promotion.source_annotation_id,
                target_profile: promotion.target_profile,
                target_entity_ref: promotion.target_entity_ref,
                record_cbor_hex: hex::encode(body),
            };
            Ok(MeetingsDecisionPromotionWriteSummary {
                promotion,
                decision_ledger: request.ledger_name.to_string(),
                ledger_sequence,
            })
        })
    }

    pub fn write_meetings_promote_question_to_lifecycle(
        &self,
        workspace: &str,
        profile_id: &str,
        request: MeetingsPromoteQuestionToLifecycleRequest<'_>,
    ) -> Result<MeetingsLifecyclePromotionWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Meetings, AclRight::Write)?;
            let promoted_by = loom.effective_principal()?.unwrap_or(ns).to_string();
            let promoted_at_ms = now_ms();
            let mut snapshot = require_meetings_snapshot(loom, profile_id)?;
            if snapshot
                .promotions
                .iter()
                .any(|promotion| promotion.promotion_id == request.promotion_id)
            {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "meeting promotion already exists",
                ));
            }
            let annotation = snapshot
                .annotations
                .iter()
                .find(|annotation| annotation.annotation_id == request.source_annotation_id)
                .cloned()
                .ok_or_else(|| LoomError::not_found("meeting promotion annotation not found"))?;
            if annotation.kind != "Question" {
                return Err(LoomError::invalid(
                    "only Question annotations can be promoted to lifecycle",
                ));
            }
            if !matches!(
                annotation.status,
                AnnotationStatus::Observed | AnnotationStatus::Accepted
            ) {
                return Err(LoomError::invalid(
                    "meeting promotion requires observed or accepted annotation evidence",
                ));
            }
            let target_entity_ref = format!("lifecycle:{}", request.instance_id);
            loom_substrate::promotion::validate_studio_promotion(
                &annotation.kind,
                "question.promoted",
                "lifecycle",
                &target_entity_ref,
            )?;
            loom_substrate::meetings::PromotionRecord::new(PromotionInput {
                promotion_id: request.promotion_id,
                operation_kind: "question.promoted",
                source_annotation_id: request.source_annotation_id,
                target_profile: "lifecycle",
                target_entity_ref: &target_entity_ref,
                promoted_by: &promoted_by,
                promoted_at_ms,
            })?;
            let lifecycle = loom_lifecycle::instantiate(
                loom,
                ns,
                profile_id,
                request.instance_id,
                request.definition_id,
                vec![
                    format!("meeting:{}", annotation.meeting_id),
                    format!("meeting-annotation:{}", annotation.annotation_id),
                ],
            )?;
            let promotion = snapshot.add_promotion(
                loom_substrate::meetings::PromotionRecord::new(PromotionInput {
                    promotion_id: request.promotion_id,
                    operation_kind: "question.promoted",
                    source_annotation_id: request.source_annotation_id,
                    target_profile: "lifecycle",
                    target_entity_ref: &target_entity_ref,
                    promoted_by: &promoted_by,
                    promoted_at_ms,
                })?,
            )?;
            save_meetings_snapshot(
                loom,
                ns,
                profile_id,
                &snapshot,
                "meetings.promotion.add",
                &promotion.promotion_id,
            )?;
            let body = promotion.encode()?;
            update_meetings_review_revision_index(
                loom,
                ns,
                profile_id,
                format!("meetings:promotion:{}", promotion.promotion_id),
                format!(
                    "meetings.promotion.add:{profile_id}:{}",
                    promotion.promotion_id
                ),
                &body,
                "application/vnd.uldren.loom.meetings.promotion+cbor",
                promotion.promoted_at_ms,
            )?;
            let promotion = MeetingsPromotionWriteSummary {
                workspace_id: profile_id.to_string(),
                promotion_id: promotion.promotion_id,
                operation_kind: promotion.operation_kind,
                source_annotation_id: promotion.source_annotation_id,
                target_profile: promotion.target_profile,
                target_entity_ref: promotion.target_entity_ref,
                record_cbor_hex: hex::encode(body),
            };
            Ok(MeetingsLifecyclePromotionWriteSummary {
                promotion,
                lifecycle,
            })
        })
    }

    pub fn write_meetings_promote_artifact_to_reference_artifact(
        &self,
        workspace: &str,
        profile_id: &str,
        request: MeetingsPromoteArtifactToReferenceArtifactRequest<'_>,
    ) -> Result<MeetingsReferenceArtifactPromotionWriteSummary> {
        self.write_meetings_promote_to_reference_artifact(
            workspace,
            profile_id,
            MeetingsReferenceArtifactPromotionInput {
                source_kind: "Artifact",
                operation_kind: "artifact.promoted",
                artifact_kind: ReferenceArtifactKind::Artifact,
                record_id: request.artifact_id,
                promotion_id: request.promotion_id,
                source_annotation_id: request.source_annotation_id,
                target_ref: request.target_ref,
            },
        )
    }

    pub fn write_meetings_promote_reference_to_reference_artifact(
        &self,
        workspace: &str,
        profile_id: &str,
        request: MeetingsPromoteReferenceToReferenceArtifactRequest<'_>,
    ) -> Result<MeetingsReferenceArtifactPromotionWriteSummary> {
        self.write_meetings_promote_to_reference_artifact(
            workspace,
            profile_id,
            MeetingsReferenceArtifactPromotionInput {
                source_kind: "Reference",
                operation_kind: "reference.promoted",
                artifact_kind: ReferenceArtifactKind::Reference,
                record_id: request.reference_id,
                promotion_id: request.promotion_id,
                source_annotation_id: request.source_annotation_id,
                target_ref: request.target_ref,
            },
        )
    }

    fn write_meetings_promote_to_reference_artifact(
        &self,
        workspace: &str,
        profile_id: &str,
        input: MeetingsReferenceArtifactPromotionInput<'_>,
    ) -> Result<MeetingsReferenceArtifactPromotionWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Meetings, AclRight::Write)?;
            let promoted_by = loom.effective_principal()?.unwrap_or(ns).to_string();
            let promoted_at_ms = now_ms();
            let mut snapshot = require_meetings_snapshot(loom, profile_id)?;
            if snapshot
                .promotions
                .iter()
                .any(|promotion| promotion.promotion_id == input.promotion_id)
            {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "meeting promotion already exists",
                ));
            }
            let annotation = snapshot
                .annotations
                .iter()
                .find(|annotation| annotation.annotation_id == input.source_annotation_id)
                .cloned()
                .ok_or_else(|| LoomError::not_found("meeting promotion annotation not found"))?;
            if annotation.kind != input.source_kind {
                return Err(LoomError::invalid(format!(
                    "only {} annotations can be promoted to references",
                    input.source_kind
                )));
            }
            if !matches!(
                annotation.status,
                AnnotationStatus::Observed | AnnotationStatus::Accepted
            ) {
                return Err(LoomError::invalid(
                    "meeting promotion requires observed or accepted annotation evidence",
                ));
            }
            let target_entity_ref = format!("{}:{}", input.artifact_kind.as_str(), input.record_id);
            loom_substrate::promotion::validate_studio_promotion(
                &annotation.kind,
                input.operation_kind,
                "references",
                &target_entity_ref,
            )?;
            loom_substrate::meetings::PromotionRecord::new(PromotionInput {
                promotion_id: input.promotion_id,
                operation_kind: input.operation_kind,
                source_annotation_id: input.source_annotation_id,
                target_profile: "references",
                target_entity_ref: &target_entity_ref,
                promoted_by: &promoted_by,
                promoted_at_ms,
            })?;
            let source_ref = format!("meeting-annotation:{}", annotation.annotation_id);
            let reference_artifact = loom_reference::create_reference_artifact(
                loom,
                ns,
                ReferenceArtifactCreateRequest {
                    workspace_id: profile_id,
                    record_id: input.record_id,
                    kind: input.artifact_kind,
                    label: &annotation.label,
                    source_ref: &source_ref,
                    source_operation_id: input.promotion_id,
                    target_ref: input.target_ref,
                    created_by: &promoted_by,
                    created_at_ms: promoted_at_ms,
                },
            )?;
            let promotion = snapshot.add_promotion(
                loom_substrate::meetings::PromotionRecord::new(PromotionInput {
                    promotion_id: input.promotion_id,
                    operation_kind: input.operation_kind,
                    source_annotation_id: input.source_annotation_id,
                    target_profile: "references",
                    target_entity_ref: &target_entity_ref,
                    promoted_by: &promoted_by,
                    promoted_at_ms,
                })?,
            )?;
            save_meetings_snapshot(
                loom,
                ns,
                profile_id,
                &snapshot,
                "meetings.promotion.add",
                &promotion.promotion_id,
            )?;
            let body = promotion.encode()?;
            update_meetings_review_revision_index(
                loom,
                ns,
                profile_id,
                format!("meetings:promotion:{}", promotion.promotion_id),
                format!(
                    "meetings.promotion.add:{profile_id}:{}",
                    promotion.promotion_id
                ),
                &body,
                "application/vnd.uldren.loom.meetings.promotion+cbor",
                promotion.promoted_at_ms,
            )?;
            let promotion = MeetingsPromotionWriteSummary {
                workspace_id: profile_id.to_string(),
                promotion_id: promotion.promotion_id,
                operation_kind: promotion.operation_kind,
                source_annotation_id: promotion.source_annotation_id,
                target_profile: promotion.target_profile,
                target_entity_ref: promotion.target_entity_ref,
                record_cbor_hex: hex::encode(body),
            };
            Ok(MeetingsReferenceArtifactPromotionWriteSummary {
                promotion,
                reference_artifact,
            })
        })
    }

    // ---- cas ----

    /// `cas.put`: store `content` in the workspace's CAS; returns its content address.
    pub fn write_cas_put(&self, workspace: &str, content: &[u8]) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.cas_put(workspace, content);
        }
        self.store
            .write(|loom| apply_cas_put(loom, workspace, content))
    }

    /// `cas.delete`: unlink `digest` from the workspace's working tree; returns whether it was present.
    pub fn write_cas_delete(&self, workspace: &str, digest: &str) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.cas_delete(workspace, digest);
        }
        self.store
            .write(|loom| apply_cas_delete(loom, workspace, digest))
    }

    // ---- graph ----

    /// `graph.upsert_node`: insert or replace a node with canonical-CBOR properties.
    pub fn write_graph_upsert_node(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
        props: &[u8],
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_upsert_node(workspace, name, id, props);
        }
        let props = props_from_cbor(props)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            graph_upsert_node(loom, ns, name, id, props)
        })
    }

    /// `graph.remove_node`: remove a node, optionally cascading incident edges.
    pub fn write_graph_remove_node(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
        cascade: bool,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_remove_node(workspace, name, id, cascade);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            graph_remove_node(loom, ns, name, id, cascade)
        })
    }

    /// `graph.upsert_edge`: insert or replace a directed labelled edge.
    pub fn write_graph_upsert_edge(
        &self,
        workspace: &str,
        name: &str,
        edge: GraphEdgeWrite<'_>,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_upsert_edge_indexed(workspace, name, edge);
        }
        let props = props_from_cbor(edge.props)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_reference::upsert_graph_edge_indexed(
                loom, ns, name, edge.id, edge.src, edge.dst, edge.label, props,
            )
        })
    }

    /// `graph.remove_edge`: remove an edge and return whether it was present.
    pub fn write_graph_remove_edge(&self, workspace: &str, name: &str, id: &str) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_remove_edge_indexed(workspace, name, id);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_reference::remove_graph_edge_indexed(loom, ns, name, id)
        })
    }

    // ---- vector ----

    /// `vector.create`: create an empty vector set.
    pub fn write_vector_create(
        &self,
        workspace: &str,
        name: &str,
        dim: u64,
        metric: i32,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_create(workspace, name, dim, metric);
        }
        let dim =
            usize::try_from(dim).map_err(|_| LoomError::invalid("vector dim out of range"))?;
        let metric = vector_metric(metric)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            vector_create(loom, ns, name, dim, metric)
        })
    }

    /// `vector.upsert`: insert or replace a vector and metadata.
    pub fn write_vector_upsert(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
        vector: &[u8],
        metadata: &[u8],
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_upsert(workspace, name, id, vector, metadata);
        }
        let vector = vector_from_bytes(vector)?;
        let metadata = vector_metadata_from_cbor(metadata)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            vector_upsert(loom, ns, name, id, vector, metadata)
        })
    }

    /// `vector.upsert_source`: insert or replace a vector with stored source text.
    pub fn write_vector_upsert_source(
        &self,
        workspace: &str,
        name: &str,
        args: VectorSourceWrite<'_>,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_upsert_source(
                workspace,
                name,
                crate::RemoteVectorUpsertSource {
                    id: args.id,
                    vector: args.vector,
                    metadata: args.metadata,
                    source_text: args.source_text,
                    model_id: args.model_id,
                    weights_digest: args.weights_digest,
                },
            );
        }
        let vector = vector_from_bytes(args.vector)?;
        let source_text = std::str::from_utf8(args.source_text)
            .map_err(|e| LoomError::invalid(format!("vector source_text must be UTF-8: {e}")))?;
        let metadata = vector_metadata_from_cbor(args.metadata)?;
        let model = args.model_id.map(|model_id| {
            EmbeddingModel::new(
                model_id.to_string(),
                vector.len(),
                args.weights_digest.map(str::to_string),
            )
        });
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            vector_upsert_with_source(
                loom,
                ns,
                name,
                args.id,
                vector,
                metadata,
                source_text,
                model,
            )
        })
    }

    /// `vector.create_metadata_index`: declare and build one metadata equality index.
    pub fn write_vector_create_metadata_index(
        &self,
        workspace: &str,
        name: &str,
        key: &str,
    ) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_create_metadata_index(workspace, name, key);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            vector_create_metadata_index(loom, ns, name, key)
        })
    }

    /// `vector.drop_metadata_index`: drop one metadata equality index.
    pub fn write_vector_drop_metadata_index(
        &self,
        workspace: &str,
        name: &str,
        key: &str,
    ) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_drop_metadata_index(workspace, name, key);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            vector_drop_metadata_index(loom, ns, name, key)
        })
    }

    /// `vector.delete`: remove a vector id and return whether it was present.
    pub fn write_vector_delete(&self, workspace: &str, name: &str, id: &str) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_delete(workspace, name, id);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            vector_delete(loom, ns, name, id)
        })
    }

    // ---- columnar ----

    /// `columnar.create`: create an empty dataset from canonical-CBOR columns.
    pub fn write_columnar_create(
        &self,
        workspace: &str,
        name: &str,
        columns: &[u8],
        target_segment_rows: u64,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.columnar_create(workspace, name, columns, target_segment_rows);
        }
        let columns = columnar_columns_from_cbor(columns)?;
        let target_segment_rows = usize::try_from(target_segment_rows)
            .map_err(|_| LoomError::invalid("columnar target_segment_rows out of range"))?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            columnar_create(loom, ns, name, columns, target_segment_rows)
        })
    }

    /// `columnar.append`: append one canonical-CBOR row.
    pub fn write_columnar_append(&self, workspace: &str, name: &str, row: &[u8]) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.columnar_append(workspace, name, row);
        }
        let row = columnar_row_from_cbor(row)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            columnar_append(loom, ns, name, row)
        })
    }

    /// `columnar.compact`: re-chunk a dataset at its target segment size.
    pub fn write_columnar_compact(&self, workspace: &str, name: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.columnar_compact(workspace, name);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            columnar_compact(loom, ns, name)
        })
    }

    // ---- dataframe ----

    /// `dataframe.create`: create a frame from canonical `DataframePlan` CBOR.
    pub fn write_dataframe_create(&self, workspace: &str, name: &str, plan: &[u8]) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.dataframe_create(workspace, name, plan);
        }
        let plan = DataframePlan::decode(plan)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            dataframe_create(loom, ns, name, &plan)
        })
    }

    /// `dataframe.materialize`: materialize a frame and return an optional `algo:hex` digest.
    pub fn write_dataframe_materialize(
        &self,
        workspace: &str,
        name: &str,
    ) -> Result<Option<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.dataframe_materialize(workspace, name);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(dataframe_materialize(loom, ns, name)?.map(|digest| digest.to_string()))
        })
    }

    // ---- fts ----

    /// `fts_create`: create a collection from canonical CBOR mapping.
    pub fn write_fts_create(&self, workspace: &str, name: &str, mapping: &[u8]) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.search_create(workspace, name, mapping);
        }
        let mapping = search_mapping_from_cbor(mapping)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            search_create(loom, ns, name, mapping)
        })
    }

    /// `fts_index`: insert or replace one document.
    pub fn write_fts_index(
        &self,
        workspace: &str,
        name: &str,
        id: Vec<u8>,
        doc: &[u8],
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.search_index(workspace, name, id, doc);
        }
        let doc = search_document_from_cbor(doc)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            search_index(loom, ns, name, id, doc)
        })
    }

    /// `fts_delete`: remove one document and return whether it was present.
    pub fn write_fts_delete(&self, workspace: &str, name: &str, id: &[u8]) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.search_delete(workspace, name, id);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            search_delete(loom, ns, name, id)
        })
    }

    /// `fts_remap`: replace a collection mapping.
    pub fn write_fts_remap(&self, workspace: &str, name: &str, mapping: &[u8]) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.search_remap(workspace, name, mapping);
        }
        let mapping = search_mapping_from_cbor(mapping)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            search_remap(loom, ns, name, mapping)
        })
    }

    // ---- kv ----

    /// `kv.put`: set the canonical-CBOR typed `key_cbor` to `value` in map `name`.
    pub fn write_kv_put(
        &self,
        workspace: &str,
        name: &str,
        key_cbor: &[u8],
        value: Vec<u8>,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.kv_put(workspace, name, key_cbor, value);
        }
        let key = key_from_cbor(key_cbor)?;
        if let Some((paths, session, auth)) = self.store.daemon_session_parts()? {
            let target = self.store.read(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                Ok((ns.to_string(), loom.kv_map_config(ns, name).tier))
            })?;
            if target.1 == loom_core::KvTier::Ephemeral {
                return daemon::kv_ephemeral_put_auth(
                    paths,
                    daemon::KvPutRequest {
                        session,
                        workspace: &target.0,
                        name,
                        key_cbor,
                        value: &value,
                        now_ms: now_ms(),
                    },
                    auth,
                );
            }
        }
        let has_runtime_state = self.store.has_runtime_state();
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            reject_stateless_ephemeral_kv(loom, has_runtime_state, ns, name)?;
            loom.kv_put_configured(ns, name, key, value, None, now_ms())
        })
    }

    /// `kv.delete`: remove the canonical-CBOR typed `key_cbor` from `name`; returns whether present.
    pub fn write_kv_delete(&self, workspace: &str, name: &str, key_cbor: &[u8]) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.kv_delete(workspace, name, key_cbor);
        }
        let key = key_from_cbor(key_cbor)?;
        if let Some((paths, session, auth)) = self.store.daemon_session_parts()? {
            let target = self.store.read(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                Ok((ns.to_string(), loom.kv_map_config(ns, name).tier))
            })?;
            if target.1 == loom_core::KvTier::Ephemeral {
                return daemon::kv_ephemeral_delete_auth(
                    paths, session, &target.0, name, key_cbor, auth,
                );
            }
        }
        let has_runtime_state = self.store.has_runtime_state();
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            reject_stateless_ephemeral_kv(loom, has_runtime_state, ns, name)?;
            loom.kv_delete_configured(ns, name, &key)
        })
    }

    // ---- document ----

    /// `document.put`: store document `id` (bytes `doc`) in collection `name`.
    pub fn write_document_put(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
        doc: Vec<u8>,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.document_put_binary_indexed(workspace, name, id, doc);
        }
        self.store
            .write(|loom| apply_document_put(loom, workspace, name, id, doc))
    }

    pub fn write_document_put_text(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
        text: &str,
        expected_entity_tag: Option<&str>,
    ) -> Result<loom_core::document::DocumentPutResult> {
        if let Some(backend) = self.store.remote_backend() {
            return backend
                .document_put_text_indexed(workspace, name, id, text, expected_entity_tag)
                .map(|digest| loom_core::document::DocumentPutResult {
                    entity_tag: loom_core::document_entity_tag_string_from_digest(digest),
                    digest,
                });
        }
        self.store.write(|loom| {
            apply_document_put_text(loom, workspace, name, id, text, expected_entity_tag)
        })
    }

    pub fn write_document_put_binary(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
        bytes: Vec<u8>,
        expected_entity_tag: Option<&str>,
    ) -> Result<loom_core::document::DocumentPutResult> {
        if let Some(backend) = self.store.remote_backend() {
            return backend
                .document_put_binary_indexed_guarded(
                    workspace,
                    name,
                    id,
                    bytes,
                    expected_entity_tag,
                )
                .map(|digest| loom_core::document::DocumentPutResult {
                    entity_tag: loom_core::document_entity_tag_string_from_digest(digest),
                    digest,
                });
        }
        self.store.write(|loom| {
            apply_document_put_binary(loom, workspace, name, id, bytes, expected_entity_tag)
        })
    }

    pub fn write_workgraph_fact_put(
        &self,
        workspace: &str,
        workspace_id: &str,
        fact: Vec<u8>,
    ) -> Result<()> {
        self.store
            .write(|loom| apply_workgraph_fact_put(loom, workspace, workspace_id, fact))
    }

    /// `document.delete`: remove document `id` from `name`; returns whether it was present.
    pub fn write_document_delete(&self, workspace: &str, name: &str, id: &str) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.document_delete_indexed(workspace, name, id);
        }
        self.store
            .write(|loom| apply_document_delete(loom, workspace, name, id))
    }

    pub fn write_document_replace_text(
        &self,
        request: DocumentReplaceTextRequest<'_>,
    ) -> Result<DocumentReplaceTextResult> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.document_replace_text_indexed(request);
        }
        self.store
            .write(|loom| apply_document_replace_text(loom, request))
    }

    pub fn write_substrate_view_define(
        &self,
        request: SubstrateViewDefineRequest<'_>,
    ) -> Result<ViewDefinitionSummary> {
        self.store
            .write(|loom| apply_substrate_view_define(loom, request))
    }

    pub fn read_substrate_write_admission_policy(
        &self,
        workspace: &str,
        surface: &str,
        scope_id: &str,
    ) -> Result<Option<WriteAdmissionPolicySummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(
                ns,
                authorization_domain_for_surface(surface)?,
                AclRight::Admin,
            )?;
            let key = write_admission_policy_key(ns, surface, scope_id)?;
            loom.store()
                .control_get(&key)?
                .map(|bytes| {
                    WriteAdmissionPolicy::decode(&bytes).map(WriteAdmissionPolicySummary::from)
                })
                .transpose()
        })
    }

    pub fn write_substrate_write_admission_policy_set(
        &self,
        request: WriteAdmissionPolicyRequest<'_>,
    ) -> Result<WriteAdmissionPolicySummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, request.workspace)?;
            loom.authorize_domain(
                ns,
                authorization_domain_for_surface(request.surface)?,
                AclRight::Admin,
            )?;
            let policy = WriteAdmissionPolicy::new(
                ns,
                request.surface,
                request.scope_id,
                WriteAdmissionMode::parse(request.default_mode)?,
                request.mandatory_targets.to_vec(),
            )?;
            let key = write_admission_policy_key(ns, request.surface, request.scope_id)?;
            loom.store().control_set(&key, policy.encode()?)?;
            Ok(WriteAdmissionPolicySummary::from(policy))
        })
    }

    pub fn write_substrate_alias_bind(
        &self,
        workspace: &str,
        scope_id: &str,
        alias: &str,
        target: &str,
    ) -> Result<SubstrateAliasSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_file_path(ns, crate::substrate_refs::REF_INDEX_DIR, AclRight::Write)?;
            loom.authorize_file_path(ns, crate::substrate_refs::ALIAS_INDEX_PATH, AclRight::Write)?;
            if TicketKey::parse(alias).is_ok()
                && let Some(profile) = TicketProfileReader::open(loom, ns, scope_id)?
                && profile.prefix_exists(&TicketKey::parse(alias)?.prefix)?
            {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "ticket-key syntax is reserved by the ticket profile",
                ));
            }
            bind_alias(loom, ns, scope_id, alias, target).map(SubstrateAliasSummary::from)
        })
    }

    pub fn write_substrate_alias_release(
        &self,
        workspace: &str,
        scope_id: &str,
        alias: &str,
    ) -> Result<bool> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_file_path(ns, crate::substrate_refs::REF_INDEX_DIR, AclRight::Write)?;
            loom.authorize_file_path(ns, crate::substrate_refs::ALIAS_INDEX_PATH, AclRight::Write)?;
            release_alias(loom, ns, scope_id, alias)
        })
    }

    pub fn write_substrate_reference_reconcile(
        &self,
        workspace: &str,
        workspace_id: &str,
        max: usize,
    ) -> Result<crate::substrate_refs::ReferenceReconciliationSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let ticket = crate::substrate_refs::reconcile_ticket_references(
                loom,
                ns,
                workspace_id,
                now_ms(),
                max,
            )?;
            let chat = crate::substrate_refs::reconcile_chat_references(
                loom,
                ns,
                workspace_id,
                now_ms(),
                max.saturating_sub(ticket.processed as usize),
            )?;
            Ok(crate::substrate_refs::ReferenceReconciliationSummary {
                pending: chat.pending,
                resolved: chat.resolved,
                failed: chat.failed,
                processed: ticket.processed.saturating_add(chat.processed),
            })
        })
    }

    pub fn write_tickets_project_create(
        &self,
        workspace: &str,
        workspace_id: &str,
        project_id: &str,
        key_prefix: &str,
        name: &str,
        expected_root: Option<&str>,
    ) -> Result<TicketProjectSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::create_project(
                loom,
                ns,
                workspace_id,
                project_id,
                key_prefix,
                name,
                expected_root,
            )
        })
    }

    pub fn write_tickets_project_rekey(
        &self,
        workspace: &str,
        workspace_id: &str,
        project_id: &str,
        key_prefix: &str,
        expected_root: Option<&str>,
    ) -> Result<TicketProjectSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::rekey_project(
                loom,
                ns,
                workspace_id,
                project_id,
                key_prefix,
                expected_root,
            )
        })
    }

    pub fn write_tickets_project_settings_set(
        &self,
        workspace: &str,
        request: loom_tickets::TicketProjectSettingsRequest<'_>,
    ) -> Result<TicketProjectSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::set_project_settings(loom, ns, request)
        })
    }

    pub fn write_tickets_field_put(
        &self,
        workspace: &str,
        request: TicketFieldDefinitionWriteRequest<'_>,
    ) -> Result<TicketFieldCatalog> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::put_ticket_field_definition(loom, ns, request)
        })
    }

    pub fn write_tickets_field_retire(
        &self,
        workspace: &str,
        request: TicketFieldDefinitionRetireRequest<'_>,
    ) -> Result<TicketFieldCatalog> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::retire_ticket_field_definition(loom, ns, request)
        })
    }

    pub fn write_tickets_create(
        &self,
        workspace: &str,
        request: TicketCreateRequest<'_>,
    ) -> Result<TicketSummary> {
        let (ticket, queued) = self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let ticket = loom_tickets::create_ticket(loom, ns, request)?;
            loom_tickets::update_ticket_field_references(
                loom,
                ns,
                &ticket.workspace_id,
                &ticket.ticket_id,
                &ticket.fields,
            )?;
            let queued = if let Some(operation_id) = ticket.operation_id.as_deref() {
                loom_tickets::enqueue_ticket_reference_candidates(
                    loom,
                    ns,
                    loom_tickets::TicketReferenceCandidateRequest {
                        workspace_id: &ticket.workspace_id,
                        ticket_id: &ticket.ticket_id,
                        operation_id,
                        source_root: Digest::parse(&ticket.profile_root)?,
                        fields: &ticket.fields,
                        now_ms: now_ms(),
                    },
                )?
            } else {
                false
            };
            Ok((ticket, queued))
        })?;
        if queued {
            self.store.signal_reference_reconcile()?;
        }
        Ok(ticket)
    }

    pub fn write_tickets_update(
        &self,
        workspace: &str,
        request: TicketUpdateRequest<'_>,
    ) -> Result<TicketSummary> {
        let (ticket, queued) = self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let ticket = loom_tickets::update_ticket(loom, ns, request)?;
            loom_tickets::update_ticket_field_references(
                loom,
                ns,
                &ticket.workspace_id,
                &ticket.ticket_id,
                &ticket.fields,
            )?;
            let queued = if let Some(operation_id) = ticket.operation_id.as_deref() {
                loom_tickets::enqueue_ticket_reference_candidates(
                    loom,
                    ns,
                    loom_tickets::TicketReferenceCandidateRequest {
                        workspace_id: &ticket.workspace_id,
                        ticket_id: &ticket.ticket_id,
                        operation_id,
                        source_root: Digest::parse(&ticket.profile_root)?,
                        fields: &ticket.fields,
                        now_ms: now_ms(),
                    },
                )?
            } else {
                false
            };
            Ok((ticket, queued))
        })?;
        if queued {
            self.store.signal_reference_reconcile()?;
        }
        Ok(ticket)
    }

    pub fn write_tickets_delete(
        &self,
        workspace: &str,
        request: TicketDeleteRequest<'_>,
    ) -> Result<TicketSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let ticket = loom_tickets::delete_ticket(loom, ns, request)?;
            loom_tickets::update_ticket_field_references(
                loom,
                ns,
                &ticket.workspace_id,
                &ticket.ticket_id,
                &ticket.fields,
            )?;
            Ok(ticket)
        })
    }

    pub fn write_tickets_create_receipt(
        &self,
        workspace: &str,
        request: TicketCreateRequest<'_>,
    ) -> Result<MutationEnvelope<TicketSummary>> {
        let root_before = request.expected_root.map(str::to_string);
        let ticket = self.write_tickets_create(workspace, request)?;
        Ok(ticket_receipt(
            "ticket.created",
            ticket,
            root_before,
            vec![MutationChange::ResourceCreated],
        ))
    }

    pub fn write_tickets_update_receipt(
        &self,
        workspace: &str,
        request: TicketUpdateRequest<'_>,
    ) -> Result<MutationEnvelope<TicketSummary>> {
        let root_before = request.expected_root.map(str::to_string);
        let changes = ticket_update_changes(&request);
        let ticket = self.write_tickets_update(workspace, request)?;
        Ok(ticket_receipt(
            "ticket.updated",
            ticket,
            root_before,
            changes,
        ))
    }

    pub fn write_tickets_delete_receipt(
        &self,
        workspace: &str,
        request: TicketDeleteRequest<'_>,
    ) -> Result<MutationEnvelope<TicketSummary>> {
        let root_before = request.expected_root.map(str::to_string);
        let ticket = self.write_tickets_delete(workspace, request)?;
        Ok(ticket_receipt(
            "ticket.deleted",
            ticket,
            root_before,
            vec![MutationChange::ResourceDeleted],
        ))
    }

    pub fn read_tickets_comments(
        &self,
        workspace: &str,
        ticket_id: &str,
    ) -> Result<Vec<TicketComment>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let pid = ns.to_string();
            loom_tickets::list_ticket_comments(loom, ns, &pid, ticket_id)
        })
    }

    pub fn write_tickets_comment_add_receipt(
        &self,
        workspace: &str,
        request: TicketCommentRequest<'_>,
    ) -> Result<MutationEnvelope<TicketSummary>> {
        let root_before = request.expected_root.map(str::to_string);
        let comment_id = request.comment_id.map(str::to_string);
        let comment_type = request
            .comment_type
            .unwrap_or(loom_tickets::TICKET_DEFAULT_COMMENT_TYPE)
            .to_string();
        let ticket = self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let ticket = loom_tickets::add_ticket_comment(loom, ns, request)?;
            loom_tickets::update_ticket_field_references(
                loom,
                ns,
                &ticket.workspace_id,
                &ticket.ticket_id,
                &ticket.fields,
            )?;
            Ok(ticket)
        })?;
        let mut changes = vec![MutationChange::field_set("comment_type", comment_type)];
        if let Some(comment_id) = comment_id {
            changes.push(MutationChange::field_set("comment_id", comment_id));
        }
        Ok(ticket_receipt(
            "ticket.comment_added",
            ticket,
            root_before,
            changes,
        ))
    }

    pub fn write_tickets_comment_update_receipt(
        &self,
        workspace: &str,
        request: TicketCommentUpdateRequest<'_>,
    ) -> Result<MutationEnvelope<TicketSummary>> {
        let root_before = request.expected_root.map(str::to_string);
        let mut changes = vec![MutationChange::field_changed(
            "comment_id",
            None::<String>,
            Some(request.comment_id.to_string()),
        )];
        if let Some(comment_type) = request.comment_type {
            changes.push(MutationChange::field_set(
                "comment_type",
                comment_type.to_string(),
            ));
        }
        if request.body.is_some() {
            changes.push(MutationChange::field_set("comment_body", "updated"));
        }
        let ticket = self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let ticket = loom_tickets::update_ticket_comment(loom, ns, request)?;
            loom_tickets::update_ticket_field_references(
                loom,
                ns,
                &ticket.workspace_id,
                &ticket.ticket_id,
                &ticket.fields,
            )?;
            Ok(ticket)
        })?;
        Ok(ticket_receipt(
            "ticket.comment_updated",
            ticket,
            root_before,
            changes,
        ))
    }

    pub fn write_tickets_comment_delete_receipt(
        &self,
        workspace: &str,
        request: TicketCommentDeleteRequest<'_>,
    ) -> Result<MutationEnvelope<TicketSummary>> {
        let root_before = request.expected_root.map(str::to_string);
        let comment_id = request.comment_id.to_string();
        let ticket = self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let ticket = loom_tickets::delete_ticket_comment(loom, ns, request)?;
            loom_tickets::update_ticket_field_references(
                loom,
                ns,
                &ticket.workspace_id,
                &ticket.ticket_id,
                &ticket.fields,
            )?;
            Ok(ticket)
        })?;
        Ok(ticket_receipt(
            "ticket.comment_deleted",
            ticket,
            root_before,
            vec![MutationChange::field_deleted("comment", Some(comment_id))],
        ))
    }

    pub fn write_tickets_board_create(
        &self,
        workspace: &str,
        request: BoardCreateRequest<'_>,
    ) -> Result<BoardSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::create_board(loom, ns, request)
        })
    }

    pub fn write_tickets_board_update(
        &self,
        workspace: &str,
        request: BoardUpdateRequest<'_>,
    ) -> Result<BoardSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::update_board(loom, ns, request)
        })
    }

    pub fn write_tickets_board_configure_columns(
        &self,
        workspace: &str,
        request: BoardColumnConfigureRequest<'_>,
    ) -> Result<BoardSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::configure_board_columns(loom, ns, request)
        })
    }

    pub fn write_tickets_board_move_card(
        &self,
        workspace: &str,
        request: BoardCardMoveRequest<'_>,
    ) -> Result<BoardSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::move_board_card(loom, ns, request)
        })
    }

    pub fn write_tickets_relation_set(
        &self,
        workspace: &str,
        request: TicketRelationRequest<'_>,
    ) -> Result<TicketRelationSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::add_ticket_relation(loom, ns, request)
        })
    }

    pub fn write_tickets_relation_set_receipt(
        &self,
        workspace: &str,
        request: TicketRelationRequest<'_>,
    ) -> Result<MutationEnvelope<TicketRelationSummary>> {
        let root_before = request.expected_root.map(str::to_string);
        let relation = self.write_tickets_relation_set(workspace, request)?;
        let change = MutationChange::relation_set(
            relation.relation_id.clone(),
            relation.kind.clone(),
            relation.target_id.clone(),
        );
        Ok(relation_receipt(
            "ticket.relation_set",
            relation,
            root_before,
            vec![change],
        ))
    }

    pub fn write_tickets_relation_remove(
        &self,
        workspace: &str,
        request: TicketRelationRemoveRequest<'_>,
    ) -> Result<TicketRelationSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::remove_ticket_relation(loom, ns, request)
        })
    }

    pub fn write_tickets_relation_remove_receipt(
        &self,
        workspace: &str,
        request: TicketRelationRemoveRequest<'_>,
    ) -> Result<MutationEnvelope<TicketRelationSummary>> {
        let root_before = request.expected_root.map(str::to_string);
        let relation = self.write_tickets_relation_remove(workspace, request)?;
        let change = MutationChange::relation_removed(
            relation.relation_id.clone(),
            relation.kind.clone(),
            relation.target_id.clone(),
        );
        Ok(relation_receipt(
            "ticket.relation_removed",
            relation,
            root_before,
            vec![change],
        ))
    }

    pub fn write_lanes_create(
        &self,
        workspace: &str,
        request: LaneCreateRequest<'_>,
    ) -> Result<Lane> {
        let build_lane = |updated_by: &str| -> Result<Lane> {
            Lane::new(LaneInput {
                lane_id: request.lane_id,
                lane_key: request.lane_key,
                title: request.title,
                description: request.description,
                lane_kind: LaneKind::parse(request.lane_kind)?,
                owner_principal: request.owner_principal,
                lane_status: LaneStatus::parse(request.lane_status)?,
                lane_tickets: request.lane_tickets,
                active_ticket_id: request.active_ticket_id,
                status_report: request.status_report,
                reviewer_feedback: request.reviewer_feedback,
                updated_at: now_ms(),
                updated_by,
            })
        };
        if let Some(backend) = self.store.remote_backend() {
            // TODO: remote host derives the actor; forward the provided override verbatim.
            return backend.lanes_create(workspace, build_lane(request.updated_by.unwrap_or(""))?);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let actor = resolve_lane_actor(loom, ns, request.updated_by)?;
            let lane = build_lane(&actor)?;
            let lane = loom_lanes::create_lane(loom, ns, lane)?;
            loom_lanes::emit_lane_change_notification(loom, ns, workspace, &lane, "lane.created")?;
            Ok(lane)
        })
    }

    pub fn write_lanes_create_receipt(
        &self,
        workspace: &str,
        request: LaneCreateRequest<'_>,
    ) -> Result<MutationEnvelope<Lane>> {
        let lane = self.write_lanes_create(workspace, request)?;
        Ok(lane_receipt(
            "lane.created",
            lane,
            vec![MutationChange::ResourceCreated],
        ))
    }

    pub fn write_lanes_update(
        &self,
        workspace: &str,
        request: LaneUpdateRequest<'_>,
    ) -> Result<Lane> {
        if request.title.is_none()
            && request.description.is_none()
            && request.lane_status.is_none()
            && request.status_report.is_none()
            && request.reviewer_feedback.is_none()
        {
            return Err(LoomError::invalid(
                "lane update requires at least one field",
            ));
        }
        if let Some(backend) = self.store.remote_backend() {
            return backend.lanes_update(
                workspace,
                crate::RemoteLaneUpdate {
                    lane_id: request.lane_id,
                    title: request.title,
                    description: request.description,
                    lane_status: request.lane_status,
                    status_report: request.status_report,
                    reviewer_feedback: request.reviewer_feedback,
                    updated_by: request.updated_by.unwrap_or(""),
                },
            );
        }
        self.write_lanes_mutation(
            workspace,
            request.lane_id,
            "lane.updated",
            |lane, loom, ns| {
                if let Some(title) = request.title {
                    lane.title = title.to_string();
                }
                if let Some(description) = request.description {
                    lane.description = description.to_string();
                }
                if let Some(lane_status) = request.lane_status {
                    lane.lane_status = LaneStatus::parse(lane_status)?.as_str().to_string();
                }
                if let Some(status_report) = request.status_report {
                    lane.status_report = status_report.to_string();
                }
                if let Some(reviewer_feedback) = request.reviewer_feedback {
                    lane.reviewer_feedback = reviewer_feedback.to_string();
                }
                let actor = resolve_lane_actor(loom, ns, request.updated_by)?;
                update_lane_metadata(lane, &actor);
                Ok(())
            },
        )
    }

    pub fn write_lanes_update_receipt(
        &self,
        workspace: &str,
        request: LaneUpdateRequest<'_>,
    ) -> Result<MutationEnvelope<Lane>> {
        let mut changes = Vec::new();
        if let Some(title) = request.title {
            changes.push(MutationChange::field_set("title", title));
        }
        if let Some(description) = request.description {
            changes.push(MutationChange::field_set("description", description));
        }
        if let Some(lane_status) = request.lane_status {
            changes.push(MutationChange::field_set("lane_status", lane_status));
        }
        if let Some(status_report) = request.status_report {
            changes.push(MutationChange::field_set("status_report", status_report));
        }
        if let Some(reviewer_feedback) = request.reviewer_feedback {
            changes.push(MutationChange::field_set(
                "reviewer_feedback",
                reviewer_feedback,
            ));
        }
        let lane = self.write_lanes_update(workspace, request)?;
        Ok(lane_receipt("lane.updated", lane, changes))
    }

    pub fn write_lanes_ticket_add(
        &self,
        workspace: &str,
        request: LaneTicketUpdateRequest<'_>,
    ) -> Result<Lane> {
        self.write_lanes_mutation(
            workspace,
            request.lane_id,
            "lane.ticket_added",
            |lane, loom, ns| {
                loom_lanes::place_lane_ticket(lane, request.ticket_id, request.placement)?;
                promote_backlog_ticket_to_ready(loom, ns, request.ticket_id)?;
                let actor = resolve_lane_actor(loom, ns, request.updated_by)?;
                update_lane_metadata(lane, &actor);
                Ok(())
            },
        )
    }

    pub fn write_lanes_ticket_add_receipt(
        &self,
        workspace: &str,
        request: LaneTicketUpdateRequest<'_>,
    ) -> Result<MutationEnvelope<Lane>> {
        let ticket_id = request.ticket_id.to_string();
        let lane = self.write_lanes_ticket_add(workspace, request)?;
        Ok(lane_receipt(
            "lane.ticket_added",
            lane,
            vec![MutationChange::field_set("ticket_id", ticket_id)],
        ))
    }

    pub fn write_lanes_ticket_remove(
        &self,
        workspace: &str,
        request: LaneTicketUpdateRequest<'_>,
    ) -> Result<Lane> {
        if let Some(backend) = self.store.remote_backend() {
            // TODO: remote host derives the actor; forward the provided override verbatim.
            return backend.lanes_ticket_remove(
                workspace,
                request.lane_id,
                request.ticket_id,
                request.updated_by.unwrap_or(""),
            );
        }
        self.write_lanes_mutation(
            workspace,
            request.lane_id,
            "lane.ticket_removed",
            |lane, loom, ns| {
                lane.lane_tickets
                    .retain(|lane_ticket| lane_ticket.ticket_id != request.ticket_id);
                if lane.active_ticket_id.as_deref() == Some(request.ticket_id) {
                    lane.active_ticket_id = None;
                }
                let actor = resolve_lane_actor(loom, ns, request.updated_by)?;
                update_lane_metadata(lane, &actor);
                Ok(())
            },
        )
    }

    pub fn write_lanes_ticket_remove_receipt(
        &self,
        workspace: &str,
        request: LaneTicketUpdateRequest<'_>,
    ) -> Result<MutationEnvelope<Lane>> {
        let ticket_id = request.ticket_id.to_string();
        let lane = self.write_lanes_ticket_remove(workspace, request)?;
        Ok(lane_receipt(
            "lane.ticket_removed",
            lane,
            vec![MutationChange::field_deleted("ticket_id", Some(ticket_id))],
        ))
    }

    pub fn write_lanes_ticket_transfer(
        &self,
        workspace: &str,
        request: LaneTicketTransferRequest<'_>,
    ) -> Result<Lane> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let (_source, target) = loom_lanes::transfer_assignment_lane_ticket(
                loom,
                ns,
                request.source_lane_id,
                request.target_lane_id,
                request.ticket_id,
                now_ms(),
                request.updated_by,
            )?;
            loom_lanes::emit_lane_change_notification(
                loom,
                ns,
                workspace,
                &target,
                "lane.ticket_transferred",
            )?;
            Ok(target)
        })
    }

    pub fn write_lanes_ticket_transfer_receipt(
        &self,
        workspace: &str,
        request: LaneTicketTransferRequest<'_>,
    ) -> Result<MutationEnvelope<Lane>> {
        let target_lane_id = request.target_lane_id.to_string();
        let ticket_id = request.ticket_id.to_string();
        let lane = self.write_lanes_ticket_transfer(workspace, request)?;
        Ok(lane_receipt(
            "lane.ticket_transferred",
            lane,
            vec![
                MutationChange::field_set("target_lane_id", target_lane_id),
                MutationChange::field_set("ticket_id", ticket_id),
            ],
        ))
    }

    pub fn write_lanes_delete(
        &self,
        workspace: &str,
        request: LaneDeleteRequest<'_>,
    ) -> Result<Lane> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lanes::delete_lane(loom, ns, request.lane_id, now_ms(), request.updated_by)
        })
    }

    pub fn write_lanes_delete_receipt(
        &self,
        workspace: &str,
        request: LaneDeleteRequest<'_>,
    ) -> Result<MutationEnvelope<Lane>> {
        let lane = self.write_lanes_delete(workspace, request)?;
        Ok(lane_receipt(
            "lane.deleted",
            lane,
            vec![MutationChange::ResourceDeleted],
        ))
    }

    fn write_lanes_mutation<F>(
        &self,
        workspace: &str,
        lane_id: &str,
        event_kind: &str,
        mutate: F,
    ) -> Result<Lane>
    where
        F: FnOnce(&mut Lane, &mut Loom<FileStore>, WorkspaceId) -> Result<()>,
    {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let mut lane = loom_lanes::get_lane(loom, ns, lane_id)?
                .ok_or_else(|| LoomError::new(Code::NotFound, "lane not found"))?;
            mutate(&mut lane, loom, ns)?;
            let lane = loom_lanes::put_lane(loom, ns, lane)?;
            loom_lanes::emit_lane_change_notification(loom, ns, workspace, &lane, event_kind)?;
            Ok(lane)
        })
    }

    pub fn write_spaces_create(
        &self,
        workspace: &str,
        workspace_id: &str,
        space_id: &str,
        title: &str,
        expected_root: Option<&str>,
    ) -> Result<SpaceSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::create_space(loom, ns, workspace_id, space_id, title, expected_root)
        })
    }

    pub fn write_pages_create(
        &self,
        workspace: &str,
        request: PageCreateRequest<'_>,
    ) -> Result<PageSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::create_page(loom, ns, request)
        })
    }

    pub fn write_pages_update(
        &self,
        workspace: &str,
        workspace_id: &str,
        page_id: &str,
        body: Vec<u8>,
        expected_root: Option<&str>,
    ) -> Result<PageUpdateSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::update_page(
                loom,
                ns,
                workspace_id,
                page_id,
                body,
                now_ms(),
                expected_root,
            )
        })
    }

    pub fn write_pages_update_text(
        &self,
        workspace: &str,
        workspace_id: &str,
        page_id: &str,
        body_text: &str,
        expected_root: Option<&str>,
    ) -> Result<PageUpdateSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::update_page_text(
                loom,
                ns,
                workspace_id,
                page_id,
                body_text,
                now_ms(),
                expected_root,
            )
        })
    }

    pub fn write_pages_publish(
        &self,
        workspace: &str,
        workspace_id: &str,
        page_id: &str,
        expected_root: Option<&str>,
    ) -> Result<PagePublishSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::publish_page(loom, ns, workspace_id, page_id, now_ms(), expected_root)
        })
    }

    pub fn write_lifecycles_define(
        &self,
        workspace: &str,
        workspace_id: &str,
        definition_cbor: &[u8],
    ) -> Result<LifecycleDefinitionSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::define_lifecycle(loom, ns, workspace_id, definition_cbor)
        })
    }

    pub fn write_lifecycles_define_standard(
        &self,
        workspace: &str,
        request: StandardLifecycleRequest<'_>,
    ) -> Result<LifecycleDefinitionSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::define_standard_lifecycle(loom, ns, request)
        })
    }

    pub fn write_lifecycles_instantiate(
        &self,
        workspace: &str,
        workspace_id: &str,
        instance_id: &str,
        definition_id: &str,
        subject_refs: Vec<String>,
    ) -> Result<LifecycleInstanceSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::instantiate(
                loom,
                ns,
                workspace_id,
                instance_id,
                definition_id,
                subject_refs,
            )
        })
    }

    pub fn write_lifecycles_transition(
        &self,
        workspace: &str,
        request: LifecycleTransitionRequest<'_>,
    ) -> Result<LifecycleTransitionResult> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::transition(loom, ns, request)
        })
    }

    pub fn write_chat_post_message(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        message_id: &str,
        thread_id: Option<&str>,
        body: Vec<u8>,
    ) -> Result<ChatWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::post_message(
                loom,
                ns,
                workspace_id,
                channel_id,
                message_id,
                thread_id,
                body,
            )
        })
    }

    pub fn write_chat_create_channel(
        &self,
        workspace: &str,
        workspace_id: &str,
        handle: &str,
        name: &str,
    ) -> Result<crate::chat::ChatChannelDirectorySummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::create_channel(loom, ns, workspace_id, handle, name)
        })
    }

    pub fn write_chat_rename_channel(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel: &str,
        handle: &str,
    ) -> Result<crate::chat::ChatChannelDirectorySummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::rename_channel(loom, ns, workspace_id, channel, handle)
        })
    }

    pub fn write_chat_edit_message(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        message_id: &str,
        body: Vec<u8>,
    ) -> Result<ChatWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::edit_message(loom, ns, workspace_id, channel_id, message_id, body)
        })
    }

    pub fn write_chat_redact_message(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        message_id: &str,
        reason: Option<&str>,
    ) -> Result<ChatWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::redact_message(loom, ns, workspace_id, channel_id, message_id, reason)
        })
    }

    pub fn write_chat_create_thread(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        thread_id: &str,
        parent_message_id: &str,
    ) -> Result<ChatWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::create_thread(
                loom,
                ns,
                workspace_id,
                channel_id,
                thread_id,
                parent_message_id,
            )
        })
    }

    pub fn write_chat_create_task(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        task_id: &str,
        message_id: Option<&str>,
        title: &str,
    ) -> Result<ChatWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::create_task(
                loom,
                ns,
                workspace_id,
                channel_id,
                task_id,
                message_id,
                title,
            )
        })
    }

    pub fn write_chat_claim_task(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        task_id: &str,
        claim_id: &str,
        lease_token: Option<&str>,
    ) -> Result<ChatWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::claim_task(
                loom,
                ns,
                workspace_id,
                channel_id,
                task_id,
                claim_id,
                lease_token,
            )
        })
    }

    pub fn write_chat_complete_task(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        task_id: &str,
        claim_id: &str,
        result_message_id: Option<&str>,
    ) -> Result<ChatWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::complete_task(
                loom,
                ns,
                workspace_id,
                channel_id,
                task_id,
                claim_id,
                result_message_id,
            )
        })
    }

    pub fn write_chat_invoke_agent(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        invocation_id: &str,
        agent_principal: &str,
        source_message_ids: Vec<String>,
        prompt: Vec<u8>,
    ) -> Result<ChatWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let agent_principal = parse_workspace_id(agent_principal)?;
            crate::chat::invoke_agent(
                loom,
                ns,
                workspace_id,
                channel_id,
                invocation_id,
                agent_principal,
                source_message_ids,
                prompt,
            )
        })
    }

    pub fn write_chat_agent_reply(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        invocation_id: &str,
        message_id: &str,
    ) -> Result<ChatWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::agent_reply(
                loom,
                ns,
                workspace_id,
                channel_id,
                invocation_id,
                message_id,
            )
        })
    }

    pub fn write_chat_request_handoff(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        handoff_id: &str,
        from_agent_principal: &str,
        to_principal: Option<&str>,
        reason: Option<&str>,
    ) -> Result<ChatWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let from_agent_principal = parse_workspace_id(from_agent_principal)?;
            let to_principal = to_principal.map(parse_workspace_id).transpose()?;
            crate::chat::request_handoff(
                loom,
                ns,
                workspace_id,
                channel_id,
                handoff_id,
                from_agent_principal,
                to_principal,
                reason,
            )
        })
    }

    pub fn write_chat_update_cursor(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        next_sequence: u64,
    ) -> Result<crate::chat::ChatCursorSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::update_cursor(loom, ns, workspace_id, channel_id, next_sequence)
        })
    }

    pub fn write_chat_add_reaction(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        message_id: &str,
        kind: &str,
    ) -> Result<ChatWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::add_reaction(loom, ns, workspace_id, channel_id, message_id, kind)
        })
    }

    pub fn write_chat_remove_reaction(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        message_id: &str,
        kind: &str,
    ) -> Result<ChatWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::remove_reaction(loom, ns, workspace_id, channel_id, message_id, kind)
        })
    }

    pub fn read_chat_emoji_registry(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<crate::chat::ChatEmojiRegistrySummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::emoji_registry(loom, ns, workspace_id)
        })
    }

    pub fn write_chat_emoji_register(
        &self,
        workspace: &str,
        workspace_id: &str,
        kind: &str,
    ) -> Result<crate::chat::ChatEmojiRegistrySummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::register_emoji(loom, ns, workspace_id, kind)
        })
    }

    pub fn write_chat_emoji_unregister(
        &self,
        workspace: &str,
        workspace_id: &str,
        kind: &str,
    ) -> Result<crate::chat::ChatEmojiRegistrySummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::unregister_emoji(loom, ns, workspace_id, kind)
        })
    }

    pub fn write_drive_create_folder(
        &self,
        workspace: &str,
        workspace_id: &str,
        parent_folder_id: &str,
        folder_id: &str,
        name: &str,
        expected_root: &str,
        write_admission: Option<&WriteAdmission>,
    ) -> Result<DriveWriteSummary> {
        self.enforce_drive_write_admission(workspace, workspace_id, write_admission)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::create_folder(
                loom,
                ns,
                workspace_id,
                parent_folder_id,
                folder_id,
                name,
                expected_root,
            )
        })
    }

    pub fn write_drive_create_upload(
        &self,
        workspace: &str,
        request: DriveCreateUploadRequest<'_>,
        write_admission: Option<&WriteAdmission>,
    ) -> Result<DriveUploadSessionSummary> {
        self.enforce_drive_write_admission(workspace, request.workspace_id, write_admission)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::create_upload(loom, ns, request)
        })
    }

    pub fn write_drive_upload_chunk(
        &self,
        workspace: &str,
        workspace_id: &str,
        upload_id: &str,
        bytes: &[u8],
        write_admission: Option<&WriteAdmission>,
    ) -> Result<DriveUploadSessionSummary> {
        self.enforce_drive_write_admission(workspace, workspace_id, write_admission)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::upload_chunk(loom, ns, workspace_id, upload_id, bytes)
        })
    }

    pub fn write_drive_commit_upload(
        &self,
        workspace: &str,
        workspace_id: &str,
        upload_id: &str,
        write_admission: Option<&WriteAdmission>,
    ) -> Result<DriveWriteSummary> {
        self.enforce_drive_write_admission(workspace, workspace_id, write_admission)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::commit_upload(loom, ns, workspace_id, upload_id)
        })
    }

    pub fn write_drive_rename(
        &self,
        workspace: &str,
        workspace_id: &str,
        folder_id: &str,
        node_id: &str,
        new_name: &str,
        expected_root: &str,
        write_admission: Option<&WriteAdmission>,
    ) -> Result<DriveWriteSummary> {
        self.enforce_drive_write_admission(workspace, workspace_id, write_admission)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::rename_node(
                loom,
                ns,
                workspace_id,
                folder_id,
                node_id,
                new_name,
                expected_root,
            )
        })
    }

    pub fn write_drive_move(
        &self,
        workspace: &str,
        workspace_id: &str,
        source_folder_id: &str,
        target_folder_id: &str,
        node_id: &str,
        expected_root: &str,
        write_admission: Option<&WriteAdmission>,
    ) -> Result<DriveWriteSummary> {
        self.enforce_drive_write_admission(workspace, workspace_id, write_admission)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::move_node(
                loom,
                ns,
                workspace_id,
                source_folder_id,
                target_folder_id,
                node_id,
                expected_root,
            )
        })
    }

    pub fn write_drive_delete(
        &self,
        workspace: &str,
        workspace_id: &str,
        folder_id: &str,
        node_id: &str,
        expected_root: &str,
        write_admission: Option<&WriteAdmission>,
    ) -> Result<DriveWriteSummary> {
        self.enforce_drive_write_admission(workspace, workspace_id, write_admission)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::delete_node(loom, ns, workspace_id, folder_id, node_id, expected_root)
        })
    }

    pub fn write_drive_resolve_conflict(
        &self,
        workspace: &str,
        workspace_id: &str,
        conflict_id: &str,
        resolution: DriveConflictResolutionRequest,
        write_admission: Option<&WriteAdmission>,
    ) -> Result<DriveWriteSummary> {
        self.enforce_drive_write_admission(workspace, workspace_id, write_admission)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::resolve_conflict(loom, ns, workspace_id, conflict_id, resolution)
        })
    }

    pub fn write_drive_grant_share(
        &self,
        workspace: &str,
        request: DriveGrantShareRequest<'_>,
    ) -> Result<DriveWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::grant_share(loom, ns, request)
        })
    }

    pub fn write_drive_revoke_share(
        &self,
        workspace: &str,
        workspace_id: &str,
        grant_id: &str,
    ) -> Result<DriveWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::revoke_share(loom, ns, workspace_id, grant_id)
        })
    }

    pub fn write_drive_apply_share_expiry(
        &self,
        workspace: &str,
        workspace_id: &str,
        now_ms: u64,
    ) -> Result<crate::drive::DriveShareExpiryApplySummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::apply_share_expiry(loom, ns, workspace_id, now_ms)
        })
    }

    pub fn write_drive_pin_retention(
        &self,
        workspace: &str,
        request: DrivePinRetentionRequest<'_>,
    ) -> Result<DriveWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::pin_retention(loom, ns, request)
        })
    }

    pub fn write_drive_unpin_retention(
        &self,
        workspace: &str,
        workspace_id: &str,
        pin_id: &str,
    ) -> Result<DriveWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::unpin_retention(loom, ns, workspace_id, pin_id)
        })
    }

    pub fn write_drive_apply_retention(
        &self,
        workspace: &str,
        workspace_id: &str,
        now_ms: u64,
    ) -> Result<crate::drive::DriveRetentionApplySummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::apply_retention(loom, ns, workspace_id, now_ms)
        })
    }

    pub fn write_drive_acquire_lease(
        &self,
        workspace: &str,
        workspace_id: &str,
        target_kind: &str,
        target_id: &str,
        lease_ms: u64,
        wait_ms: u64,
    ) -> Result<DriveLeaseTokenSummary> {
        let (key, principal) = self.drive_lease_key_and_principal(
            workspace,
            workspace_id,
            target_kind,
            target_id,
            AclRight::Write,
        )?;
        let (paths, session, auth) = self.drive_daemon_session_parts()?;
        let response = daemon::lock_acquire_auth(
            paths,
            daemon::AcquireRequest {
                key: &key,
                principal: &principal,
                session,
                mode: loom_core::LockMode::Exclusive,
                lease_ms,
                wait_ms,
                now_ms: now_ms(),
            },
            auth,
        )?;
        let token = drive_lease_token_from_response(&response)?;
        if let Err(err) = self.record_drive_lease_operation(
            workspace,
            workspace_id,
            "lock.acquired",
            target_kind,
            target_id,
        ) {
            let _ = daemon::lock_release_auth(
                paths,
                daemon::ReleaseRequest {
                    key: &key,
                    principal: &principal,
                    session,
                    mode: loom_core::LockMode::Exclusive,
                    fence: loom_core::Fence::from(token.fence),
                    now_ms: now_ms(),
                },
                auth,
            );
            return Err(err);
        }
        Ok(token)
    }

    pub fn write_drive_refresh_lease(
        &self,
        workspace: &str,
        workspace_id: &str,
        target_kind: &str,
        target_id: &str,
        fence: loom_core::Fence,
        lease_ms: u64,
    ) -> Result<DriveLeaseTokenSummary> {
        let (key, principal) = self.drive_lease_key_and_principal(
            workspace,
            workspace_id,
            target_kind,
            target_id,
            AclRight::Write,
        )?;
        let (paths, session, auth) = self.drive_daemon_session_parts()?;
        let response = match daemon::lock_refresh_auth(
            paths,
            daemon::RefreshRequest {
                key: &key,
                principal: &principal,
                session,
                mode: loom_core::LockMode::Exclusive,
                fence,
                lease_ms,
                now_ms: now_ms(),
            },
            auth,
        ) {
            Ok(response) => response,
            Err(err) => {
                if err.code == Code::LockLeaseExpired {
                    self.record_drive_lease_operation(
                        workspace,
                        workspace_id,
                        "lock.expired",
                        target_kind,
                        target_id,
                    )?;
                }
                return Err(err);
            }
        };
        let token = drive_lease_token_from_response(&response)?;
        self.record_drive_lease_operation(
            workspace,
            workspace_id,
            "lock.refreshed",
            target_kind,
            target_id,
        )?;
        Ok(token)
    }

    pub fn write_drive_release_lease(
        &self,
        workspace: &str,
        workspace_id: &str,
        target_kind: &str,
        target_id: &str,
        fence: loom_core::Fence,
    ) -> Result<bool> {
        let (key, principal) = self.drive_lease_key_and_principal(
            workspace,
            workspace_id,
            target_kind,
            target_id,
            AclRight::Write,
        )?;
        let (paths, session, auth) = self.drive_daemon_session_parts()?;
        if let Err(err) = daemon::lock_release_auth(
            paths,
            daemon::ReleaseRequest {
                key: &key,
                principal: &principal,
                session,
                mode: loom_core::LockMode::Exclusive,
                fence,
                now_ms: now_ms(),
            },
            auth,
        ) {
            if err.code == Code::LockLeaseExpired {
                self.record_drive_lease_operation(
                    workspace,
                    workspace_id,
                    "lock.expired",
                    target_kind,
                    target_id,
                )?;
            }
            return Err(err);
        }
        self.record_drive_lease_operation(
            workspace,
            workspace_id,
            "lock.released",
            target_kind,
            target_id,
        )?;
        Ok(true)
    }

    pub fn write_drive_break_lease(
        &self,
        workspace: &str,
        workspace_id: &str,
        target_kind: &str,
        target_id: &str,
    ) -> Result<DriveLeaseBreakSummary> {
        let (key, _) = self.drive_lease_key_and_principal(
            workspace,
            workspace_id,
            target_kind,
            target_id,
            AclRight::Admin,
        )?;
        let (paths, _, auth) = self.drive_daemon_session_parts()?;
        let response = daemon::lock_break_auth(
            paths,
            daemon::BreakRequest {
                key: &key,
                now_ms: now_ms(),
            },
            auth,
        )?;
        let broken_holders = drive_lease_break_count_from_response(&response)?;
        self.record_drive_lease_operation(
            workspace,
            workspace_id,
            "lock.broken",
            target_kind,
            target_id,
        )?;
        Ok(DriveLeaseBreakSummary {
            key,
            broken_holders,
        })
    }

    fn record_drive_lease_operation(
        &self,
        workspace: &str,
        workspace_id: &str,
        operation_kind: &str,
        target_kind: &str,
        target_id: &str,
    ) -> Result<DriveWriteSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::record_lease_operation(
                loom,
                ns,
                workspace_id,
                operation_kind,
                target_kind,
                target_id,
            )
        })
    }

    fn drive_lease_key_and_principal(
        &self,
        workspace: &str,
        workspace_id: &str,
        target_kind: &str,
        target_id: &str,
        right: AclRight,
    ) -> Result<(String, String)> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_domain(ns, AclDomain::Files, right)?;
            let principal = loom.effective_principal()?.unwrap_or(ns).to_string();
            Ok((
                crate::drive::drive_lease_key(ns, workspace_id, target_kind, target_id)?,
                principal,
            ))
        })
    }

    fn drive_daemon_session_parts(
        &self,
    ) -> Result<(&daemon::DaemonPaths, &str, &daemon::DaemonAuth)> {
        self.store.daemon_session_parts()?.ok_or_else(|| {
            LoomError::invalid("drive lease operations require an attached daemon session")
        })
    }

    fn enforce_drive_write_admission(
        &self,
        workspace: &str,
        workspace_id: &str,
        admission: Option<&WriteAdmission>,
    ) -> Result<()> {
        let mandatory = self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let key = write_admission_policy_key(ns, "drive", workspace_id)?;
            let Some(bytes) = loom.store().control_get(&key)? else {
                return Ok(false);
            };
            let policy = WriteAdmissionPolicy::decode(&bytes)?;
            Ok(match admission {
                Some(admission) => {
                    policy.mode_for(&admission.target_kind, &admission.target_id)
                        == WriteAdmissionMode::Mandatory
                }
                None => {
                    policy.default_mode == WriteAdmissionMode::Mandatory
                        || !policy.mandatory_targets.is_empty()
                }
            })
        })?;
        if mandatory && admission.is_none() {
            return Err(LoomError::new(
                Code::Locked,
                "write admission is mandatory for this surface",
            ));
        }
        let Some(admission) = admission else {
            return Ok(());
        };
        let (key, principal) = self.drive_lease_key_and_principal(
            workspace,
            workspace_id,
            &admission.target_kind,
            &admission.target_id,
            AclRight::Write,
        )?;
        let (paths, session, auth) = self.drive_daemon_session_parts()?;
        let response = daemon::lock_apply_fence_auth(
            paths,
            daemon::ApplyFenceRequest {
                key: &key,
                principal: &principal,
                session,
                mode: loom_core::LockMode::Exclusive,
                fence: admission.fence,
                now_ms: now_ms(),
            },
            auth,
        )?;
        if response.trim_end() == "applied" {
            Ok(())
        } else {
            Err(LoomError::invalid(format!(
                "unexpected drive write admission response {response:?}"
            )))
        }
    }

    pub fn read_drive_list_conflicts(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<Vec<DriveConflictSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::list_conflicts(loom, ns, workspace_id)
        })
    }

    pub fn write_structures_create(
        &self,
        workspace: &str,
        request: StructureCreateRequest<'_>,
    ) -> Result<StructureRenderSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::create_structure(loom, ns, request)
        })
    }

    pub fn write_structures_add_node(
        &self,
        workspace: &str,
        request: StructureNodeRequest<'_>,
    ) -> Result<StructureNodeSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::add_structure_node(loom, ns, request)
        })
    }

    pub fn write_structures_update_node(
        &self,
        workspace: &str,
        request: StructureNodeRequest<'_>,
    ) -> Result<StructureNodeSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::update_structure_node(loom, ns, request)
        })
    }

    pub fn write_structures_bind(
        &self,
        workspace: &str,
        request: StructureBindRequest<'_>,
    ) -> Result<StructureNodeSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::bind_structure_node(loom, ns, request)
        })
    }

    pub fn write_structures_move_node(
        &self,
        workspace: &str,
        request: StructureMoveRequest<'_>,
    ) -> Result<StructureMoveSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::move_structure_node(loom, ns, request)
        })
    }

    pub fn write_structures_link_node(
        &self,
        workspace: &str,
        request: StructureLinkRequest<'_>,
    ) -> Result<StructureEdgeSummary> {
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::link_structure_node(loom, ns, request)
        })
    }

    pub fn write_structures_decompose_to_tickets(
        &self,
        workspace: &str,
        request: StructureDecomposeRequest<'_>,
    ) -> Result<StructureDecomposeSummary> {
        self.store.write(|loom| {
            let snapshot = loom.export_state();
            let ns = resolve_ns(loom, workspace)?;
            match crate::pages::decompose_to_tickets(loom, ns, request) {
                Ok(summary) => Ok(summary),
                Err(error) => {
                    if let Err(rollback_error) = loom.import_state(&snapshot) {
                        return Err(LoomError::new(
                            Code::Internal,
                            format!(
                                "structure decomposition rollback failed after operation error {error}: {rollback_error}"
                            ),
                        ));
                    }
                    Err(error)
                }
            }
        })
    }

    pub fn write_substrate_transact(
        &self,
        ops: Vec<SubstrateTransactOp>,
    ) -> Result<SubstrateTransactResult> {
        self.store.write(|loom| {
            let snapshot = loom.export_state();
            let mut results = Vec::with_capacity(ops.len());
            for op in ops {
                match apply_substrate_transact_op(loom, op) {
                    Ok(result) => results.push(result),
                    Err(error) => {
                        if let Err(rollback_error) = loom.import_state(&snapshot) {
                            return Err(LoomError::new(
                                Code::Internal,
                                format!(
                                    "transaction rollback failed after operation error {error}: {rollback_error}"
                                ),
                            ));
                        }
                        return Err(error);
                    }
                }
            }
            Ok(SubstrateTransactResult {
                applied: results.len() as u64,
                results,
            })
        })
    }

    // ---- telemetry ----

    pub fn write_metrics_put_descriptor(&self, workspace: &str, descriptor: &[u8]) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.metrics_put_descriptor(workspace, descriptor);
        }
        let descriptor = MetricDescriptor::decode(descriptor)?;
        self.store.write(|loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &WsSelector::Typed {
                    ty: FacetKind::Metrics,
                    name: workspace.to_string(),
                },
                fresh_workspace_id()?,
            )?;
            metrics_put_descriptor(loom, ns, &descriptor)
        })
    }

    pub fn write_metrics_put_observation(
        &self,
        workspace: &str,
        descriptor_name: &str,
        observation: &[u8],
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.metrics_put_observation(workspace, descriptor_name, observation);
        }
        let observation = MetricObservation::decode(observation)?;
        self.store.write(|loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &WsSelector::Typed {
                    ty: FacetKind::Metrics,
                    name: workspace.to_string(),
                },
                fresh_workspace_id()?,
            )?;
            metrics_put_observation(loom, ns, descriptor_name, &observation)
        })
    }

    pub fn write_logs_put_record(&self, workspace: &str, record: &[u8]) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.logs_put_record(workspace, record);
        }
        let record = LogRecord::decode(record)?;
        self.store.write(|loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &WsSelector::Typed {
                    ty: FacetKind::Logs,
                    name: workspace.to_string(),
                },
                fresh_workspace_id()?,
            )?;
            logs_put_record(loom, ns, &record)
        })
    }

    pub fn write_traces_put_span(&self, workspace: &str, span: &[u8]) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.traces_put_span(workspace, span);
        }
        let span = SpanRecord::decode(span)?;
        self.store.write(|loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &WsSelector::Typed {
                    ty: FacetKind::Traces,
                    name: workspace.to_string(),
                },
                fresh_workspace_id()?,
            )?;
            traces_put_span(loom, ns, &span)
        })
    }

    // ---- timeseries ----

    /// `timeseries.put`: record `value` at `ts` in series `name`.
    pub fn write_timeseries_put(
        &self,
        workspace: &str,
        name: &str,
        ts: i64,
        value: Vec<u8>,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.ts_put(workspace, name, ts, value);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            ts_put(loom, ns, name, ts, value)
        })
    }

    // ---- ledger ----

    /// `ledger.append`: append `payload` to ledger `name`; returns the new entry's sequence.
    pub fn write_ledger_append(
        &self,
        workspace: &str,
        name: &str,
        payload: Vec<u8>,
    ) -> Result<u64> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.ledger_append(workspace, name, payload);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            ledger_append(loom, ns, name, payload)
        })
    }

    // ---- queue ----

    /// `queue.append`: append `entry` to `stream`; returns the assigned sequence.
    pub fn write_queue_append(&self, workspace: &str, stream: &str, entry: &[u8]) -> Result<u64> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.queue_append(workspace, stream, entry);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(loom.stream_append(ns, stream, entry)? as u64)
        })
    }

    /// `queue.consumer_advance`: advance the named consumer's next sequence (monotonic).
    pub fn write_queue_consumer_advance(
        &self,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
        next_seq: u64,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.queue_consumer_advance(workspace, stream, consumer_id, next_seq);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.consumer_advance(ns, stream, consumer_id, next_seq)
        })
    }

    /// `queue.consumer_reset`: set the named consumer's next sequence (may move backward).
    pub fn write_queue_consumer_reset(
        &self,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
        next_seq: u64,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.queue_consumer_reset(workspace, stream, consumer_id, next_seq);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.consumer_reset(ns, stream, consumer_id, next_seq)
        })
    }

    // ---- fs ----

    /// `fs.write_file`: write `content` to `path` with `mode`.
    pub fn write_fs_write_file(
        &self,
        workspace: &str,
        path: &str,
        content: &[u8],
        mode: u32,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_write_file(workspace, path, content, mode);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.write_file(ns, path, content, mode)
        })
    }

    /// `fs.append_file`: append `content` to `path`.
    pub fn write_fs_append_file(&self, workspace: &str, path: &str, content: &[u8]) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_append_file(workspace, path, content);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.append_file(ns, path, content)
        })
    }

    /// `fs.remove_file`: remove `path`.
    pub fn write_fs_remove_file(&self, workspace: &str, path: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_remove_file(workspace, path);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.remove_file(ns, path)
        })
    }

    pub fn write_fs_create_directory(
        &self,
        workspace: &str,
        path: &str,
        recursive: bool,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_create_directory(workspace, path, recursive);
        }
        self.store.write(|loom| {
            let ns = loom.registry_mut().ensure_for_write(
                &WsSelector::Typed {
                    ty: FacetKind::Files,
                    name: workspace.to_string(),
                },
                fresh_workspace_id()?,
            )?;
            loom.create_directory(ns, path, recursive)
        })
    }

    pub fn write_fs_remove_directory(
        &self,
        workspace: &str,
        path: &str,
        recursive: bool,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_remove_directory(workspace, path, recursive);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.remove_directory(ns, path, recursive)
        })
    }

    pub fn write_mcp_app_create(
        &self,
        workspace: &str,
        app: &str,
        index_html: &[u8],
        meta_md: &[u8],
    ) -> Result<()> {
        apps::validate_app_name(app)?;
        let meta_text = std::str::from_utf8(meta_md)
            .map_err(|e| LoomError::invalid(format!("app _meta.md must be UTF-8: {e}")))?;
        apps::parse_meta(app, meta_text)?;
        std::str::from_utf8(index_html)
            .map_err(|e| LoomError::invalid(format!("app index.html must be UTF-8: {e}")))?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.create_directory_reserved(ns, &apps::app_dir(app), true)?;
            loom.write_file_reserved(ns, &apps::index_path(app), index_html, 0o100644)?;
            loom.write_file_reserved(ns, &apps::meta_path(app), meta_md, 0o100644)
        })
    }

    pub fn write_mcp_app_write_file(
        &self,
        workspace: &str,
        app: &str,
        path: &str,
        content: &[u8],
        mode: u32,
    ) -> Result<()> {
        let file_path = apps::app_file_path(app, path)?;
        let parent = apps::app_parent_dir(app, path)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.create_directory_reserved(ns, &parent, true)?;
            loom.write_file_reserved(ns, &file_path, content, mode)
        })
    }

    pub fn write_mcp_app_remove_file(&self, workspace: &str, app: &str, path: &str) -> Result<()> {
        let file_path = apps::app_file_path(app, path)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.remove_file_reserved(ns, &file_path)
        })
    }

    /// `fs.write_at`: write `data` into `path` at `offset`.
    pub fn write_fs_write_at(
        &self,
        workspace: &str,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_write_at(workspace, path, offset, data);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.write_at(ns, path, offset, data)
        })
    }

    /// `fs.symlink`: create a symlink at `link_path` pointing at `target`.
    pub fn write_fs_symlink(&self, workspace: &str, target: &str, link_path: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_symlink(workspace, target, link_path);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.symlink(ns, target, link_path)
        })
    }

    // ---- vcs (workspace-level, cross-facet) ----

    /// `vcs.commit`: record the working tree on the current branch; returns the commit address.
    pub fn write_vcs_commit(
        &self,
        workspace: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_commit(workspace, author, message, timestamp_ms);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(loom.commit(ns, author, message, timestamp_ms)?.to_string())
        })
    }

    /// `vcs.branch`: create branch `name` at the current tip.
    pub fn write_vcs_branch(&self, workspace: &str, name: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_branch(workspace, name);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.branch(ns, name)
        })
    }

    /// `vcs.stage`: move `path`'s working-tree change into the shared index.
    pub fn write_vcs_stage(&self, workspace: &str, path: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_stage(workspace, path);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.stage(ns, &[path])
        })
    }

    /// `vcs.stage_all`: stage every working-tree change.
    pub fn write_vcs_stage_all(&self, workspace: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_stage_all(workspace);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.stage_all(ns)
        })
    }

    /// `vcs.unstage`: revert `path`'s index entry to HEAD.
    pub fn write_vcs_unstage(&self, workspace: &str, path: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_unstage(workspace, path);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.unstage(ns, &[path])
        })
    }

    /// `vcs.commit_staged`: record only the staged index; returns the commit address.
    pub fn write_vcs_commit_staged(
        &self,
        workspace: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_commit_staged(workspace, author, message, timestamp_ms);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(loom
                .commit_staged(ns, author, message, timestamp_ms)?
                .to_string())
        })
    }

    /// `vcs.tag_create`: create a tag at `rev` (empty `message` = lightweight); returns the ref target.
    #[allow(clippy::too_many_arguments)]
    pub fn write_vcs_tag_create(
        &self,
        workspace: &str,
        name: &str,
        rev: &str,
        tagger: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_tag_create(workspace, name, rev, tagger, message, timestamp_ms);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(loom
                .tag_create(ns, name, rev, tagger, message, timestamp_ms)?
                .to_string())
        })
    }

    /// `vcs.tag_delete`: delete tag `name`.
    pub fn write_vcs_tag_delete(&self, workspace: &str, name: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_tag_delete(workspace, name);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.tag_delete(ns, name)
        })
    }

    /// `vcs.tag_rename`: rename tag `old_name` to `new_name`.
    pub fn write_vcs_tag_rename(
        &self,
        workspace: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_tag_rename(workspace, old_name, new_name);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.tag_rename(ns, old_name, new_name)
        })
    }

    /// `vcs.restore_file`: reset `path` in the working tree to the snapshot `rev` resolves to.
    pub fn write_vcs_restore_file(&self, workspace: &str, rev: &str, path: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_restore_file(workspace, rev, path);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.restore_file(ns, rev, path)
        })
    }

    /// `vcs.restore_path`: reset the subtree under `prefix` to `rev` (empty prefix = whole tree).
    pub fn write_vcs_restore_path(&self, workspace: &str, rev: &str, prefix: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_restore_path(workspace, rev, prefix);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.restore_path(ns, rev, prefix)
        })
    }

    // ---- workspace ----

    /// Rename the workspace selected by id or name.
    pub fn write_workspace_rename(&self, workspace: &str, new_name: &str) -> Result<()> {
        self.store.write(|loom| {
            loom.authorize_global_admin()?;
            let ns = resolve_ns(loom, workspace)?;
            loom.registry_mut().rename(ns, new_name)
        })
    }

    /// Delete the workspace selected by id or name.
    pub fn write_workspace_delete(&self, workspace: &str) -> Result<()> {
        self.store.write(|loom| {
            loom.authorize_global_admin()?;
            let ns = resolve_ns(loom, workspace)?;
            loom.registry_mut().delete(ns)
        })
    }

    /// Create a workspace with the given `facet` and optional `name`;
    /// returns the new workspace id.
    pub fn write_workspace_create(&self, name: Option<&str>, facet: &str) -> Result<String> {
        let ty = FacetKind::parse(facet)?;
        if let Some(backend) = self.store.remote_backend() {
            return backend.workspace_create(name, Some(ty));
        }
        self.store.write(|loom| {
            loom.authorize_global_admin()?;
            let id = fresh_workspace_id()?;
            Ok(loom.registry_mut().create(ty, name, id)?.to_string())
        })
    }

    // ---- fs ----

    /// `fs.truncate`: resize `path` to `size` bytes.
    pub fn write_fs_truncate(&self, workspace: &str, path: &str, size: u64) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_truncate(workspace, path, size);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.truncate_file(ns, path, size)
        })
    }

    // ---- vcs: branch movement, merge, replay ----

    /// `vcs.checkout`: switch the workspace to `branch`.
    pub fn write_vcs_checkout(&self, workspace: &str, branch: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_checkout(workspace, branch);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.checkout_branch(ns, branch)
        })
    }

    /// `vcs.merge`: reconcile `from_branch` into the current branch; returns the outcome
    /// (up-to-date / fast-forward / merged / conflicts).
    pub fn write_vcs_merge(
        &self,
        workspace: &str,
        from_branch: &str,
        author: &str,
        timestamp_ms: u64,
    ) -> Result<MergeOutcome> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_merge(workspace, from_branch, author, false, timestamp_ms);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.merge(ns, from_branch, author, timestamp_ms)
        })
    }

    /// `vcs.merge_resolve`: settle one conflicted `path` with `resolution` (`ours`/`theirs`/`working`).
    pub fn write_vcs_merge_resolve(
        &self,
        workspace: &str,
        path: &str,
        resolution: &str,
    ) -> Result<()> {
        let resolution = parse_resolution(resolution)?;
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_merge_resolve(workspace, path, &[resolution.stable_tag()]);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.merge_resolve(ns, path, resolution)
        })
    }

    /// `vcs.merge_abort`: discard an in-progress merge and restore the pre-merge working tree.
    pub fn write_vcs_merge_abort(&self, workspace: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_merge_abort(workspace);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.merge_abort(ns)
        })
    }

    /// `vcs.merge_continue`: record the two-parent merge commit once every path is resolved.
    pub fn write_vcs_merge_continue(
        &self,
        workspace: &str,
        author: &str,
        timestamp_ms: u64,
    ) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_merge_continue(workspace, author, timestamp_ms);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(loom.merge_continue(ns, author, timestamp_ms)?.to_string())
        })
    }

    /// `vcs.cherry_pick`: replay `commits` onto the current branch; `dry_run` previews only.
    pub fn write_vcs_cherry_pick(
        &self,
        workspace: &str,
        commits: &[String],
        timestamp_ms: u64,
        dry_run: bool,
    ) -> Result<ReplayOutcome> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_cherry_pick(workspace, commits, dry_run, timestamp_ms);
        }
        let commits = parse_digests(commits)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.cherry_pick(ns, &commits, timestamp_ms, dry_run)
        })
    }

    /// `vcs.revert`: apply the inverse of `commits` as new commits; `dry_run` previews only.
    pub fn write_vcs_revert(
        &self,
        workspace: &str,
        commits: &[String],
        author: &str,
        timestamp_ms: u64,
        dry_run: bool,
    ) -> Result<ReplayOutcome> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_revert(workspace, commits, author, dry_run, timestamp_ms);
        }
        let commits = parse_digests(commits)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.revert(ns, &commits, author, timestamp_ms, dry_run)
        })
    }

    /// `vcs.rebase`: replay the current branch onto `onto`; `dry_run` previews only.
    pub fn write_vcs_rebase(
        &self,
        workspace: &str,
        onto: &str,
        timestamp_ms: u64,
        dry_run: bool,
    ) -> Result<ReplayOutcome> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_rebase(workspace, onto, dry_run, timestamp_ms);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.rebase(ns, onto, timestamp_ms, dry_run)
        })
    }

    /// `vcs.squash`: collapse the commits after `onto` up to the tip into one; returns the new commit.
    pub fn write_vcs_squash(
        &self,
        workspace: &str,
        onto: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_squash(workspace, onto, author, message, timestamp_ms);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(loom
                .squash(ns, onto, author, message, timestamp_ms)?
                .to_string())
        })
    }

    // ---- calendar ----

    /// `calendar.create_collection`: create a collection with `display_name` and a comma-separated
    /// `components` set (`event`/`todo`).
    pub fn write_calendar_create_collection(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        display_name: &str,
        components: &str,
    ) -> Result<()> {
        let meta = CollectionMeta {
            display_name: display_name.to_string(),
            component_set: parse_component_set(components)?,
        };
        if let Some(backend) = self.store.remote_backend() {
            return backend.calendar_create_collection(
                workspace,
                principal,
                collection,
                &meta.encode(),
            );
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            calendar::create_collection(loom, ns, principal, collection, &meta)
        })
    }

    /// `calendar.delete_collection`: remove `collection`; returns whether it was present.
    pub fn write_calendar_delete_collection(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.calendar_delete_collection(workspace, principal, collection);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            calendar::delete_collection(loom, ns, principal, collection)
        })
    }

    /// `calendar.put_entry`: upsert a calendar entry (canonical-CBOR `CalendarEntry`); returns its
    /// content address.
    pub fn write_calendar_put_entry(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        entry_cbor: &[u8],
    ) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.calendar_put_entry(workspace, principal, collection, entry_cbor);
        }
        let entry = CalendarEntry::decode(entry_cbor)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(calendar::put_entry(loom, ns, principal, collection, &entry)?.to_string())
        })
    }

    pub fn write_calendar_put_ics(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        ics: &str,
    ) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.calendar_put_ics(workspace, principal, collection, ics);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(calendar::put_ics(loom, ns, principal, collection, ics)?.to_string())
        })
    }

    /// `calendar.delete_entry`: remove entry `uid`; returns whether it was present.
    pub fn write_calendar_delete_entry(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.calendar_delete_entry(workspace, principal, collection, uid);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            calendar::delete_entry(loom, ns, principal, collection, uid)
        })
    }

    // ---- contacts ----

    /// `contacts.create_book`: create an address book with `display_name`.
    pub fn write_contacts_create_book(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        display_name: &str,
    ) -> Result<()> {
        let meta = BookMeta {
            display_name: display_name.to_string(),
        };
        if let Some(backend) = self.store.remote_backend() {
            return backend.contacts_create_book(workspace, principal, book, &meta.encode());
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            contacts::create_book(loom, ns, principal, book, &meta)
        })
    }

    /// `contacts.delete_book`: remove `book`; returns whether it was present.
    pub fn write_contacts_delete_book(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.contacts_delete_book(workspace, principal, book);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            contacts::delete_book(loom, ns, principal, book)
        })
    }

    /// `contacts.put_entry`: upsert a contact (canonical-CBOR `ContactEntry`); returns its address.
    pub fn write_contacts_put_entry(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        entry_cbor: &[u8],
    ) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.contacts_put_entry(workspace, principal, book, entry_cbor);
        }
        let entry = ContactEntry::decode(entry_cbor)?;
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(contacts::put_entry(loom, ns, principal, book, &entry)?.to_string())
        })
    }

    pub fn write_contacts_put_vcard(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        vcard: &str,
    ) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.contacts_put_vcard(workspace, principal, book, vcard);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(contacts::put_vcard(loom, ns, principal, book, vcard)?.to_string())
        })
    }

    /// `contacts.delete_entry`: remove contact `uid`; returns whether it was present.
    pub fn write_contacts_delete_entry(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.contacts_delete_entry(workspace, principal, book, uid);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            contacts::delete_entry(loom, ns, principal, book, uid)
        })
    }

    // ---- mail ----

    /// `mail.create_mailbox`: create a mailbox with `display_name`.
    pub fn write_mail_create_mailbox(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        display_name: &str,
    ) -> Result<()> {
        let meta = MailboxMeta {
            display_name: display_name.to_string(),
        };
        if let Some(backend) = self.store.remote_backend() {
            return backend.mail_create_mailbox(workspace, principal, mailbox, &meta.encode());
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            mail::create_mailbox(loom, ns, principal, mailbox, &meta)
        })
    }

    /// `mail.delete_mailbox`: remove `mailbox`; returns whether it was present.
    pub fn write_mail_delete_mailbox(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.mail_delete_mailbox(workspace, principal, mailbox);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            mail::delete_mailbox(loom, ns, principal, mailbox)
        })
    }

    /// `mail.ingest_message`: store the raw RFC 5322 `raw` under `uid`; returns the body address.
    pub fn write_mail_ingest_message(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
        raw: &[u8],
    ) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.mail_ingest_message(workspace, principal, mailbox, uid, raw);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(mail::ingest_message(loom, ns, principal, mailbox, uid, raw)?.to_string())
        })
    }

    /// `mail.delete_message`: remove message `uid`; returns whether it was present.
    pub fn write_mail_delete_message(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.mail_delete_message(workspace, principal, mailbox, uid);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            mail::delete_message(loom, ns, principal, mailbox, uid)
        })
    }

    /// `mail.set_flags`: set the flag set on message `uid` (sorted, deduplicated by the core).
    pub fn write_mail_set_flags(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
        flags: &[String],
    ) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.mail_set_flags(workspace, principal, mailbox, uid, flags);
        }
        self.store.write(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            mail::set_flags(loom, ns, principal, mailbox, uid, flags)
        })
    }

    // ---- sql (session ops; per-request store access) ----

    /// `sql.exec`: run one or more `;`-separated statements against the SQL-facet workspace `workspace`
    /// database `db`; returns the result payloads as Loom Canonical CBOR. Mirrors the C ABI per-op
    /// session: a lock-free read snapshot runs the statements, and the exclusive write lock is taken to
    /// flush only when something changed. A transaction must open and close within one call.
    pub fn write_sql_exec(&self, workspace: &str, db: &str, sql: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.sql_exec(workspace, db, sql);
        }
        let (path, auth, daemon_authorized) = self.sql_parts()?;
        with_local_store_write_lock(path, || {
            let mut loom = open_sql_write_loom(path, auth, daemon_authorized)?;
            ensure_sql_ns(&mut loom, workspace)?;
            save_loom(&mut loom)?;
            drop(loom);
            Ok(())
        })?;
        let read = if local_auth_requires_write(auth) {
            with_local_store_write_lock(path, || {
                crate::open_per_request_read_loom(path, auth, daemon_authorized)
            })?
        } else {
            crate::open_per_request_read_loom(path, auth, daemon_authorized)?
        };
        let ns = resolve_ns(&read, workspace)?;
        let mut store = LoomSqlStore::open_write(read, ns, db)?;
        let payload = store.exec_cbor(sql)?;
        if store.in_transaction() {
            return Err(LoomError::invalid(
                "BEGIN without a matching COMMIT/ROLLBACK in one exec: a transaction must open and \
                 resolve within a single sql.exec call",
            ));
        }
        if store.is_dirty() {
            with_local_store_write_lock(path, || {
                let mut loom = open_sql_write_loom(path, auth, daemon_authorized)?;
                let ns = resolve_ns(&loom, workspace)?;
                store.persist(&mut loom, ns, db)?;
                save_loom(&mut loom)?;
                drop(loom);
                Ok(())
            })?;
        }
        Ok(payload)
    }

    /// `sql.commit`: record a version-control commit over the SQL-facet workspace; returns its address.
    pub fn write_sql_commit(
        &self,
        workspace: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            // `sql_commit` records a VCS commit over the SQL-facet workspace, the same `loom.commit` the
            // VCS `commit` runs, so it forwards to the timestamped `VersionControl.commit` for a
            // digest-identical result (the workspace's SQL facet already exists after prior sql writes).
            return backend.vcs_commit(workspace, author, message, timestamp_ms);
        }
        let (path, auth, daemon_authorized) = self.sql_parts()?;
        with_local_store_write_lock(path, || {
            let mut loom = open_sql_write_loom(path, auth, daemon_authorized)?;
            let ns = ensure_sql_ns(&mut loom, workspace)?;
            let digest = loom.commit(ns, author, message, timestamp_ms)?;
            save_loom(&mut loom)?;
            drop(loom);
            Ok(digest.to_string())
        })
    }

    /// The path + key for the SQL session ops, or an error in persistent (held-handle) mode.
    fn sql_parts(&self) -> Result<(&std::path::Path, &LocalOpenAuth, bool)> {
        match self.store.per_request_parts()? {
            Some((path, auth, daemon_authorized)) => Ok((path, auth, daemon_authorized)),
            None => Err(LoomError::invalid(
                "sql.exec/sql.commit require per-request store access (a path), not a held handle",
            )),
        }
    }
}

fn open_sql_write_loom(
    path: &std::path::Path,
    auth: &LocalOpenAuth,
    daemon_authorized: bool,
) -> Result<Loom<FileStore>> {
    let loom = if daemon_authorized {
        open_loom_daemon_authorized_unlocked(path, auth.unlock_key.as_ref())?
    } else {
        open_loom_unlocked(path, auth.unlock_key.as_ref())?
    };
    attach_local_auth(loom, auth)
}

fn ensure_sql_ns(loom: &mut Loom<FileStore>, name: &str) -> Result<WorkspaceId> {
    loom.registry_mut().ensure_for_write(
        &WsSelector::Typed {
            ty: FacetKind::Sql,
            name: name.to_string(),
        },
        fresh_workspace_id()?,
    )
}

fn parse_digests(commits: &[String]) -> Result<Vec<Digest>> {
    commits.iter().map(|c| Digest::parse(c)).collect()
}

fn drive_lease_token_from_response(response: &str) -> Result<DriveLeaseTokenSummary> {
    let mut parts = response.trim_end().split('\t');
    let kind = parts
        .next()
        .ok_or_else(|| LoomError::invalid("empty drive lease response"))?;
    if kind != "lock" {
        return Err(LoomError::invalid(format!(
            "unexpected drive lease response {kind:?}"
        )));
    }
    let key = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing drive lease key"))?;
    let principal = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing drive lease principal"))?;
    let session = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing drive lease session"))?;
    let mode = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing drive lease mode"))?;
    let fence = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing drive lease fence"))?
        .parse::<u64>()
        .map_err(|_| LoomError::invalid("invalid drive lease fence"))?;
    let lease_deadline_ms = parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing drive lease deadline"))?
        .parse()
        .map_err(|_| LoomError::invalid("invalid drive lease deadline"))?;
    Ok(DriveLeaseTokenSummary {
        key: key.to_string(),
        principal: principal.to_string(),
        session: session.to_string(),
        mode: mode.to_string(),
        fence: FenceSummary::from(loom_core::Fence::embedded(fence)),
        lease_deadline_ms,
    })
}

fn drive_lease_break_count_from_response(response: &str) -> Result<usize> {
    let mut parts = response.trim_end().split('\t');
    let kind = parts
        .next()
        .ok_or_else(|| LoomError::invalid("empty drive lease break response"))?;
    if kind != "broken" {
        return Err(LoomError::invalid(format!(
            "unexpected drive lease break response {kind:?}"
        )));
    }
    parts
        .next()
        .ok_or_else(|| LoomError::invalid("missing drive lease break count"))?
        .parse()
        .map_err(|_| LoomError::invalid("invalid drive lease break count"))
}

/// Resolve the actor recorded on a Lane mutation.
///
/// Routine mutations omit `provided` and derive the actor from the effective principal (falling back
/// to the workspace namespace when unauthenticated). An explicit override is honored as-is when it
/// matches the effective principal; when it differs it is authorized through the shared ACL substrate
/// (`Tickets` domain, `Admin` right) rather than any bespoke lane-only policy. Resolving inside the
/// write transaction avoids a TOCTOU gap between the authorization check and the write.
fn resolve_lane_actor(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    provided: Option<&str>,
) -> Result<String> {
    let effective_str = loom.effective_principal()?.map(|p| p.to_string());
    match provided.filter(|value| !value.trim().is_empty()) {
        Some(actor) => {
            if Some(actor) != effective_str.as_deref() {
                loom.authorize_domain(ns, AclDomain::Tickets, AclRight::Admin)?;
            }
            Ok(actor.to_string())
        }
        None => Ok(effective_str.unwrap_or_else(|| ns.to_string())),
    }
}

fn promote_backlog_ticket_to_ready(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    ticket_id: &str,
) -> Result<()> {
    let namespace_workspace = ns.to_string();
    for workspace_id in [namespace_workspace.as_str(), "studio"] {
        let Some(ticket) = loom_tickets::get_ticket(loom, ns, workspace_id, ticket_id)? else {
            continue;
        };
        if ticket.fields.get("status").and_then(|value| value.as_str()) != Some("backlog") {
            return Ok(());
        }
        loom_tickets::update_ticket(
            loom,
            ns,
            TicketUpdateRequest {
                workspace_id,
                ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("ready"),
                observed_source_status: Some("backlog"),
                observed_workflow_version: None,
                assignee: None,
                expected_root: None,
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )?;
        return Ok(());
    }
    Ok(())
}

fn update_lane_metadata(lane: &mut Lane, updated_by: &str) {
    lane.updated_at = now_ms();
    lane.updated_by = updated_by.to_string();
}

fn ticket_receipt(
    operation: &str,
    ticket: TicketSummary,
    root_before: Option<String>,
    changes: Vec<MutationChange>,
) -> MutationEnvelope<TicketSummary> {
    let receipt = MutationReceipt::new(operation, "ticket", ticket.primary_key.clone())
        .operation_id(ticket.operation_id.clone())
        .roots(root_before, Some(ticket.profile_root.clone()))
        .changes(changes);
    MutationEnvelope::new(ticket, receipt)
}

fn relation_receipt(
    operation: &str,
    relation: TicketRelationSummary,
    root_before: Option<String>,
    changes: Vec<MutationChange>,
) -> MutationEnvelope<TicketRelationSummary> {
    let receipt = MutationReceipt::new(operation, "ticket_relation", relation.relation_id.clone())
        .operation_id(Some(relation.operation_id.clone()))
        .roots(root_before, Some(relation.profile_root.clone()))
        .changes(changes);
    MutationEnvelope::new(relation, receipt)
}

fn lane_receipt(
    operation: &str,
    lane: Lane,
    changes: Vec<MutationChange>,
) -> MutationEnvelope<Lane> {
    let receipt = MutationReceipt::new(operation, "lane", lane.lane_id.clone()).changes(changes);
    MutationEnvelope::new(lane, receipt)
}

fn ticket_update_changes(request: &TicketUpdateRequest<'_>) -> Vec<MutationChange> {
    let mut changes = Vec::new();
    if let Some(fields) = request.set_fields
        && let Some(fields) = fields.as_object()
    {
        changes.extend(
            fields
                .iter()
                .map(|(field, value)| MutationChange::field_set(field.clone(), value.to_string())),
        );
    }
    changes.extend(
        request
            .delete_fields
            .iter()
            .map(|field| MutationChange::field_deleted((*field).to_string(), None::<String>)),
    );
    if let Some(target_status) = request.target_status {
        changes.push(MutationChange::field_changed(
            "status",
            request.observed_source_status.map(str::to_string),
            Some(target_status.to_string()),
        ));
    }
    if let Some(assignee) = request.assignee {
        changes.push(MutationChange::field_changed(
            "assignee",
            None::<String>,
            Some(assignee.to_string()),
        ));
    }
    if request.action.is_some() && request.target_status.is_none() {
        changes.push(MutationChange::field_set("lifecycle_action", "applied"));
    }
    if let Some(comment) = request.comment.as_ref() {
        changes.push(MutationChange::field_set(
            "comment",
            comment.comment_type.unwrap_or("general"),
        ));
    }
    changes.extend(request.comments.iter().map(|comment| {
        MutationChange::field_set("comment", comment.comment_type.unwrap_or("general"))
    }));
    changes.extend(request.relation_sets.iter().map(|relation| {
        MutationChange::relation_set(
            relation
                .relation_id
                .map(str::to_string)
                .unwrap_or_else(|| "default".to_string()),
            relation.kind.as_str().to_string(),
            relation.target_id.to_string(),
        )
    }));
    changes.extend(request.relation_removes.iter().map(|relation| {
        MutationChange::field_deleted(format!("relation:{}", relation.relation_id), None::<String>)
    }));
    changes
}

#[cfg(test)]
mod tests {
    use super::{
        DocumentReplaceTextRequest, GraphEdgeWrite, LaneCreateRequest, LaneDeleteRequest,
        LaneTicketUpdateRequest, MeetingsPromoteArtifactToReferenceArtifactRequest,
        MeetingsPromoteReferenceToReferenceArtifactRequest, MeetingsPromoteTaskToTicketRequest,
        PageCreateRequest, StructureBindRequest, StructureCreateRequest, StructureDecomposeRequest,
        StructureLinkRequest, StructureMoveRequest, StructureNodeRequest,
        SubstrateViewDefineRequest, TicketCreateRequest, TicketRelationRemoveRequest,
        TicketRelationRequest, TicketUpdateRequest, WriteAdmissionPolicyRequest,
        apply_workgraph_fact_put, drive_lease_break_count_from_response,
        drive_lease_token_from_response,
    };
    use crate::drive::{DriveGrantShareRequest, DrivePinRetentionRequest};
    use crate::pages::StructureDecomposeItem;
    use crate::reads::FtsSearchReadRequest;
    use crate::writes::{
        MeetingsPromoteDecisionToDecisionLogRequest, MeetingsPromoteQuestionToLifecycleRequest,
    };
    use crate::{LoomMcp, StoreAccess};
    use loom_codec::Value as WireValue;
    use loom_core::error::Code;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{
        AclDomain, AclEffect, AclGrant, AclRight, AclScope, AclScopeKind, AclSubject, Algo, Digest,
        IdentityStore, KvMapConfig, KvTier, Loom, PrincipalKind, key_to_cbor, ledger_range,
    };
    use loom_interchange::{
        Coverage, ImportBatch, ImportBatchItem, ImportExecutionBatch, ImportExecutionPayload,
    };
    use loom_interchange_io::{load_meetings_snapshot, meetings_source_payload_path};
    use loom_lanes::{LaneTicket, LaneTicketPlacement};
    use loom_store::{FileStore, open_loom_unlocked, save_loom};
    use loom_substrate::ActorKind;
    use loom_substrate::admission::WriteAdmissionTarget;
    use loom_substrate::body::{Block, BlockKind, Body, TextRun};
    use loom_substrate::meetings::{
        AnnotationRecord, AnnotationStatus, Coverage as MeetingsCoverage, MeetingRecord,
        MeetingRecordInput, MeetingsProfileSnapshot, MeetingsProfileSnapshotParts, SourceRecord,
        SourceRecordInput, SpanKind, SpanRecord, VocabularyTermInput,
    };
    use loom_substrate::order_token::first_token;
    use loom_substrate::refs::{ReferenceArtifactKind, ReferenceIndex, ReferenceSource};
    use loom_substrate::search::EmbeddingProjectionJob;
    use loom_substrate::workgraph::{WorkgraphFact, WorkgraphFactKind, WorkgraphState};
    use loom_tickets::{ExternalTicketIdentity, TicketProfileReader};
    use serde_json::json;
    use std::path::PathBuf;

    fn temp_path() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let uniq = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "loom-mcp-writes-{}-{seq}-{uniq}.loom",
            std::process::id()
        ))
    }

    fn wire(value: WireValue) -> Vec<u8> {
        loom_codec::encode(&value).unwrap()
    }

    fn wire_back(bytes: &[u8]) -> WireValue {
        loom_codec::decode(bytes).unwrap()
    }

    fn cell_int(value: u64) -> WireValue {
        WireValue::Array(vec![WireValue::Uint(2), WireValue::Uint(value)])
    }

    fn cell_u64(value: u64) -> WireValue {
        WireValue::Array(vec![WireValue::Uint(13), WireValue::Uint(value)])
    }

    fn cell_text(value: &str) -> WireValue {
        WireValue::Array(vec![WireValue::Uint(4), WireValue::Text(value.to_string())])
    }

    fn define_optional_string_fields(
        m: &LoomMcp,
        workspace: &str,
        workspace_id: &str,
        project_id: &str,
        field_ids: &[&str],
    ) {
        for field_id in field_ids {
            m.write_tickets_field_put(
                workspace,
                loom_tickets::TicketFieldDefinitionWriteRequest {
                    workspace_id,
                    project_id,
                    field_id,
                    key: field_id,
                    name: field_id,
                    description: None,
                    field_type: "string",
                    option_set: None,
                    max_length: Some(512),
                    required: false,
                    searchable: true,
                    orderable: false,
                    cardinality: loom_tickets::TicketFieldCardinality::Optional,
                    applicable_type_ids: &[],
                    expected_root: None,
                },
            )
            .unwrap();
        }
    }

    fn vector_bytes(values: &[f32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(values.len() * 4);
        for value in values {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    #[test]
    fn drive_lease_key_and_token_response_are_stable() {
        let ns = WorkspaceId::v4_from_bytes([3u8; 16]);
        let key = crate::drive::drive_lease_key(ns, "main", "file", "file-1").unwrap();
        assert_eq!(key, format!("drive/{ns}/main/file/file-1"));
        assert!(crate::drive::drive_lease_key(ns, "main", "node", "file-1").is_err());
        assert!(crate::drive::drive_lease_key(ns, "main", "file", "bad/id").is_err());

        let token = drive_lease_token_from_response(&format!(
            "lock\t{key}\t{ns}\tmcp:1\texclusive\t7\t1000\n"
        ))
        .unwrap();
        assert_eq!(token.key, key);
        assert_eq!(token.principal, ns.to_string());
        assert_eq!(token.session, "mcp:1");
        assert_eq!(token.mode, "exclusive");
        assert_eq!(token.fence.sequence, 7);
        assert_eq!(token.lease_deadline_ms, 1000);
        assert!(drive_lease_token_from_response("error\tLOCKED\n").is_err());
        assert_eq!(
            drive_lease_break_count_from_response("broken\t2\n").unwrap(),
            2
        );
        assert!(drive_lease_break_count_from_response("released\n").is_err());
    }

    #[test]
    fn drive_lease_tools_fail_closed_without_attached_daemon() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let err = m
            .write_drive_acquire_lease("repo", "main", "file", "file-1", 1000, 0)
            .unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(
            err.message
                .contains("drive lease operations require an attached daemon session")
        );
        let err = m
            .write_drive_break_lease("repo", "main", "file", "file-1")
            .unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        assert!(
            err.message
                .contains("drive lease operations require an attached daemon session")
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_admission_policy_controls_drive_missing_admission() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        assert!(
            m.read_substrate_write_admission_policy("repo", "drive", "main")
                .unwrap()
                .is_none()
        );
        let root = m
            .read_drive_list("repo", "main", "root")
            .unwrap()
            .profile_root;
        let policy = m
            .write_substrate_write_admission_policy_set(WriteAdmissionPolicyRequest {
                workspace: "repo",
                surface: "drive",
                scope_id: "main",
                default_mode: "advisory",
                mandatory_targets: &[WriteAdmissionTarget::new("file", "file-1").unwrap()],
            })
            .unwrap();
        assert_eq!(policy.default_mode, "advisory");
        assert_eq!(policy.mandatory_targets.len(), 1);
        assert_eq!(
            m.read_substrate_write_admission_policy("repo", "drive", "main")
                .unwrap()
                .unwrap(),
            policy
        );
        let err = m
            .write_drive_create_folder("repo", "main", "root", "folder-1", "Specs", &root, None)
            .unwrap_err();
        assert_eq!(err.code, Code::Locked);
        m.write_substrate_write_admission_policy_set(WriteAdmissionPolicyRequest {
            workspace: "repo",
            surface: "drive",
            scope_id: "main",
            default_mode: "mandatory",
            mandatory_targets: &[],
        })
        .unwrap();
        let err = m
            .write_drive_create_folder("repo", "main", "root", "folder-1", "Specs", &root, None)
            .unwrap_err();
        assert_eq!(err.code, Code::Locked);
        m.write_substrate_write_admission_policy_set(WriteAdmissionPolicyRequest {
            workspace: "repo",
            surface: "drive",
            scope_id: "main",
            default_mode: "advisory",
            mandatory_targets: &[],
        })
        .unwrap();
        let folder = m
            .write_drive_create_folder("repo", "main", "root", "folder-1", "Specs", &root, None)
            .unwrap();
        assert_eq!(folder.operation_kind, "folder.created");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn drive_share_and_retention_management_round_trip() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        assert!(m.read_drive_list_shares("repo", "main").unwrap().is_empty());
        let granted = m
            .write_drive_grant_share(
                "repo",
                DriveGrantShareRequest {
                    workspace_id: "main",
                    grant_id: "grant-1",
                    target_kind: "folder",
                    target_id: "root",
                    principal: "05050505-0505-4505-8505-050505050505",
                    role: "editor",
                    granted_at_ms: 100,
                    expires_at_ms: None,
                },
            )
            .unwrap();
        assert_eq!(granted.operation_kind, "share.granted");
        let shares = m.read_drive_list_shares("repo", "main").unwrap();
        assert_eq!(shares.len(), 1);
        assert_eq!(shares[0].target_kind, "folder");
        assert_eq!(shares[0].role, "editor");
        assert_eq!(shares[0].expires_at_ms, None);
        let duplicate = m
            .write_drive_grant_share(
                "repo",
                DriveGrantShareRequest {
                    workspace_id: "main",
                    grant_id: "grant-1",
                    target_kind: "folder",
                    target_id: "root",
                    principal: "05050505-0505-4505-8505-050505050505",
                    role: "editor",
                    granted_at_ms: 100,
                    expires_at_ms: None,
                },
            )
            .unwrap_err();
        assert_eq!(duplicate.code, Code::AlreadyExists);
        let expiring = m
            .write_drive_grant_share(
                "repo",
                DriveGrantShareRequest {
                    workspace_id: "main",
                    grant_id: "grant-2",
                    target_kind: "folder",
                    target_id: "root",
                    principal: "05050505-0505-4505-8505-050505050505",
                    role: "viewer",
                    granted_at_ms: 100,
                    expires_at_ms: Some(200),
                },
            )
            .unwrap();
        assert_eq!(expiring.operation_kind, "share.granted");
        let expired = m
            .write_drive_apply_share_expiry("repo", "main", 500)
            .unwrap();
        assert_eq!(expired.expired_grant_ids, ["grant-2"]);
        assert_eq!(expired.remaining_grants, 1);
        assert_eq!(
            expired
                .operation
                .as_ref()
                .map(|op| op.operation_kind.as_str()),
            Some("share.expired")
        );
        let shares = m.read_drive_list_shares("repo", "main").unwrap();
        assert_eq!(shares.len(), 1);
        assert_eq!(shares[0].grant_id, "grant-1");
        let revoked = m
            .write_drive_revoke_share("repo", "main", "grant-1")
            .unwrap();
        assert_eq!(revoked.operation_kind, "share.revoked");
        assert!(m.read_drive_list_shares("repo", "main").unwrap().is_empty());

        let root = m
            .read_drive_list("repo", "main", "root")
            .unwrap()
            .profile_root;
        assert!(
            m.read_drive_list_retention("repo", "main")
                .unwrap()
                .is_empty()
        );
        let pinned = m
            .write_drive_pin_retention(
                "repo",
                DrivePinRetentionRequest {
                    workspace_id: "main",
                    pin_id: "hold-1",
                    kind: "legal_hold",
                    root: &root,
                    target_entity_id: Some("folder:root"),
                    added_at_ms: 300,
                    expires_at_ms: None,
                },
            )
            .unwrap();
        assert_eq!(pinned.operation_kind, "retention.pinned");
        let pins = m.read_drive_list_retention("repo", "main").unwrap();
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].kind, "legal_hold");
        assert_eq!(pins[0].target_entity_id.as_deref(), Some("folder:root"));
        m.write_drive_pin_retention(
            "repo",
            DrivePinRetentionRequest {
                workspace_id: "main",
                pin_id: "trash-1",
                kind: "trash_subtree",
                root: &root,
                target_entity_id: Some("folder:trash"),
                added_at_ms: 300,
                expires_at_ms: Some(400),
            },
        )
        .unwrap();
        let applied = m.write_drive_apply_retention("repo", "main", 500).unwrap();
        assert_eq!(applied.expired_pin_ids, ["trash-1"]);
        assert_eq!(applied.remaining_pins, 1);
        assert_eq!(
            applied
                .operation
                .as_ref()
                .map(|op| op.operation_kind.as_str()),
            Some("retention.applied")
        );
        let expiring_hold = m
            .write_drive_pin_retention(
                "repo",
                DrivePinRetentionRequest {
                    workspace_id: "main",
                    pin_id: "hold-2",
                    kind: "legal_hold",
                    root: &root,
                    target_entity_id: None,
                    added_at_ms: 300,
                    expires_at_ms: Some(400),
                },
            )
            .unwrap_err();
        assert_eq!(expiring_hold.code, Code::InvalidArgument);
        let unpinned = m
            .write_drive_unpin_retention("repo", "main", "hold-1")
            .unwrap();
        assert_eq!(unpinned.operation_kind, "retention.unpinned");
        assert!(
            m.read_drive_list_retention("repo", "main")
                .unwrap()
                .is_empty()
        );
        let _ = std::fs::remove_file(&path);
    }

    fn setup<T>(path: &std::path::Path, f: impl FnOnce(&mut Loom<FileStore>) -> T) -> T {
        loom_coordination::with_local_store_write_lock(path, || {
            let store = FileStore::create_with_profile(path, Algo::Blake3).unwrap();
            let mut loom = Loom::new(store);
            let out = f(&mut loom);
            save_loom(&mut loom).unwrap();
            drop(loom);
            Ok(out)
        })
        .unwrap()
    }

    fn update_store<T>(path: &std::path::Path, f: impl FnOnce(&mut Loom<FileStore>) -> T) -> T {
        loom_coordination::with_local_store_write_lock(path, || {
            let mut loom = open_loom_unlocked(path, None).unwrap();
            let out = f(&mut loom);
            save_loom(&mut loom).unwrap();
            drop(loom);
            Ok(out)
        })
        .unwrap()
    }

    fn redmine_source_values(
        ticket: &loom_tickets::Ticket,
        field_id: &str,
    ) -> Vec<serde_json::Value> {
        ticket
            .fields
            .get(field_id)
            .unwrap()
            .to_json()
            .as_array()
            .unwrap()
            .iter()
            .map(|value| match value {
                serde_json::Value::String(text) => serde_json::from_str(text).unwrap(),
                value => value.clone(),
            })
            .collect()
    }

    /// A fresh loom with one Cas, one Ledger, and one Files workspace, no data.
    fn fresh(path: &std::path::Path) {
        setup(path, |loom| {
            loom.registry_mut()
                .create(
                    FacetKind::Cas,
                    Some("blobs"),
                    WorkspaceId::v4_from_bytes([1u8; 16]),
                )
                .unwrap();
            loom.registry_mut()
                .create(
                    FacetKind::Ledger,
                    Some("audit"),
                    WorkspaceId::v4_from_bytes([2u8; 16]),
                )
                .unwrap();
            let repo = loom
                .registry_mut()
                .create(
                    FacetKind::Files,
                    Some("repo"),
                    WorkspaceId::v4_from_bytes([3u8; 16]),
                )
                .unwrap();
            loom.registry_mut().add_facet(repo, FacetKind::Vcs).unwrap();
            loom.registry_mut().add_facet(repo, FacetKind::Cas).unwrap();
            loom.registry_mut()
                .create(
                    FacetKind::Kv,
                    Some("cache-ns"),
                    WorkspaceId::v4_from_bytes([4u8; 16]),
                )
                .unwrap();
        });
    }

    fn mcp(path: &std::path::Path) -> crate::LoomMcp {
        crate::LoomMcp::new(StoreAccess::per_request(path, None))
    }

    fn workgraph_fact(task_id: &str, event_id: &str) -> WorkgraphFact {
        WorkgraphFact {
            event_id: event_id.to_string(),
            occurred_at: 1,
            task_id: task_id.to_string(),
            batch_id: "batch-1".to_string(),
            actor_kind: ActorKind::Agent,
            actor_id: "agent:scoped".to_string(),
            correlation_id: event_id.to_string(),
            causation_id: event_id.to_string(),
            attempt: 1,
            previous_state: WorkgraphState::Ready,
            next_state: WorkgraphState::Assigned,
            payload_digest: Digest::hash(Algo::Blake3, event_id.as_bytes()),
            reason_code: None,
            kind: WorkgraphFactKind::AssignmentIssued,
        }
    }

    #[test]
    fn workgraph_fact_write_honors_task_key_scope() {
        let path = temp_path();
        let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
        let root = WorkspaceId::v4_from_bytes([90; 16]);
        let principal = WorkspaceId::v4_from_bytes([91; 16]);
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([92; 16]),
            )
            .unwrap();
        let mut identity = IdentityStore::new(root);
        identity
            .add_principal(principal, "scoped", PrincipalKind::User)
            .unwrap();
        identity.bind_session(principal, "workgraph-scope").unwrap();
        loom.set_identity_store(identity);
        loom.set_session("workgraph-scope");
        loom.acl_store_mut()
            .grant(AclGrant {
                subject: AclSubject::Principal(principal),
                workspace: Some(ns),
                domain: Some(AclDomain::Tickets),
                ref_glob: None,
                scopes: vec![AclScope::Prefix {
                    kind: AclScopeKind::Key,
                    prefix: b"workgraph:task-1".to_vec(),
                }],
                rights: [AclRight::Write].into_iter().collect(),
                effect: AclEffect::Allow,
                predicate: None,
            })
            .unwrap();

        apply_workgraph_fact_put(
            &mut loom,
            "repo",
            &ns.to_string(),
            workgraph_fact("task-1", "event-1").encode().unwrap(),
        )
        .unwrap();
        let denied = apply_workgraph_fact_put(
            &mut loom,
            "repo",
            &ns.to_string(),
            workgraph_fact("task-2", "event-2").encode().unwrap(),
        )
        .unwrap_err();
        assert_eq!(denied.code, Code::PermissionDenied);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn import_submit_batch_persists_canonical_batch() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let mut batch =
            ImportBatch::new("tickets", "jira", "jira:site", 100, Coverage::Partial).unwrap();
        batch
            .items
            .push(ImportBatchItem::new("CORE-1", Digest::hash(Algo::Blake3, b"CORE-1")).unwrap());
        let bytes = batch.encode().unwrap();

        let summary = m.write_import_submit_batch("repo", &bytes).unwrap();

        assert_eq!(summary.workspace, "repo");
        assert_eq!(summary.profile, "tickets");
        assert_eq!(summary.source_system, "jira");
        assert_eq!(summary.source_scope, "jira:site");
        assert_eq!(summary.coverage, "partial");
        assert_eq!(summary.observed_at_ms, 100);
        assert_eq!(summary.item_count, 1);
        assert_eq!(
            summary.batch_digest,
            Digest::hash(Algo::Blake3, &bytes).to_string()
        );

        update_store(&path, |loom| {
            assert_eq!(
                loom.store()
                    .control_get(summary.control_key.as_bytes())
                    .unwrap()
                    .as_deref(),
                Some(bytes.as_slice())
            );
        });
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_execute_batch_runs_source_backed_profile() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let input = json!({
            "source_scope": "redmine://example",
            "projects": [
                {"id": 1, "identifier": "core", "key_prefix": "CORE", "name": "Core"}
            ],
            "issues": [{
                "id": 42,
                "project_identifier": "core",
                "tracker": "Bug",
                "subject": "Login fails"
            }]
        });
        let payload = serde_json::to_vec(&input).unwrap();
        let mut batch = ImportExecutionBatch::new(
            "tickets",
            "redmine",
            "redmine://example",
            100,
            Coverage::Complete,
        )
        .unwrap();
        batch.payloads.push(
            ImportExecutionPayload::new("snapshot.json", "application/json", payload, Algo::Blake3)
                .unwrap(),
        );
        let bytes = batch.encode().unwrap();

        let summary = m.write_import_execute_batch("repo", &bytes, false).unwrap();

        assert_eq!(summary.workspace, "repo");
        assert_eq!(summary.profile, "tickets");
        assert_eq!(summary.source_system, "redmine");
        assert_eq!(summary.source_scope, "redmine://example");
        assert_eq!(summary.coverage, "complete");
        assert_eq!(summary.payload_count, 1);
        assert_eq!(
            summary.execution_digest,
            Digest::hash(Algo::Blake3, &bytes).to_string()
        );
        assert!(summary.changed);
        assert_eq!(summary.rows_imported, 2);
        assert_eq!(summary.operations_applied, 2);

        update_store(&path, |loom| {
            let ns = WorkspaceId::v4_from_bytes([3u8; 16]);
            assert_eq!(
                loom.store()
                    .control_get(summary.control_key.as_bytes())
                    .unwrap()
                    .as_deref(),
                Some(bytes.as_slice())
            );
            let reader = TicketProfileReader::open(loom, ns, &ns.to_string())
                .unwrap()
                .unwrap();
            let identity = ExternalTicketIdentity::new("redmine", "issue:42").unwrap();
            assert!(
                reader
                    .ticket_by_external_identity(&identity)
                    .unwrap()
                    .is_some()
            );
        });
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn redmine_import_snapshot_imports_normalized_batch() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let input = json!({
            "source_scope": "redmine://example",
            "projects": [
                {"id": 1, "identifier": "core", "key_prefix": "CORE", "name": "Core"}
            ],
            "issues": [{
                "id": 42,
                "project_identifier": "core",
                "tracker": "Bug",
                "subject": "Login fails",
                "journals": [{"id": 7, "notes": "Status changed"}],
                "comments": [{"id": 8, "text": "Needs logs"}],
                "attachments": [{"id": 9, "filename": "error.txt"}],
                "time_entries": [{"id": 10, "hours": 1.5}],
                "relations": [{"id": 11, "relation_type": "blocks"}]
            }]
        });
        let bytes = serde_json::to_vec(&input).unwrap();

        let summary = m
            .write_redmine_import_snapshot("repo", "studio", None, &bytes, Some("infer"), false)
            .unwrap();

        assert_eq!(summary.workspace, "repo");
        assert_eq!(summary.profile, "studio");
        assert_eq!(summary.source_scope, "redmine://example");
        assert_eq!(summary.rows_imported, 2);
        assert_eq!(summary.operations_applied, 2);
        assert_eq!(summary.fidelity_issues, 0);

        update_store(&path, |loom| {
            let ns = WorkspaceId::v4_from_bytes([3u8; 16]);
            let reader = TicketProfileReader::open(loom, ns, "studio")
                .unwrap()
                .unwrap();
            let identity = ExternalTicketIdentity::new("redmine", "issue:42").unwrap();
            let ticket = reader
                .ticket_by_external_identity(&identity)
                .unwrap()
                .unwrap();
            assert_eq!(
                redmine_source_values(&ticket, "redmine_journals")[0]["notes"],
                "Status changed"
            );
            assert_eq!(
                redmine_source_values(&ticket, "redmine_relations")[0]["relation_type"],
                "blocks"
            );
        });
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn meetings_import_snapshot_imports_normalized_batch() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let source_digest = Digest::hash(Algo::Blake3, b"source").to_string();
        let input = json!({
            "snapshot_version": 1,
            "profile": "granola-mcp",
            "source_system": "granola-mcp",
            "source_scope": "assistant-session",
            "observed_at": 700,
            "coverage": "partial",
            "items": [{
                "source_entity_id": "note-1",
                "source_digest": source_digest,
                "source_sidecar": {"id": "note-1", "via": "mcp"},
                "title": "Architecture review",
                "summary_text": "Use normalized snapshots.",
                "transcript_spans": [{"text": "The importer writes Meetings records."}],
                "decisions": [{"label": "Keep source payloads."}]
            }]
        });
        let bytes = serde_json::to_vec(&input).unwrap();

        let summary = m
            .write_meetings_import_snapshot("repo", "granola-mcp", &bytes, false)
            .unwrap();

        assert_eq!(summary.workspace, "repo");
        assert_eq!(summary.input_profile, "granola-mcp");
        assert_eq!(summary.source_scope, "assistant-session");
        assert!(summary.changed);
        assert_eq!(summary.rows_imported, 1);
        assert_eq!(summary.operations_applied, summary.operations_planned);
        assert!(summary.payload_bytes > 0);

        update_store(&path, |loom| {
            let ns = WorkspaceId::v4_from_bytes([3u8; 16]);
            let profile_id = ns.to_string();
            let snapshot = load_meetings_snapshot(loom, &profile_id).unwrap().unwrap();
            assert_eq!(snapshot.meetings[0].meeting_id, "meeting/note-1");
            assert_eq!(snapshot.annotations[0].label, "Keep source payloads.");
            assert_eq!(
                loom.read_file_reserved(
                    ns,
                    &meetings_source_payload_path(&profile_id, "note-1", "summary.txt")
                )
                .unwrap(),
                b"Use normalized snapshots."
            );
        });
        let _ = std::fs::remove_file(&path);
    }

    fn seed_meetings_review_snapshot(path: &std::path::Path, profile_id: &str) {
        update_store(path, |loom| {
            let digest = |label: &[u8]| Digest::hash(Algo::Blake3, label);
            let mut source = SourceRecord::new(SourceRecordInput {
                source_id: "src-1",
                source_system: "granola-api",
                external_id: "note-1",
                source_digest: digest(b"source"),
                observed_at_ms: 100,
                access_scope: "personal-notes",
                coverage: MeetingsCoverage::Partial,
            })
            .unwrap();
            source.sidecar_digest = Some(digest(b"sidecar"));
            let mut meeting = MeetingRecord::new(MeetingRecordInput {
                meeting_id: "meet-1",
                title: "Architecture review",
                current_source_digest: digest(b"source"),
                created_at_ms: 100,
                updated_at_ms: 120,
            })
            .unwrap();
            meeting.source_refs = vec!["src-1".to_string()];
            let mut span = SpanRecord::new(
                "span-1",
                "meet-1",
                "src-1",
                SpanKind::TranscriptEntry,
                "granola:note-1/transcript/0",
            )
            .unwrap();
            span.text_digest = Some(digest(b"text"));
            let annotation = AnnotationRecord::new(
                "ann-1",
                "meet-1",
                vec!["span-1".to_string()],
                "Decision",
                "Use normalized import snapshots",
                130,
            )
            .unwrap();
            let mut task = AnnotationRecord::new(
                "task-1",
                "meet-1",
                vec!["span-1".to_string()],
                "Task",
                "Create the follow-up ticket",
                131,
            )
            .unwrap();
            task.status = AnnotationStatus::Observed;
            let mut question = AnnotationRecord::new(
                "question-1",
                "meet-1",
                vec!["span-1".to_string()],
                "Question",
                "Who owns the rollout?",
                132,
            )
            .unwrap();
            question.status = AnnotationStatus::Observed;
            let mut artifact = AnnotationRecord::new(
                "artifact-1",
                "meet-1",
                vec!["span-1".to_string()],
                "Artifact",
                "Design recording",
                133,
            )
            .unwrap();
            artifact.status = AnnotationStatus::Observed;
            let mut reference = AnnotationRecord::new(
                "reference-1",
                "meet-1",
                vec!["span-1".to_string()],
                "Reference",
                "Related roadmap page",
                134,
            )
            .unwrap();
            reference.status = AnnotationStatus::Observed;
            let snapshot = MeetingsProfileSnapshot::new(
                profile_id.to_string(),
                MeetingsProfileSnapshotParts {
                    sources: vec![source],
                    meetings: vec![meeting],
                    spans: vec![span],
                    annotations: vec![annotation, task, question, artifact, reference],
                    vocabulary_terms: Vec::new(),
                    entity_merges: Vec::new(),
                    promotions: Vec::new(),
                    import_runs: Vec::new(),
                    redactions: Vec::new(),
                },
            )
            .unwrap();
            loom.store()
                .control_set(
                    &super::meetings_profile_key(profile_id).unwrap(),
                    snapshot.encode().unwrap(),
                )
                .unwrap();
        });
    }

    #[test]
    fn studio_reindex_persists_no_engine_job() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let result = m.write_studio_reindex("repo", Some("meetings")).unwrap();
        assert_eq!(
            result.workspace,
            WorkspaceId::v4_from_bytes([3u8; 16]).to_string()
        );
        assert_eq!(result.profile, "meetings");
        assert_eq!(result.state, "no_engine");
        assert_eq!(result.model_id, "loom-built-in-embedding");
        assert_eq!(result.vector_records_indexed, 0);
        assert_eq!(result.vector_records_deleted, 0);

        let job = update_store(&path, |loom| {
            let ns = super::resolve_ns(loom, "repo").unwrap();
            let bytes = loom.read_file_reserved(ns, &result.job_path).unwrap();
            EmbeddingProjectionJob::decode(&bytes).unwrap()
        });
        assert_eq!(job.state.as_str(), "no_engine");
        assert_eq!(job.key.facet, "studio");
        assert_eq!(job.key.collection, "meetings");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn meetings_review_writes_maintain_revision_rows() {
        let path = temp_path();
        fresh(&path);
        seed_meetings_review_snapshot(&path, "main");
        let m = mcp(&path);

        let accepted = m
            .write_meetings_accept_annotation("repo", "main", "ann-1")
            .unwrap();
        assert_eq!(accepted.status, "accepted");
        let annotation_history = m
            .read_substrate_history("repo", "main", "meetings:annotation:ann-1")
            .unwrap();
        assert!(annotation_history.index_present);
        assert_eq!(annotation_history.revisions.len(), 1);
        assert_eq!(
            annotation_history.revisions[0].body_media_type,
            "application/vnd.uldren.loom.meetings.annotation+cbor"
        );

        let proposed = m
            .write_meetings_propose_vocabulary(
                "repo",
                "main",
                VocabularyTermInput {
                    term_id: "term-1",
                    kind: "decision",
                    label: "Architecture decision",
                    evidence_annotation_ids: vec!["ann-1".to_string()],
                    created_at_ms: 200,
                },
                Vec::new(),
            )
            .unwrap();
        assert_eq!(proposed.status, "proposed");
        let accepted = m
            .write_meetings_accept_vocabulary("repo", "main", "term-1")
            .unwrap();
        assert_eq!(accepted.status, "accepted");
        let vocabulary_history = m
            .read_substrate_history("repo", "main", "meetings:vocabulary:term-1")
            .unwrap();
        assert!(vocabulary_history.index_present);
        assert_eq!(vocabulary_history.revisions.len(), 2);
        assert_eq!(
            vocabulary_history
                .revisions
                .iter()
                .map(|entry| entry.revision)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            vocabulary_history.revisions[0].body_media_type,
            "application/vnd.uldren.loom.meetings.vocabulary-term+cbor"
        );

        let merge = m
            .write_meetings_add_entity_merge(
                "repo",
                "main",
                "merge-1",
                "entity-1",
                vec!["entity-2".to_string()],
                vec!["ann-1".to_string()],
            )
            .unwrap();
        assert_eq!(merge.merge_id, "merge-1");
        let merge_history = m
            .read_substrate_history("repo", "main", "meetings:entity-merge:merge-1")
            .unwrap();
        assert!(merge_history.index_present);
        assert_eq!(merge_history.revisions.len(), 1);
        assert_eq!(
            merge_history.revisions[0].body_media_type,
            "application/vnd.uldren.loom.meetings.entity-merge+cbor"
        );

        let promotion = m
            .write_meetings_add_promotion(
                "repo",
                "main",
                "promote-1",
                "decision.promoted",
                "ann-1",
                "decision-log",
                "decision:dec-1",
            )
            .unwrap();
        assert_eq!(promotion.promotion_id, "promote-1");
        assert_eq!(promotion.source_annotation_id, "ann-1");
        let promotion_history = m
            .read_substrate_history("repo", "main", "meetings:promotion:promote-1")
            .unwrap();
        assert!(promotion_history.index_present);
        assert_eq!(promotion_history.revisions.len(), 1);
        assert_eq!(
            promotion_history.revisions[0].body_media_type,
            "application/vnd.uldren.loom.meetings.promotion+cbor"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn meetings_task_promotion_creates_ticket_and_records_promotion() {
        let path = temp_path();
        fresh(&path);
        seed_meetings_review_snapshot(&path, "main");
        let m = mcp(&path);
        m.write_tickets_project_create("repo", "main", "core", "CORE", "Core", None)
            .unwrap();
        define_optional_string_fields(
            &m,
            "repo",
            "main",
            "core",
            &["meeting_id", "meeting_annotation_id"],
        );

        let promoted = m
            .write_meetings_promote_task_to_ticket(
                "repo",
                "main",
                MeetingsPromoteTaskToTicketRequest {
                    promotion_id: "promote-task-1",
                    source_annotation_id: "task-1",
                    project_id: "core",
                    ticket_type: "task",
                    policy_labels: &[],
                    expected_ticket_root: None,
                },
            )
            .unwrap();

        assert_eq!(
            promoted.ticket.fields["title"],
            "Create the follow-up ticket"
        );
        assert_eq!(promoted.ticket.external_source.as_deref(), Some("meetings"));
        assert_eq!(promoted.ticket.external_id.as_deref(), Some("task-1"));
        assert_eq!(
            promoted.promotion.target_entity_ref,
            format!("ticket:{}", promoted.ticket.ticket_id)
        );
        assert_eq!(promoted.promotion.operation_kind, "task.promoted");
        assert_eq!(promoted.promotion.target_profile, "tickets");
        let promotion_history = m
            .read_substrate_history("repo", "main", "meetings:promotion:promote-task-1")
            .unwrap();
        assert!(promotion_history.index_present);
        assert_eq!(promotion_history.revisions.len(), 1);

        update_store(&path, |loom| {
            let ns = super::resolve_ns(loom, "repo").unwrap();
            let reader = TicketProfileReader::open(loom, ns, "main")
                .unwrap()
                .unwrap();
            let identity = ExternalTicketIdentity::new("meetings", "task-1").unwrap();
            let ticket = reader
                .ticket_by_external_identity(&identity)
                .unwrap()
                .unwrap();
            assert_eq!(ticket.ticket_id, promoted.ticket.ticket_id);
        });

        let duplicate = m
            .write_meetings_promote_task_to_ticket(
                "repo",
                "main",
                MeetingsPromoteTaskToTicketRequest {
                    promotion_id: "promote-task-2",
                    source_annotation_id: "task-1",
                    project_id: "core",
                    ticket_type: "task",
                    policy_labels: &[],
                    expected_ticket_root: Some(&promoted.ticket.profile_root),
                },
            )
            .unwrap_err();
        assert_eq!(duplicate.code, Code::AlreadyExists);

        let invalid_kind = m
            .write_meetings_promote_task_to_ticket(
                "repo",
                "main",
                MeetingsPromoteTaskToTicketRequest {
                    promotion_id: "promote-decision-1",
                    source_annotation_id: "ann-1",
                    project_id: "core",
                    ticket_type: "task",
                    policy_labels: &[],
                    expected_ticket_root: Some(&promoted.ticket.profile_root),
                },
            )
            .unwrap_err();
        assert_eq!(invalid_kind.code, Code::InvalidArgument);

        let rollback_path = temp_path();
        fresh(&rollback_path);
        seed_meetings_review_snapshot(&rollback_path, "main");
        let m = mcp(&rollback_path);
        m.write_tickets_project_create("repo", "main", "core", "CORE", "Core", None)
            .unwrap();
        define_optional_string_fields(
            &m,
            "repo",
            "main",
            "core",
            &["meeting_id", "meeting_annotation_id"],
        );
        let invalid_promotion = m
            .write_meetings_promote_task_to_ticket(
                "repo",
                "main",
                MeetingsPromoteTaskToTicketRequest {
                    promotion_id: "",
                    source_annotation_id: "task-1",
                    project_id: "core",
                    ticket_type: "task",
                    policy_labels: &[],
                    expected_ticket_root: None,
                },
            )
            .unwrap_err();
        assert_eq!(invalid_promotion.code, Code::InvalidArgument);
        update_store(&rollback_path, |loom| {
            let ns = super::resolve_ns(loom, "repo").unwrap();
            let reader = TicketProfileReader::open(loom, ns, "main")
                .unwrap()
                .unwrap();
            let identity = ExternalTicketIdentity::new("meetings", "task-1").unwrap();
            assert!(
                reader
                    .ticket_by_external_identity(&identity)
                    .unwrap()
                    .is_none()
            );
        });
        let _ = std::fs::remove_file(&rollback_path);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn meetings_decision_promotion_appends_decision_log_and_records_promotion() {
        let path = temp_path();
        fresh(&path);
        seed_meetings_review_snapshot(&path, "main");
        let m = mcp(&path);
        m.write_meetings_accept_annotation("repo", "main", "ann-1")
            .unwrap();

        let promoted = m
            .write_meetings_promote_decision_to_decision_log(
                "repo",
                "main",
                MeetingsPromoteDecisionToDecisionLogRequest {
                    promotion_id: "promote-decision-1",
                    source_annotation_id: "ann-1",
                    decision_id: "dec-1",
                    ledger_name: "decisions",
                },
            )
            .unwrap();

        assert_eq!(promoted.decision_ledger, "decisions");
        assert_eq!(promoted.ledger_sequence, 0);
        assert_eq!(promoted.promotion.operation_kind, "decision.promoted");
        assert_eq!(promoted.promotion.target_profile, "decision-log");
        assert_eq!(promoted.promotion.target_entity_ref, "decision:dec-1");
        let promotion_history = m
            .read_substrate_history("repo", "main", "meetings:promotion:promote-decision-1")
            .unwrap();
        assert!(promotion_history.index_present);
        assert_eq!(promotion_history.revisions.len(), 1);

        update_store(&path, |loom| {
            let ns = super::resolve_ns(loom, "repo").unwrap();
            let entries = ledger_range(loom, ns, "decisions", 0, 1).unwrap();
            assert_eq!(entries.entries.len(), 1);
            let payload: serde_json::Value =
                serde_json::from_slice(&entries.entries[0].payload).unwrap();
            assert_eq!(payload["schema"], "loom.studio.decision-log.entry.v1");
            assert_eq!(payload["decision_id"], "dec-1");
            assert_eq!(payload["label"], "Use normalized import snapshots");
            assert_eq!(payload["meeting_annotation_id"], "ann-1");
        });
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn meetings_question_promotion_creates_lifecycle_and_records_promotion() {
        let path = temp_path();
        fresh(&path);
        seed_meetings_review_snapshot(&path, "main");
        let m = mcp(&path);
        let predicate_digest = Digest::hash(Algo::Blake3, b"answered").to_string();
        m.write_lifecycles_define_standard(
            "repo",
            loom_lifecycle::StandardLifecycleRequest {
                workspace_id: "main",
                kind: "design",
                version: "1",
                completion_predicate_digest: &predicate_digest,
            },
        )
        .unwrap();

        let promoted = m
            .write_meetings_promote_question_to_lifecycle(
                "repo",
                "main",
                MeetingsPromoteQuestionToLifecycleRequest {
                    promotion_id: "promote-question-1",
                    source_annotation_id: "question-1",
                    instance_id: "question-1",
                    definition_id: "design",
                },
            )
            .unwrap();

        assert_eq!(promoted.lifecycle.instance_id, "question-1");
        assert_eq!(promoted.lifecycle.definition_id, "design");
        assert_eq!(
            promoted.lifecycle.subject_refs,
            vec![
                "meeting:meet-1".to_string(),
                "meeting-annotation:question-1".to_string()
            ]
        );
        assert_eq!(promoted.promotion.operation_kind, "question.promoted");
        assert_eq!(promoted.promotion.target_profile, "lifecycle");
        assert_eq!(promoted.promotion.target_entity_ref, "lifecycle:question-1");
        let promotion_history = m
            .read_substrate_history("repo", "main", "meetings:promotion:promote-question-1")
            .unwrap();
        assert!(promotion_history.index_present);
        assert_eq!(promotion_history.revisions.len(), 1);
        let lifecycle = m
            .read_lifecycles_instance("repo", "main", "question-1")
            .unwrap()
            .unwrap();
        assert_eq!(lifecycle.instance_id, "question-1");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn meetings_artifact_and_reference_promotions_create_reference_artifacts() {
        let path = temp_path();
        fresh(&path);
        seed_meetings_review_snapshot(&path, "main");
        let m = mcp(&path);

        let artifact = m
            .write_meetings_promote_artifact_to_reference_artifact(
                "repo",
                "main",
                MeetingsPromoteArtifactToReferenceArtifactRequest {
                    promotion_id: "promote-artifact-1",
                    source_annotation_id: "artifact-1",
                    artifact_id: "artifact-1",
                    target_ref: Some("meeting:meet-1"),
                },
            )
            .unwrap();
        assert_eq!(artifact.promotion.operation_kind, "artifact.promoted");
        assert_eq!(artifact.promotion.target_profile, "references");
        assert_eq!(artifact.promotion.target_entity_ref, "artifact:artifact-1");
        assert_eq!(
            artifact.reference_artifact.entity_ref,
            "artifact:artifact-1"
        );
        assert_eq!(
            artifact.reference_artifact.source_ref,
            "meeting-annotation:artifact-1"
        );
        assert_eq!(
            artifact.reference_artifact.target_ref.as_deref(),
            Some("meeting:meet-1")
        );

        let reference = m
            .write_meetings_promote_reference_to_reference_artifact(
                "repo",
                "main",
                MeetingsPromoteReferenceToReferenceArtifactRequest {
                    promotion_id: "promote-reference-1",
                    source_annotation_id: "reference-1",
                    reference_id: "reference-1",
                    target_ref: Some("page:roadmap"),
                },
            )
            .unwrap();
        assert_eq!(reference.promotion.operation_kind, "reference.promoted");
        assert_eq!(reference.promotion.target_profile, "references");
        assert_eq!(
            reference.promotion.target_entity_ref,
            "reference:reference-1"
        );
        assert_eq!(
            reference.reference_artifact.entity_ref,
            "reference:reference-1"
        );
        assert_eq!(
            reference.reference_artifact.source_ref,
            "meeting-annotation:reference-1"
        );
        assert_eq!(
            reference.reference_artifact.target_ref.as_deref(),
            Some("page:roadmap")
        );

        update_store(&path, |loom| {
            let ns = super::resolve_ns(loom, "repo").unwrap();
            assert!(
                loom_reference::get_reference_artifact(
                    loom,
                    ns,
                    "main",
                    ReferenceArtifactKind::Artifact,
                    "artifact-1"
                )
                .unwrap()
                .is_some()
            );
            assert!(
                loom_reference::get_reference_artifact(
                    loom,
                    ns,
                    "main",
                    ReferenceArtifactKind::Reference,
                    "reference-1"
                )
                .unwrap()
                .is_some()
            );
        });
        let promotion_history = m
            .read_substrate_history("repo", "main", "meetings:promotion:promote-reference-1")
            .unwrap();
        assert!(promotion_history.index_present);
        assert_eq!(promotion_history.revisions.len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lifecycles_define_instantiate_transition_and_read() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let workspace_id = WorkspaceId::v4_from_bytes([3u8; 16]).to_string();
        let actor = WorkspaceId::v4_from_bytes([1u8; 16]).to_string();
        let predicate_digest = Digest::hash(Algo::Blake3, b"done").to_string();

        let definition = m
            .write_lifecycles_define_standard(
                "repo",
                loom_lifecycle::StandardLifecycleRequest {
                    workspace_id: &workspace_id,
                    kind: "feature",
                    version: "1",
                    completion_predicate_digest: &predicate_digest,
                },
            )
            .unwrap();
        assert_eq!(definition.definition_id, "feature");
        assert_eq!(definition.initial_stage_id, "ideate");
        let definition_history = m
            .read_substrate_history("repo", &workspace_id, "lifecycle:definition:feature")
            .unwrap();
        assert!(definition_history.index_present);
        assert_eq!(definition_history.revisions.len(), 1);
        assert_eq!(
            definition_history.revisions[0].body_media_type,
            "application/vnd.uldren.loom.lifecycle.definition+cbor"
        );
        assert!(
            m.read_lifecycles_definitions("repo", &workspace_id)
                .unwrap()
                .iter()
                .any(|item| item.definition_id == "feature")
        );

        let instance = m
            .write_lifecycles_instantiate(
                "repo",
                &workspace_id,
                "feature-1",
                "feature",
                vec!["page:roadmap".to_string()],
            )
            .unwrap();
        assert_eq!(instance.current_stage_id, "ideate");
        let instance_history = m
            .read_substrate_history("repo", &workspace_id, "lifecycle:instance:feature-1")
            .unwrap();
        assert!(instance_history.index_present);
        assert_eq!(instance_history.revisions.len(), 1);

        let result = m
            .write_lifecycles_transition(
                "repo",
                loom_lifecycle::LifecycleTransitionRequest {
                    workspace_id: &workspace_id,
                    instance_id: "feature-1",
                    transition_id: "tr-1",
                    to_stage_id: "draft",
                    actor_principal_id: &actor,
                    gate_evaluations: vec![loom_lifecycle::LifecycleGateEvaluationInput {
                        gate_id: "enter-draft".to_string(),
                        passed: true,
                        principal_id: Some(actor.clone()),
                        evidence_digest: None,
                        evaluated_at_ms: 1,
                    }],
                    snapshot_digest: None,
                    recorded_at_ms: 2,
                },
            )
            .unwrap();
        assert_eq!(result.instance.current_stage_id, "draft");
        assert_eq!(result.surface.surfaced_tools, vec!["pages_update"]);
        assert_eq!(result.operation_log.records.len(), 1);
        let instance_history = m
            .read_substrate_history("repo", &workspace_id, "lifecycle:instance:feature-1")
            .unwrap();
        assert_eq!(instance_history.revisions.len(), 2);
        assert_eq!(
            instance_history
                .revisions
                .iter()
                .map(|entry| entry.revision)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            instance_history.revisions[1].body_media_type,
            "application/vnd.uldren.loom.lifecycle.instance+cbor"
        );

        let read = m
            .read_lifecycles_instance("repo", &workspace_id, "feature-1")
            .unwrap()
            .unwrap();
        assert_eq!(read.current_stage_id, "draft");
        m.write_lifecycles_transition(
            "repo",
            loom_lifecycle::LifecycleTransitionRequest {
                workspace_id: &workspace_id,
                instance_id: "feature-1",
                transition_id: "tr-2",
                to_stage_id: "structure",
                actor_principal_id: &actor,
                gate_evaluations: vec![loom_lifecycle::LifecycleGateEvaluationInput {
                    gate_id: "enter-structure".to_string(),
                    passed: true,
                    principal_id: Some(actor.clone()),
                    evidence_digest: None,
                    evaluated_at_ms: 3,
                }],
                snapshot_digest: None,
                recorded_at_ms: 4,
            },
        )
        .unwrap();
        let snapshot_digest = m.write_cas_put("repo", b"ready snapshot").unwrap();
        let ready = m
            .write_lifecycles_transition(
                "repo",
                loom_lifecycle::LifecycleTransitionRequest {
                    workspace_id: &workspace_id,
                    instance_id: "feature-1",
                    transition_id: "tr-3",
                    to_stage_id: "ready",
                    actor_principal_id: &actor,
                    gate_evaluations: vec![loom_lifecycle::LifecycleGateEvaluationInput {
                        gate_id: "enter-ready".to_string(),
                        passed: true,
                        principal_id: Some(actor.clone()),
                        evidence_digest: Some(predicate_digest.clone()),
                        evaluated_at_ms: 5,
                    }],
                    snapshot_digest: Some(&snapshot_digest),
                    recorded_at_ms: 6,
                },
            )
            .unwrap();
        assert_eq!(
            ready.snapshot.as_ref().unwrap().snapshot_id,
            "feature-1:tr-3"
        );
        assert_eq!(
            m.read_lifecycles_snapshot_content("repo", &workspace_id, "feature-1:tr-3")
                .unwrap()
                .unwrap(),
            b"ready snapshot".to_vec()
        );
        let log = m
            .read_lifecycles_operation_log("repo", &workspace_id)
            .unwrap();
        assert_eq!(log.records[0].operation_kind, "lifecycle.transitioned");
        assert_eq!(log.records.len(), 3);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cas_put_persists_and_reads_back() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let digest = m.write_cas_put("blobs", b"hello").expect("put");
        // The write persisted: a fresh per-request read sees it through the PEP.
        assert_eq!(
            m.read_cas_get("blobs", &digest).expect("get"),
            Some(b"hello".to_vec())
        );
        assert!(m.write_cas_delete("blobs", &digest).expect("delete"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn substrate_view_define_persists_and_reads_back() {
        let path = temp_path();
        fresh(&path);
        let source_root = update_store(&path, |loom| {
            let ns = super::resolve_ns(loom, "repo").unwrap();
            loom.write_file(ns, "plan.md", b"plan", 0o100644).unwrap();
            loom.commit(ns, "owner", "seed", 1).unwrap()
        });
        let m = mcp(&path);
        let view = m
            .write_substrate_view_define(SubstrateViewDefineRequest {
                workspace: "repo",
                view_id: "bootstrap",
                source_scopes: &["organization"],
                source_facets: &["document", "graph"],
                projection_ref: "program:bootstrap-v1",
                output_facet: Some("document"),
                media_type: "text/markdown",
                freshness_policy: "on_read",
            })
            .expect("define view");
        assert_eq!(view.view_id, "bootstrap");
        assert_eq!(view.freshness_policy, "on_read");
        assert_eq!(view.source_digests, vec![source_root.to_string()]);
        let read = m
            .read_substrate_view_get("repo", "bootstrap")
            .expect("read view")
            .expect("view exists");
        assert_eq!(read.view_id, view.view_id);
        assert_eq!(read.source_scopes, view.source_scopes);
        assert_eq!(read.source_facets, view.source_facets);
        assert_eq!(read.projection_ref, view.projection_ref);
        assert_eq!(read.output_facet, view.output_facet);
        assert_eq!(read.media_type, view.media_type);
        assert_eq!(read.freshness_policy, view.freshness_policy);
        assert_eq!(read.output_digest, view.output_digest);
        assert_eq!(read.source_digests, view.source_digests);
        assert_eq!(
            read.projection.as_ref().unwrap()["status"],
            serde_json::json!("target")
        );
        let listed = m.read_substrate_view_list("repo").expect("list views");
        assert_eq!(listed, vec![view]);
        assert!(m.read_substrate_view_list("blobs").unwrap().is_empty());
        assert!(
            m.read_substrate_view_get("repo", "missing")
                .unwrap()
                .is_none()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn drive_writes_use_expected_roots_upload_sessions_logs_and_conflicts() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);

        let root = m
            .read_drive_list("repo", "main", "root")
            .unwrap()
            .profile_root;
        let folder = m
            .write_drive_create_folder("repo", "main", "root", "folder-1", "Specs", &root, None)
            .unwrap();
        assert_eq!(folder.operation_kind, "folder.created");
        let folder_history = m
            .read_substrate_history("repo", "main", "drive:metadata:folder-1")
            .unwrap();
        assert!(folder_history.index_present);
        assert_eq!(folder_history.revisions.len(), 1);
        assert_eq!(folder_history.revisions[0].revision, 1);
        assert_eq!(
            folder_history.revisions[0].body_media_type,
            "application/vnd.uldren.loom.drive.operation+cbor"
        );
        assert!(
            m.write_drive_create_folder("repo", "main", "root", "folder-2", "Other", &root, None)
                .is_err()
        );

        let upload = m
            .write_drive_create_upload(
                "repo",
                crate::drive::DriveCreateUploadRequest {
                    workspace_id: "main",
                    upload_id: "upload-1",
                    parent_folder_id: "folder-1",
                    name: "Plan.txt",
                    file_id: "file-1",
                    expected_root: &folder.profile_root,
                    created_at_ms: 100,
                    replace_file: false,
                },
                None,
            )
            .unwrap();
        assert_eq!(upload.chunk_count, 0);
        let upload = m
            .write_drive_upload_chunk("repo", "main", "upload-1", b"hello drive", None)
            .unwrap();
        assert_eq!(upload.chunk_count, 1);
        let committed = m
            .write_drive_commit_upload("repo", "main", "upload-1", None)
            .unwrap();
        assert_eq!(committed.operation_kind, "file.upload_committed");
        let err = m
            .write_drive_commit_upload("repo", "main", "upload-1", None)
            .unwrap_err();
        assert_eq!(err.code, Code::NotFound);
        assert_eq!(
            m.read_drive_read("repo", "main", "file-1").unwrap(),
            b"hello drive"
        );
        let drive_history = m
            .read_substrate_history("repo", "main", "drive:file:file-1")
            .unwrap();
        assert!(drive_history.index_present);
        assert_eq!(drive_history.revisions.len(), 1);
        assert_eq!(drive_history.revisions[0].revision, 1);
        assert_eq!(drive_history.revisions[0].entity_id, "drive:file:file-1");
        assert_eq!(
            drive_history.revisions[0].body_media_type,
            "application/vnd.uldren.loom.drive.file-content"
        );
        assert!(
            drive_history
                .checkpoints
                .iter()
                .any(|checkpoint| checkpoint.checkpoint_id == "drive:file-1:1")
        );

        let renamed = m
            .write_drive_rename(
                "repo",
                "main",
                "folder-1",
                "file-1",
                "Plan v2.txt",
                &committed.profile_root,
                None,
            )
            .unwrap();
        assert_eq!(renamed.operation_kind, "file.renamed");
        let moved = m
            .write_drive_move(
                "repo",
                "main",
                "folder-1",
                "root",
                "file-1",
                &renamed.profile_root,
                None,
            )
            .unwrap();
        assert_eq!(moved.operation_kind, "file.moved");
        let deleted = m
            .write_drive_delete("repo", "main", "root", "file-1", &moved.profile_root, None)
            .unwrap();
        assert_eq!(deleted.operation_kind, "file.deleted");
        let file_metadata_history = m
            .read_substrate_history("repo", "main", "drive:metadata:file-1")
            .unwrap();
        assert!(file_metadata_history.index_present);
        assert_eq!(file_metadata_history.revisions.len(), 4);
        assert_eq!(
            file_metadata_history
                .revisions
                .iter()
                .map(|entry| entry.revision)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(
            file_metadata_history
                .revisions
                .iter()
                .map(|entry| entry.body_media_type.as_str())
                .collect::<Vec<_>>(),
            vec![
                "application/vnd.uldren.loom.drive.operation+cbor",
                "application/vnd.uldren.loom.drive.operation+cbor",
                "application/vnd.uldren.loom.drive.operation+cbor",
                "application/vnd.uldren.loom.drive.operation+cbor"
            ]
        );
        assert!(
            m.read_drive_stat("repo", "main", "root", "Plan v2.txt")
                .is_err()
        );

        let root_after_delete = m
            .read_drive_list("repo", "main", "root")
            .unwrap()
            .profile_root;
        let first = m
            .write_drive_create_upload(
                "repo",
                crate::drive::DriveCreateUploadRequest {
                    workspace_id: "main",
                    upload_id: "upload-2",
                    parent_folder_id: "root",
                    name: "Budget.xlsx",
                    file_id: "file-2",
                    expected_root: &root_after_delete,
                    created_at_ms: 200,
                    replace_file: false,
                },
                None,
            )
            .unwrap();
        assert_eq!(first.target_kind, "new_file");
        let second = m
            .write_drive_create_upload(
                "repo",
                crate::drive::DriveCreateUploadRequest {
                    workspace_id: "main",
                    upload_id: "upload-3",
                    parent_folder_id: "root",
                    name: "budget.XLSX",
                    file_id: "file-3",
                    expected_root: &root_after_delete,
                    created_at_ms: 300,
                    replace_file: false,
                },
                None,
            )
            .unwrap();
        assert_eq!(second.expected_root, root_after_delete);
        m.write_drive_upload_chunk("repo", "main", "upload-2", b"budget-a", None)
            .unwrap();
        let _first_commit = m
            .write_drive_commit_upload("repo", "main", "upload-2", None)
            .unwrap();
        assert!(
            m.write_drive_commit_upload("repo", "main", "upload-2", None)
                .is_err()
        );
        m.write_drive_upload_chunk("repo", "main", "upload-3", b"budget-b", None)
            .unwrap();
        let conflict = m
            .write_drive_commit_upload("repo", "main", "upload-3", None)
            .unwrap();
        assert_eq!(conflict.conflict_id.as_deref(), Some("upload-3:conflict"));
        assert!(
            m.write_drive_commit_upload("repo", "main", "upload-3", None)
                .is_err()
        );
        let conflicts = m.read_drive_list_conflicts("repo", "main").unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_id, "upload-3:conflict");
        assert_eq!(conflicts[0].resolution, "open");
        let list = m.read_drive_list("repo", "main", "root").unwrap();
        assert!(
            list.entries
                .iter()
                .any(|entry| entry.name.contains("conflicted copy"))
        );
        let resolved = m
            .write_drive_resolve_conflict(
                "repo",
                "main",
                "upload-3:conflict",
                crate::drive::DriveConflictResolutionRequest::Current,
                None,
            )
            .unwrap();
        assert_eq!(resolved.operation_kind, "conflict.resolved");
        let conflicts = m.read_drive_list_conflicts("repo", "main").unwrap();
        assert_eq!(conflicts[0].resolution, "keep_current");
        let list = m.read_drive_list("repo", "main", "root").unwrap();
        assert!(
            !list
                .entries
                .iter()
                .any(|entry| entry.name.contains("conflicted copy"))
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn drive_stale_delete_is_held_when_content_wins() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);

        let root = m
            .read_drive_list("repo", "main", "root")
            .unwrap()
            .profile_root;
        m.write_drive_create_upload(
            "repo",
            crate::drive::DriveCreateUploadRequest {
                workspace_id: "main",
                upload_id: "upload-1",
                parent_folder_id: "root",
                name: "Plan.txt",
                file_id: "file-1",
                expected_root: &root,
                created_at_ms: 100,
                replace_file: false,
            },
            None,
        )
        .unwrap();
        m.write_drive_upload_chunk("repo", "main", "upload-1", b"v1", None)
            .unwrap();
        let committed = m
            .write_drive_commit_upload("repo", "main", "upload-1", None)
            .unwrap();
        let delete_base = committed.profile_root.clone();

        m.write_drive_create_upload(
            "repo",
            crate::drive::DriveCreateUploadRequest {
                workspace_id: "main",
                upload_id: "upload-2",
                parent_folder_id: "root",
                name: "Plan.txt",
                file_id: "file-1",
                expected_root: &delete_base,
                created_at_ms: 200,
                replace_file: true,
            },
            None,
        )
        .unwrap();
        m.write_drive_upload_chunk("repo", "main", "upload-2", b"v2", None)
            .unwrap();
        m.write_drive_commit_upload("repo", "main", "upload-2", None)
            .unwrap();
        let drive_history = m
            .read_substrate_history("repo", "main", "drive:file:file-1")
            .unwrap();
        assert_eq!(drive_history.revisions.len(), 2);
        assert_eq!(drive_history.revisions[1].revision, 2);
        assert!(
            drive_history
                .checkpoints
                .iter()
                .any(|checkpoint| checkpoint.checkpoint_id == "drive:file-1:2")
        );

        let held = m
            .write_drive_delete("repo", "main", "root", "file-1", &delete_base, None)
            .unwrap();
        assert_eq!(held.operation_kind, "file.delete_held");
        assert_eq!(held.conflict_id.as_deref(), Some("delete:file-1:file-1"));
        assert_eq!(m.read_drive_read("repo", "main", "file-1").unwrap(), b"v2");
        let conflicts = m.read_drive_list_conflicts("repo", "main").unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_id, "delete:file-1:file-1");
        assert_eq!(conflicts[0].resolution, "open");

        m.write_drive_resolve_conflict(
            "repo",
            "main",
            "delete:file-1:file-1",
            crate::drive::DriveConflictResolutionRequest::Conflict,
            None,
        )
        .unwrap();
        assert!(
            m.read_drive_stat("repo", "main", "root", "Plan.txt")
                .is_err()
        );
        let conflicts = m.read_drive_list_conflicts("repo", "main").unwrap();
        assert_eq!(conflicts[0].resolution, "keep_conflict");

        let root = m
            .read_drive_list("repo", "main", "root")
            .unwrap()
            .profile_root;
        let folder = m
            .write_drive_create_folder(
                "repo",
                "main",
                "root",
                "folder-held",
                "Held Folder",
                &root,
                None,
            )
            .unwrap();
        m.write_drive_create_upload(
            "repo",
            crate::drive::DriveCreateUploadRequest {
                workspace_id: "main",
                upload_id: "upload-folder-1",
                parent_folder_id: "folder-held",
                name: "Child.txt",
                file_id: "child-file",
                expected_root: &folder.profile_root,
                created_at_ms: 300,
                replace_file: false,
            },
            None,
        )
        .unwrap();
        m.write_drive_upload_chunk("repo", "main", "upload-folder-1", b"child-v1", None)
            .unwrap();
        let child_committed = m
            .write_drive_commit_upload("repo", "main", "upload-folder-1", None)
            .unwrap();
        let folder_delete_base = child_committed.profile_root.clone();
        m.write_drive_create_upload(
            "repo",
            crate::drive::DriveCreateUploadRequest {
                workspace_id: "main",
                upload_id: "upload-folder-2",
                parent_folder_id: "folder-held",
                name: "Child.txt",
                file_id: "child-file",
                expected_root: &folder_delete_base,
                created_at_ms: 400,
                replace_file: true,
            },
            None,
        )
        .unwrap();
        m.write_drive_upload_chunk("repo", "main", "upload-folder-2", b"child-v2", None)
            .unwrap();
        m.write_drive_commit_upload("repo", "main", "upload-folder-2", None)
            .unwrap();
        let held_folder = m
            .write_drive_delete(
                "repo",
                "main",
                "root",
                "folder-held",
                &folder_delete_base,
                None,
            )
            .unwrap();
        assert_eq!(held_folder.operation_kind, "folder.delete_held");
        assert_eq!(
            held_folder.conflict_id.as_deref(),
            Some("delete:folder-held:child-file")
        );
        assert!(
            m.read_drive_list("repo", "main", "root")
                .unwrap()
                .entries
                .iter()
                .any(|entry| entry.node_id == "folder-held")
        );
        assert_eq!(
            m.read_drive_read("repo", "main", "child-file").unwrap(),
            b"child-v2"
        );
        let conflicts = m.read_drive_list_conflicts("repo", "main").unwrap();
        assert!(conflicts.iter().any(|conflict| conflict.conflict_id
            == "delete:folder-held:child-file"
            && conflict.resolution == "open"));
        m.write_drive_resolve_conflict(
            "repo",
            "main",
            "delete:folder-held:child-file",
            crate::drive::DriveConflictResolutionRequest::Conflict,
            None,
        )
        .unwrap();
        assert!(
            m.read_drive_list("repo", "main", "root")
                .unwrap()
                .entries
                .iter()
                .all(|entry| entry.node_id != "folder-held")
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn substrate_alias_bind_resolve_list_and_release_persist() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);

        let binding = m
            .write_substrate_alias_bind("repo", "studio", "LOOM-1", "ticket:01HX")
            .expect("bind alias");
        assert_eq!(binding.alias, "LOOM-1");
        assert_eq!(binding.target, "ticket:01HX");
        assert_eq!(binding.scope_id, "studio");
        assert_eq!(binding.sequence, Some(1));
        assert_eq!(
            m.read_substrate_alias_resolve("repo", "studio", "LOOM-1")
                .expect("resolve alias"),
            Some(binding.clone())
        );
        assert_eq!(
            m.read_substrate_alias_list("repo", "studio")
                .expect("list aliases"),
            vec![binding]
        );

        let rebound = m
            .write_substrate_alias_bind("repo", "studio", "LOOM-1", "ticket:01HY")
            .expect("rebind alias");
        assert_eq!(rebound.sequence, Some(2));
        assert_eq!(rebound.target, "ticket:01HY");
        assert_eq!(
            m.read_substrate_alias_list("repo", "studio")
                .expect("list aliases"),
            vec![rebound.clone()]
        );
        assert!(
            m.write_substrate_alias_release("repo", "studio", "LOOM-1")
                .expect("release alias")
        );
        assert!(
            m.read_substrate_alias_resolve("repo", "studio", "LOOM-1")
                .expect("resolve released alias")
                .is_none()
        );
        assert!(
            !m.write_substrate_alias_release("repo", "studio", "LOOM-1")
                .expect("release missing alias")
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ledger_append_persists() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        assert_eq!(
            m.write_ledger_append("audit", "ev", b"e0".to_vec())
                .expect("append"),
            0
        );
        assert_eq!(
            m.write_ledger_append("audit", "ev", b"e1".to_vec())
                .expect("append"),
            1
        );
        assert_eq!(m.read_ledger_len("audit", "ev").expect("len"), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fs_write_then_commit_and_read() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        m.write_fs_write_file("repo", "a.txt", b"hi", 0o644)
            .expect("write");
        m.write_fs_create_directory("repo", "docs", false)
            .expect("mkdir");
        m.write_fs_write_file("repo", "docs/readme.txt", b"docs", 0o644)
            .expect("write nested");
        assert_eq!(
            m.read_fs_read_file("repo", "a.txt").expect("read"),
            b"hi".to_vec()
        );
        let stat = loom_wire::fs::fs_stat_from_cbor(
            &m.read_fs_stat("repo", "docs/readme.txt").expect("stat"),
        )
        .expect("stat cbor");
        assert_eq!(stat.size, 4);
        let listing = loom_wire::fs::dir_listing_from_cbor(
            &m.read_fs_list_directory("repo", "docs").expect("list"),
        )
        .expect("list cbor");
        assert_eq!(listing.len(), 1);
        assert_eq!(listing[0].name, "readme.txt");
        m.write_fs_remove_directory("repo", "docs", true)
            .expect("rmdir");
        let commit = m.write_vcs_commit("repo", "me", "c1", 0).expect("commit");
        assert!(commit.contains(':'));
        let log = m.read_vcs_log("repo", "main").expect("log");
        assert_eq!(log, vec![commit]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn workspace_rename_and_delete() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        m.write_workspace_rename("blobs", "objects")
            .expect("rename");
        assert!(m.read_workspace_get("objects").expect("get").is_some());
        m.write_workspace_delete("objects").expect("delete");
        assert!(m.read_workspace_get("objects").expect("get").is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_against_missing_loom_errors_not_panics() {
        let m = mcp(&temp_path());
        assert!(m.write_cas_put("blobs", b"x").is_err());
    }

    #[test]
    fn chat_messages_threads_reactions_and_events_persist() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let created = m
            .write_chat_create_channel("repo", "studio", "general", "General")
            .expect("channel");
        assert_eq!(created.channel_handle, "general");
        let posted = m
            .write_chat_post_message("repo", "studio", "general", "m1", None, b"hello".to_vec())
            .expect("post message");
        assert_eq!(posted.operation_kind, "message.created");
        assert_eq!(posted.sequence, 1);
        let thread = m
            .write_chat_create_thread("repo", "studio", "general", "t1", "m1")
            .expect("thread");
        assert_eq!(thread.operation_kind, "thread.created");
        let reply = m
            .write_chat_post_message(
                "repo",
                "studio",
                "general",
                "m2",
                Some("t1"),
                b"reply".to_vec(),
            )
            .expect("reply");
        assert_eq!(reply.sequence, 3);
        m.write_chat_emoji_register("repo", "studio", "approved")
            .expect("register emoji");
        m.write_chat_add_reaction("repo", "studio", "general", "m1", "approved")
            .expect("reaction");
        m.write_chat_edit_message("repo", "studio", "general", "m2", b"edited".to_vec())
            .expect("edit");

        let channel = m
            .read_chat_messages("repo", "studio", "general")
            .expect("messages");
        assert_eq!(channel.messages.len(), 2);
        assert_eq!(channel.threads.len(), 1);
        assert_eq!(channel.threads[0].parent_message_id, "m1");
        assert_eq!(channel.messages[0].message_id, "m1");
        assert_eq!(channel.messages[0].body, b"hello".to_vec());
        assert_eq!(channel.messages[0].body_text.as_deref(), Some("hello"));
        assert_eq!(channel.messages[0].reactions[0].kind, "approved");
        assert_eq!(channel.messages[1].thread_id, Some("t1".to_string()));
        assert_eq!(channel.messages[1].body, b"edited".to_vec());
        assert_eq!(channel.messages[1].body_text.as_deref(), Some("edited"));
        let message_entity = format!("chat:{}:message:m2", created.channel_id);
        let message_history = m
            .read_substrate_history("repo", "studio", &message_entity)
            .expect("chat message history");
        assert!(message_history.index_present);
        assert_eq!(message_history.revisions.len(), 2);
        assert_eq!(message_history.revisions[0].revision, 1);
        assert_eq!(message_history.revisions[1].revision, 2);
        assert_eq!(
            message_history.revisions[1].body_media_type,
            "application/vnd.uldren.loom.chat.operation+cbor"
        );
        assert!(
            message_history.checkpoints.iter().any(
                |checkpoint| checkpoint.checkpoint_id == format!("{}:m2:5", created.channel_id)
            )
        );

        assert_eq!(
            m.read_queue_len(
                "repo",
                &format!(
                    "profile/chat/v1/studio/channels/{}/operations",
                    created.channel_id
                ),
            )
            .expect("chat stream len"),
            5
        );
        let cursor = m
            .read_chat_cursor("repo", "studio", "general")
            .expect("chat cursor");
        assert_eq!(cursor.next_sequence, 0);
        assert_eq!(cursor.head_sequence, 5);
        assert_eq!(cursor.unread_count, 5);
        let advanced = m
            .write_chat_update_cursor("repo", "studio", "general", 3)
            .expect("advance chat cursor");
        assert_eq!(advanced.next_sequence, 3);
        assert_eq!(advanced.head_sequence, 5);
        assert_eq!(advanced.unread_count, 2);
        assert!(
            m.write_chat_update_cursor("repo", "studio", "general", 6)
                .is_err()
        );
        assert!(
            m.write_chat_update_cursor("repo", "studio", "general", 2)
                .is_err()
        );
        m.write_chat_create_task("repo", "studio", "general", "task-1", Some("m1"), "triage")
            .expect("create task");
        m.write_chat_claim_task(
            "repo",
            "studio",
            "general",
            "task-1",
            "claim-1",
            Some("lease-1"),
        )
        .expect("claim task");
        assert!(
            m.write_chat_claim_task("repo", "studio", "general", "task-1", "claim-2", None)
                .is_err()
        );
        m.write_chat_complete_task("repo", "studio", "general", "task-1", "claim-1", Some("m2"))
            .expect("complete task");
        let agent = loom_core::WorkspaceId::v4_from_bytes([9u8; 16]).to_string();
        m.write_chat_invoke_agent(
            "repo",
            "studio",
            "general",
            "invoke-1",
            &agent,
            vec!["m1".to_string()],
            b"summarize".to_vec(),
        )
        .expect("invoke agent");
        m.write_chat_post_message("repo", "studio", "general", "m3", None, b"summary".to_vec())
            .expect("agent message");
        m.write_chat_agent_reply("repo", "studio", "general", "invoke-1", "m3")
            .expect("agent reply");
        m.write_chat_request_handoff(
            "repo",
            "studio",
            "general",
            "handoff-1",
            &agent,
            None,
            Some("needs human"),
        )
        .expect("handoff");
        let channel = m
            .read_chat_messages("repo", "studio", "general")
            .expect("messages with tasks");
        assert_eq!(channel.tasks.len(), 1);
        assert!(matches!(
            channel.tasks[0].state,
            crate::chat::ChatTaskStateSummary::Completed { .. }
        ));
        assert_eq!(channel.agent_invocations.len(), 1);
        assert_eq!(
            channel.agent_invocations[0].prompt_text.as_deref(),
            Some("summarize")
        );
        assert_eq!(channel.agent_invocations[0].reply_message_ids, ["m3"]);
        assert_eq!(channel.handoffs.len(), 1);
        assert_eq!(channel.handoffs[0].reason.as_deref(), Some("needs human"));
        assert_eq!(
            m.read_queue_len(
                "repo",
                &format!(
                    "profile/chat/v1/studio/channels/{}/operations",
                    created.channel_id
                ),
            )
            .expect("expanded chat stream len"),
            12
        );
        let events = m
            .read_chat_fetch_events("repo", "studio", "general", 1, 20)
            .expect("events");
        assert_eq!(events.events.len(), 12);
        assert_eq!(
            events.next,
            format!("oplog:13:chat:studio:{}", created.channel_id)
        );
        match &events.events[0] {
            crate::reads::SubstrateChangeSummary::Operation { operation_kind, .. } => {
                assert_eq!(operation_kind, "message.created");
            }
            _ => panic!("expected chat operation event"),
        }
        match &events.events[11] {
            crate::reads::SubstrateChangeSummary::Operation { operation_kind, .. } => {
                assert_eq!(operation_kind, "handoff.requested");
            }
            _ => panic!("expected chat operation event"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn chat_channels_use_uuid_storage_and_retain_renamed_handles() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let created = m
            .write_chat_create_channel("repo", "studio", "general", "General")
            .expect("create channel");
        assert!(WorkspaceId::parse(&created.channel_id).is_ok());
        let renamed = m
            .write_chat_rename_channel("repo", "studio", "general", "team-chat")
            .expect("rename channel");
        assert_eq!(renamed.channel_id, created.channel_id);
        assert_eq!(renamed.channel_handle, "team-chat");
        m.write_chat_post_message("repo", "studio", "general", "m1", None, b"hello".to_vec())
            .expect("post through retained handle");
        let by_current = m
            .read_chat_messages("repo", "studio", "team-chat")
            .expect("read through current handle");
        assert_eq!(by_current.channel_id, created.channel_id);
        assert_eq!(by_current.channel_handle, "team-chat");
        assert_eq!(by_current.messages.len(), 1);
        let channels = m
            .read_chat_channels("repo", "studio")
            .expect("list channels");
        assert_eq!(channels, vec![renamed]);
        assert!(
            m.write_chat_create_channel("repo", "studio", "general", "Other")
                .is_err()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn chat_messages_project_principal_and_channel_references() {
        let path = temp_path();
        fresh(&path);
        let root = WorkspaceId::v4_from_bytes([88u8; 16]);
        update_store(&path, |loom| {
            let identity = loom_core::IdentityStore::new(root);
            loom.store().save_identity_store(&identity).unwrap();
            loom.set_identity_store(identity);
        });
        let m = mcp(&path);
        let channel = m
            .write_chat_create_channel("repo", "studio", "general", "General")
            .expect("create channel");
        m.write_chat_post_message(
            "repo",
            "studio",
            "general",
            "m1",
            None,
            b"Hello @root in #general".to_vec(),
        )
        .expect("post message");
        let principal = m
            .read_substrate_refs("repo", &format!("principal:{root}"))
            .expect("principal references");
        assert_eq!(principal.inbound.len(), 1);
        assert_eq!(principal.inbound[0].evidence, "@root");
        let channel_refs = m
            .read_substrate_refs("repo", &format!("channel:{}", channel.channel_id))
            .expect("channel references");
        assert_eq!(channel_refs.inbound.len(), 1);
        assert_eq!(channel_refs.inbound[0].evidence, "#general");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn chat_reference_candidates_reconcile_after_a_ticket_is_created() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        m.write_chat_create_channel("repo", "studio", "general", "General")
            .expect("create channel");
        m.write_chat_post_message(
            "repo",
            "studio",
            "general",
            "m1",
            None,
            b"See !ticket:CORE-1".to_vec(),
        )
        .expect("post message");
        let project = m
            .write_tickets_project_create("repo", "studio", "core", "CORE", "Core", None)
            .expect("create project");
        let ticket = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "core",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &json!({"title": "First"}),
                    policy_labels: &[],
                    expected_root: Some(&project.profile_root),
                },
            )
            .expect("create ticket");
        let reconciliation = m
            .write_substrate_reference_reconcile("repo", "studio", 10)
            .expect("reconcile chat reference");
        assert_eq!(reconciliation.processed, 1);
        let refs = m
            .read_substrate_refs("repo", &format!("ticket:{}", ticket.ticket_id))
            .expect("ticket references");
        assert_eq!(refs.inbound.len(), 1);
        assert_eq!(refs.inbound[0].evidence, "See !ticket:CORE-1");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tickets_project_and_history_persist() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let project = m
            .write_tickets_project_create("repo", "studio", "eng", "ENG", "Engineering", None)
            .expect("project");
        assert_eq!(project.sequence, 1);
        assert_eq!(project.key_prefix, "ENG");
        let ticket = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "eng",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &json!({
                        "title": "Build tickets",
                        "description": "Coordinate with !page:Roadmap.",
                        "story_points": 3
                    }),
                    policy_labels: &["internal".to_string()],
                    expected_root: Some(&project.profile_root),
                },
            )
            .expect("ticket");
        assert_eq!(ticket.sequence, Some(2));
        assert_eq!(ticket.primary_key, "ENG-1");
        assert_ne!(ticket.profile_root, project.profile_root);
        let ticket_id = ticket.ticket_id.clone();
        let ticket_ref = format!("ticket:{ticket_id}");
        let stale = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "eng",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &json!({ "title": "Stale" }),
                    policy_labels: &[],
                    expected_root: Some(&project.profile_root),
                },
            )
            .unwrap_err();
        assert_eq!(stale.code, Code::Conflict);
        let read = m
            .read_tickets_get("repo", "studio", &ticket_id, None)
            .expect("get")
            .expect("ticket");
        assert_eq!(read.primary_key, "ENG-1");
        assert_eq!(read.fields["title"], json!("Build tickets"));
        let by_key = m
            .read_tickets_get("repo", "studio", "ENG-1", None)
            .expect("get")
            .expect("ticket by key");
        assert_eq!(by_key.ticket_id, ticket_id);
        let projected = m
            .read_tickets_get("repo", "studio", "ENG-1", Some("jira"))
            .expect("projected get")
            .expect("projected ticket");
        assert_eq!(projected.projection_profile, "jira");
        assert_eq!(projected.projection_kind, "ticket.projected.jira");
        assert_eq!(projected.projection_source, "canonical_ticket");
        assert_eq!(projected.projection_selection_source, "explicit_request");
        assert_eq!(projected.fields["fields.summary"], json!("Build tickets"));
        assert!(!projected.fields.contains_key("title"));
        let alias = m
            .read_substrate_alias_resolve("repo", "studio", "ENG-1")
            .expect("resolve generated alias")
            .expect("generated alias");
        assert_eq!(alias.target, ticket_ref);
        assert_eq!(alias.scope_id, "studio");
        let initial_refs = m
            .read_substrate_refs("repo", "page:Roadmap")
            .expect("ticket refs");
        assert_eq!(initial_refs.inbound.len(), 1);
        assert_eq!(initial_refs.inbound[0].source_facet, "tickets");
        assert_eq!(initial_refs.inbound[0].source_collection, "studio");
        assert_eq!(initial_refs.inbound[0].source_id, ticket_id);
        assert_eq!(initial_refs.inbound[0].field, "description");
        let history = m
            .read_tickets_history("repo", "studio", Some(&ticket_id))
            .expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].operation_kind, "ticket.created");
        let substrate_history = m
            .read_substrate_history("repo", "studio", &ticket_ref)
            .expect("substrate history");
        assert!(substrate_history.index_present);
        assert_eq!(substrate_history.revisions.len(), 1);
        assert_eq!(substrate_history.revisions[0].revision, 1);
        assert_eq!(substrate_history.revisions[0].entity_id, ticket_ref);
        assert_eq!(
            substrate_history.revisions[0].body_media_type,
            "application/vnd.uldren.loom.ticket.ticket+cbor"
        );
        assert_eq!(substrate_history.checkpoints.len(), 1);
        assert_eq!(
            substrate_history.checkpoints[0].checkpoint_id,
            format!("{ticket_id}:2")
        );
        let updated = m
            .write_tickets_update(
                "repo",
                TicketUpdateRequest {
                    workspace_id: "studio",
                    ticket_id: "ENG-1",
                    set_fields: Some(&json!({
                        "title": "Build ticket updates for !page:Updated.",
                        "description": "No external reference.",
                        "status_category": "done"
                    })),
                    delete_fields: &[],
                    action: Some(loom_tickets::TicketLifecycleAction::Claim),
                    target_status: None,
                    observed_source_status: None,
                    observed_workflow_version: None,
                    assignee: None,
                    expected_root: Some(&ticket.profile_root),
                    comment: None,
                    comments: &[],
                    relation_sets: &[],
                    relation_removes: &[],
                },
            )
            .expect("update");
        assert_eq!(updated.sequence, Some(3));
        assert_eq!(
            updated.fields["title"],
            json!("Build ticket updates for !page:Updated.")
        );
        assert_eq!(updated.fields["status_category"], json!("done"));
        assert!(
            m.read_substrate_refs("repo", "page:Roadmap")
                .expect("old ticket refs")
                .inbound
                .is_empty()
        );
        let updated_refs = m
            .read_substrate_refs("repo", "page:Updated")
            .expect("updated ticket refs");
        assert_eq!(updated_refs.inbound.len(), 1);
        assert_eq!(updated_refs.inbound[0].source_facet, "tickets");
        assert_eq!(updated_refs.inbound[0].source_collection, "studio");
        assert_eq!(updated_refs.inbound[0].source_id, ticket_id);
        assert_eq!(updated_refs.inbound[0].field, "title");
        let update_stale = m
            .write_tickets_update(
                "repo",
                TicketUpdateRequest {
                    workspace_id: "studio",
                    ticket_id: &ticket_id,
                    set_fields: Some(&json!({ "title": "Stale update" })),
                    delete_fields: &[],
                    action: None,
                    target_status: None,
                    observed_source_status: None,
                    observed_workflow_version: None,
                    assignee: None,
                    expected_root: Some(&ticket.profile_root),
                    comment: None,
                    comments: &[],
                    relation_sets: &[],
                    relation_removes: &[],
                },
            )
            .unwrap_err();
        assert_eq!(update_stale.code, Code::Conflict);
        let updated_history = m
            .read_tickets_history("repo", "studio", Some("ENG-1"))
            .expect("history");
        assert_eq!(updated_history.len(), 2);
        assert_eq!(updated_history[1].operation_kind, "ticket.transitioned");
        let updated_substrate_history = m
            .read_substrate_history("repo", "studio", &ticket_ref)
            .expect("substrate history");
        assert_eq!(updated_substrate_history.revisions.len(), 2);
        assert_eq!(updated_substrate_history.revisions[1].revision, 2);
        assert_eq!(
            updated_substrate_history.revisions[1].operation_id,
            "studio:3"
        );
        assert_eq!(updated_substrate_history.checkpoints.len(), 2);
        assert_eq!(
            updated_substrate_history.checkpoints[1].checkpoint_id,
            format!("{ticket_id}:3")
        );
        let all_history = m
            .read_tickets_history("repo", "studio", None)
            .expect("history");
        assert_eq!(all_history.len(), 3);
        let before_rekey = loom_tickets::TicketProfileState::decode(
            &FileStore::open(&path)
                .expect("open ticket store")
                .control_get(&loom_tickets::ticket_profile_state_key("studio").expect("state key"))
                .expect("state read")
                .expect("ticket profile state"),
        )
        .expect("decode ticket profile state");
        let rekeyed = m
            .write_tickets_project_rekey(
                "repo",
                "studio",
                "eng",
                "CORE",
                Some(&updated.profile_root),
            )
            .expect("rekey");
        assert_eq!(rekeyed.sequence, 4);
        assert_eq!(rekeyed.key_prefix, "CORE");
        let after_rekey = loom_tickets::TicketProfileState::decode(
            &FileStore::open(&path)
                .expect("open ticket store")
                .control_get(&loom_tickets::ticket_profile_state_key("studio").expect("state key"))
                .expect("state read")
                .expect("ticket profile state"),
        )
        .expect("decode ticket profile state");
        assert_eq!(before_rekey.tickets_root, after_rekey.tickets_root);
        assert_eq!(
            before_rekey.ticket_numbers_root,
            after_rekey.ticket_numbers_root
        );
        let current = m
            .read_substrate_alias_resolve("repo", "studio", "CORE-1")
            .expect("resolve current ticket key")
            .expect("current ticket key");
        assert_eq!(current.kind, "derived_ticket_key");
        assert!(!current.retired);
        assert_eq!(current.sequence, None);
        assert_eq!(current.target, ticket_ref);
        assert!(
            FileStore::open(&path)
                .expect("open ticket store")
                .audit_records()
                .expect("audit records")
                .iter()
                .any(|record| record.action == "tickets.project.rekeyed")
        );
        let retired = m
            .read_substrate_alias_resolve("repo", "studio", "ENG-1")
            .expect("resolve retired ticket key")
            .expect("retired ticket key");
        assert_eq!(retired.kind, "derived_ticket_key");
        assert!(retired.retired);
        assert_eq!(retired.target, ticket_ref);
        let reserved = m
            .write_substrate_alias_bind("repo", "studio", "CORE-2", "page:Roadmap")
            .unwrap_err();
        assert_eq!(reserved.code, Code::AlreadyExists);
        let relation = m
            .write_tickets_relation_set(
                "repo",
                TicketRelationRequest {
                    workspace_id: "studio",
                    ticket_id: &ticket_id,
                    relation_id: Some("spec-doc"),
                    kind: loom_tickets::TicketRelationKind::ReferencesDocument,
                    target_id: "spec/JIRAISH",
                    expected_root: Some(&rekeyed.profile_root),
                },
            )
            .expect("set ticket relation");
        assert_eq!(relation.sequence, 5);
        assert_eq!(relation.ticket_id, ticket_id);
        assert_eq!(relation.relation_id, "spec-doc");
        assert_eq!(relation.target_type, "document");
        assert_eq!(relation.target_id, "spec/JIRAISH");
        assert_eq!(
            relation.graph_edge_id,
            loom_tickets::ticket_relation_edge_id(
                &relation.ticket_id,
                &loom_tickets::TicketRelation::new(
                    relation.relation_id.clone(),
                    loom_tickets::TicketRelationKind::ReferencesDocument,
                    loom_tickets::TicketRelationTargetType::Document,
                    relation.target_id.clone(),
                )
                .expect("relation model")
            )
        );
        let removed_relation = m
            .write_tickets_relation_remove(
                "repo",
                TicketRelationRemoveRequest {
                    workspace_id: "studio",
                    ticket_id: &ticket_id,
                    relation_id: "spec-doc",
                    expected_root: Some(&relation.profile_root),
                },
            )
            .expect("remove ticket relation");
        assert_eq!(removed_relation.sequence, 6);
        assert_eq!(removed_relation.relation_id, "spec-doc");
        assert!(
            m.read_substrate_alias_resolve("repo", "studio", "ENG-1")
                .expect("resolve retired key after relation changes")
                .is_some()
        );
        let blocked_rebind = m
            .write_substrate_alias_bind("repo", "studio", "ENG-1", "page:Roadmap")
            .unwrap_err();
        assert_eq!(blocked_rebind.code, Code::AlreadyExists);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lanes_tools_project_shared_model_and_reject_invalid_status() {
        let path = temp_path();
        fresh(&path);
        update_store(&path, |loom| {
            let repo = loom
                .registry()
                .open(&loom_core::WsSelector::Name("repo".to_string()))
                .unwrap();
            loom.registry_mut()
                .add_facet(repo, FacetKind::Document)
                .unwrap();
            loom.registry_mut()
                .add_facet(repo, FacetKind::Queue)
                .unwrap();
        });
        let m = mcp(&path);
        let lane = m
            .write_lanes_create(
                "repo",
                LaneCreateRequest {
                    lane_id: "agent-3",
                    lane_key: "agent-3",
                    title: "Agent 3 lane",
                    description: "Durable intention for mcp write round-trip.",
                    lane_kind: loom_lanes::LaneKind::Assignment.as_str(),
                    owner_principal: Some("agent:3"),
                    lane_status: "ready",
                    lane_tickets: &[
                        LaneTicket {
                            ticket_id: "MX-102".to_string(),
                            order_key: "F".to_string(),
                        },
                        LaneTicket {
                            ticket_id: "MX-103".to_string(),
                            order_key: "V".to_string(),
                        },
                    ],
                    active_ticket_id: Some("MX-102"),
                    status_report: "ready",
                    reviewer_feedback: "",
                    updated_by: Some("agent:3"),
                },
            )
            .expect("create lane");
        assert_eq!(lane.lane_status, "ready");

        m.write_lanes_update(
            "repo",
            crate::writes::LaneUpdateRequest {
                lane_id: "agent-3",
                title: None,
                description: None,
                lane_status: None,
                status_report: Some("working MX-104"),
                reviewer_feedback: Some("revise order"),
                updated_by: Some("reviewer"),
            },
        )
        .expect("update lane");
        m.write_lanes_ticket_add(
            "repo",
            LaneTicketUpdateRequest {
                lane_id: "agent-3",
                ticket_id: "MX-104",
                placement: LaneTicketPlacement::First,
                updated_by: Some("agent:3"),
            },
        )
        .expect("add ticket");
        m.write_lanes_ticket_remove(
            "repo",
            LaneTicketUpdateRequest {
                lane_id: "agent-3",
                ticket_id: "MX-102",
                placement: LaneTicketPlacement::Append,
                updated_by: Some("agent:3"),
            },
        )
        .expect("remove ticket");
        let replay = update_store(&path, |loom| {
            let repo = loom
                .registry()
                .open(&loom_core::WsSelector::Name("repo".to_string()))
                .unwrap();
            loom_core::delivery::delivery_replay(
                loom,
                repo,
                &loom_lanes::lane_change_stream("repo"),
                "client",
                None,
                false,
                16,
            )
            .unwrap()
        });
        assert!(replay.messages.iter().any(|message| {
            message.envelope.subject == "lane:agent-3"
                && serde_json::from_slice::<serde_json::Value>(&message.payload)
                    .unwrap()["event_kind"]
                    == "lane.ticket_removed"
        }));
        update_store(&path, |loom| {
            let repo = loom
                .registry()
                .open(&loom_core::WsSelector::Name("repo".to_string()))
                .unwrap();
            loom_core::delivery::delivery_ack(
                loom,
                repo,
                &loom_lanes::lane_change_stream("repo"),
                "client",
                replay.messages[0].envelope.seq,
            )
            .unwrap();
        });

        let read = m
            .read_lanes_get("repo", "agent-3")
            .expect("read lane")
            .expect("lane present");
        assert_eq!(read.status_report, "working MX-104");
        assert_eq!(read.reviewer_feedback, "revise order");
        assert_eq!(read.lane_tickets[0].ticket_id, "MX-104");
        assert_eq!(m.read_lanes_list("repo").expect("list lanes"), vec![read]);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lanes_ticket_add_promotes_only_backlog_ticket_to_ready() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let project = m
            .write_tickets_project_create("repo", "studio", "eng", "ENG", "Engineering", None)
            .expect("project");
        let backlog = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "eng",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &json!({
                        "title": "Promote when assigned",
                        "status": "backlog"
                    }),
                    policy_labels: &[],
                    expected_root: Some(&project.profile_root),
                },
            )
            .expect("backlog ticket");
        let planned = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "eng",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &json!({
                        "title": "Do not promote planned",
                        "status": "planned"
                    }),
                    policy_labels: &[],
                    expected_root: Some(&backlog.profile_root),
                },
            )
            .expect("planned ticket");
        m.write_lanes_create(
            "repo",
            LaneCreateRequest {
                lane_id: "agent-7",
                lane_key: "agent-7",
                title: "Agent 7",
                description: "",
                lane_kind: "assignment",
                owner_principal: None,
                lane_status: "ready",
                lane_tickets: &[],
                active_ticket_id: None,
                status_report: "",
                reviewer_feedback: "",
                updated_by: Some("agent:7"),
            },
        )
        .expect("lane");
        m.write_lanes_ticket_add(
            "repo",
            LaneTicketUpdateRequest {
                lane_id: "agent-7",
                ticket_id: &backlog.primary_key,
                placement: LaneTicketPlacement::Append,
                updated_by: Some("agent:7"),
            },
        )
        .expect("add backlog ticket");
        m.write_lanes_ticket_add(
            "repo",
            LaneTicketUpdateRequest {
                lane_id: "agent-7",
                ticket_id: &planned.primary_key,
                placement: LaneTicketPlacement::Append,
                updated_by: Some("agent:7"),
            },
        )
        .expect("add planned ticket");
        let backlog = m
            .read_tickets_get("repo", "studio", &backlog.ticket_id, None)
            .expect("read backlog")
            .expect("backlog ticket exists");
        let planned = m
            .read_tickets_get("repo", "studio", &planned.ticket_id, None)
            .expect("read planned")
            .expect("planned ticket exists");
        assert_eq!(backlog.fields["status"], json!("ready"));
        assert_eq!(planned.fields["status"], json!("planned"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lanes_delete_removes_only_closed_lane_and_preserves_tickets() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let project = m
            .write_tickets_project_create("repo", "studio", "eng", "ENG", "Engineering", None)
            .expect("project");
        let ticket = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "eng",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &json!({
                        "title": "Keep ticket",
                        "status": "ready"
                    }),
                    policy_labels: &[],
                    expected_root: Some(&project.profile_root),
                },
            )
            .expect("ticket");
        let ticket_ids = vec![ticket.primary_key.clone()];
        let lane_tickets = loom_lanes::lane_tickets_from_order(&ticket_ids).expect("lane tickets");
        m.write_lanes_create(
            "repo",
            LaneCreateRequest {
                lane_id: "done-lane",
                lane_key: "done-lane",
                title: "Done lane",
                description: "Historical membership.",
                lane_kind: "assignment",
                owner_principal: None,
                lane_status: "closed",
                lane_tickets: &lane_tickets,
                active_ticket_id: None,
                status_report: "",
                reviewer_feedback: "",
                updated_by: Some("agent:3"),
            },
        )
        .expect("closed lane");
        let open_lane = m
            .write_lanes_create(
                "repo",
                LaneCreateRequest {
                    lane_id: "open-lane",
                    lane_key: "open-lane",
                    title: "Open lane",
                    description: "",
                    lane_kind: "tracking",
                    owner_principal: None,
                    lane_status: "ready",
                    lane_tickets: &[],
                    active_ticket_id: None,
                    status_report: "",
                    reviewer_feedback: "",
                    updated_by: Some("agent:3"),
                },
            )
            .expect("open lane");
        assert_eq!(
            m.write_lanes_delete(
                "repo",
                LaneDeleteRequest {
                    lane_id: &open_lane.lane_id,
                    updated_by: "agent:3",
                },
            )
            .unwrap_err()
            .code,
            Code::InvalidArgument
        );
        let receipt = m
            .write_lanes_delete_receipt(
                "repo",
                LaneDeleteRequest {
                    lane_id: "done-lane",
                    updated_by: "agent:3",
                },
            )
            .expect("delete receipt");
        assert_eq!(receipt.receipt.operation, "lane.deleted");
        assert!(
            receipt
                .receipt
                .changes
                .iter()
                .any(|change| matches!(change, loom_types::MutationChange::ResourceDeleted))
        );
        assert!(
            m.read_lanes_get("repo", "done-lane")
                .expect("read deleted lane")
                .is_none()
        );
        let read_ticket = m
            .read_tickets_get("repo", "studio", &ticket.primary_key, None)
            .expect("ticket read")
            .expect("ticket remains");
        assert_eq!(read_ticket.fields["title"], json!("Keep ticket"));
        assert_eq!(read_ticket.fields["status"], json!("ready"));
        let history = m
            .read_tickets_history("repo", "studio", Some(&ticket.primary_key))
            .expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].operation_kind, "ticket.created");
        assert_eq!(
            m.write_lanes_delete(
                "repo",
                LaneDeleteRequest {
                    lane_id: "missing-lane",
                    updated_by: "agent:3",
                },
            )
            .unwrap_err()
            .code,
            Code::NotFound
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lanes_list_is_fail_soft_and_surfaces_decode_diagnostics() {
        let path = temp_path();
        fresh(&path);
        update_store(&path, |loom| {
            let repo = loom
                .registry()
                .open(&loom_core::WsSelector::Name("repo".to_string()))
                .unwrap();
            loom.registry_mut()
                .add_facet(repo, FacetKind::Document)
                .unwrap();
            loom.registry_mut()
                .add_facet(repo, FacetKind::Queue)
                .unwrap();
        });
        let m = mcp(&path);
        m.write_lanes_create(
            "repo",
            LaneCreateRequest {
                lane_id: "agent-3",
                lane_key: "agent-3",
                title: "Agent 3 lane",
                description: "Healthy coordination lane.",
                lane_kind: loom_lanes::LaneKind::Assignment.as_str(),
                owner_principal: Some("agent:3"),
                lane_status: "ready",
                lane_tickets: &[],
                active_ticket_id: None,
                status_report: "",
                reviewer_feedback: "",
                updated_by: Some("agent:3"),
            },
        )
        .expect("create healthy lane");

        // Inject a malformed lane document directly into the coordination collection.
        update_store(&path, |loom| {
            let repo = loom
                .registry()
                .open(&loom_core::WsSelector::Name("repo".to_string()))
                .unwrap();
            loom_core::document::document_put_text(
                loom,
                repo,
                loom_lanes::LANE_COLLECTION,
                "agent-broken",
                "{ this is not valid lane json",
                None,
            )
            .unwrap();
        });

        // lanes_list is fail-soft: the healthy lane view plus one diagnostic for the broken record.
        let (views, diagnostics) = m
            .read_lanes_list_views_with_diagnostics("repo")
            .expect("fail-soft lane list");
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].lane_id, "agent-3");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].lane_id, "agent-broken");
        assert!(!diagnostics[0].error.is_empty());

        // Targeted lanes_get on the malformed record returns an actionable error, not a silent drop.
        let error = m
            .read_lanes_get_view("repo", "agent-broken")
            .expect_err("malformed targeted get must error");
        assert_eq!(error.code, Code::InvalidArgument);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lanes_update_applies_partial_edits_and_requires_a_field() {
        let path = temp_path();
        fresh(&path);
        update_store(&path, |loom| {
            let repo = loom
                .registry()
                .open(&loom_core::WsSelector::Name("repo".to_string()))
                .unwrap();
            loom.registry_mut()
                .add_facet(repo, FacetKind::Document)
                .unwrap();
            loom.registry_mut()
                .add_facet(repo, FacetKind::Queue)
                .unwrap();
        });
        let m = mcp(&path);
        m.write_lanes_create(
            "repo",
            LaneCreateRequest {
                lane_id: "agent-3",
                lane_key: "agent-3",
                title: "Original title",
                description: "Original description",
                lane_kind: loom_lanes::LaneKind::Assignment.as_str(),
                owner_principal: Some("agent:3"),
                lane_status: "ready",
                lane_tickets: &[],
                active_ticket_id: None,
                status_report: "",
                reviewer_feedback: "",
                updated_by: Some("agent:3"),
            },
        )
        .unwrap();

        // Update the title only: description is left unchanged (omission != clearing).
        let lane = m
            .write_lanes_update(
                "repo",
                crate::writes::LaneUpdateRequest {
                    lane_id: "agent-3",
                    title: Some("New title"),
                    description: None,
                    lane_status: Some("working"),
                    status_report: None,
                    reviewer_feedback: None,
                    updated_by: Some("agent:9"),
                },
            )
            .unwrap();
        assert_eq!(lane.title, "New title");
        assert_eq!(lane.description, "Original description");
        assert_eq!(lane.lane_status, "working");
        assert_eq!(lane.updated_by, "agent:9");

        // Clear the description explicitly with an empty string; title stays unchanged.
        let lane = m
            .write_lanes_update(
                "repo",
                crate::writes::LaneUpdateRequest {
                    lane_id: "agent-3",
                    title: None,
                    description: Some(""),
                    lane_status: None,
                    status_report: Some("working MX-104"),
                    reviewer_feedback: Some("revise order"),
                    updated_by: Some("agent:9"),
                },
            )
            .unwrap();
        assert_eq!(lane.title, "New title");
        assert_eq!(lane.description, "");
        assert_eq!(lane.status_report, "working MX-104");
        assert_eq!(lane.reviewer_feedback, "revise order");

        assert_eq!(
            m.write_lanes_update(
                "repo",
                crate::writes::LaneUpdateRequest {
                    lane_id: "agent-3",
                    title: None,
                    description: None,
                    lane_status: None,
                    status_report: None,
                    reviewer_feedback: None,
                    updated_by: Some("agent:9"),
                },
            )
            .unwrap_err()
            .code,
            Code::InvalidArgument
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tickets_project_settings_round_trips_lifecycle_policy_through_mcp_facades() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let project = m
            .write_tickets_project_create("repo", "studio", "eng", "ENG", "Engineering", None)
            .expect("project");
        assert_eq!(project.lifecycle_authorization_policy, "write_access");
        let updated = m
            .write_tickets_project_settings_set(
                "repo",
                loom_tickets::TicketProjectSettingsRequest {
                    workspace_id: "studio",
                    project_id: "eng",
                    default_projection: None,
                    enable_projections: &[],
                    disable_projections: &[],
                    actor_enforcement: Some(
                        loom_tickets::TicketLifecycleAuthorizationPolicy::Assignee,
                    ),
                    project_owner_principal: None,
                    clear_project_owner_principal: false,
                    acceptance_authorities: None,
                    acceptance_evidence_enforcement: None,
                    required_acceptance_evidence_keys: None,
                    owner_contract_summary: None,
                    owner_contract_details: None,
                    worker_contract_summary: None,
                    worker_contract_details: None,
                    expected_root: Some(&project.profile_root),
                },
            )
            .expect("settings set");
        assert_eq!(updated.lifecycle_authorization_policy, "assignee");
        let read = m
            .read_tickets_project_settings_get("repo", "studio", "eng", false)
            .expect("settings get")
            .expect("project");
        assert_eq!(read.lifecycle_authorization_policy, "assignee");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tickets_generate_uuid_identity_and_enforce_external_identity_uniqueness() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let project = m
            .write_tickets_project_create("repo", "studio", "core", "CORE", "Core", None)
            .expect("project");
        let ticket = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "core",
                    ticket_type: "bug",
                    external_source: Some("jira-cloud"),
                    external_id: Some("10042"),
                    fields: &json!({"title": "External ticket"}),
                    policy_labels: &[],
                    expected_root: Some(&project.profile_root),
                },
            )
            .expect("ticket");
        assert!(WorkspaceId::parse(&ticket.ticket_id).is_ok());
        assert_eq!(ticket.external_source.as_deref(), Some("jira-cloud"));
        assert_eq!(ticket.external_id.as_deref(), Some("10042"));

        let duplicate = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "core",
                    ticket_type: "bug",
                    external_source: Some("jira-cloud"),
                    external_id: Some("10042"),
                    fields: &json!({"title": "Duplicate"}),
                    policy_labels: &[],
                    expected_root: Some(&ticket.profile_root),
                },
            )
            .unwrap_err();
        assert_eq!(duplicate.code, Code::AlreadyExists);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ticket_reference_candidates_reconcile_after_a_later_project_key_exists() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let eng = m
            .write_tickets_project_create("repo", "studio", "eng", "ENG", "Engineering", None)
            .expect("engineering project");
        let eng_ticket = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "eng",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &json!({"description": "Depends on !ticket:CORE-1"}),
                    policy_labels: &[],
                    expected_root: Some(&eng.profile_root),
                },
            )
            .expect("unresolved reference source");
        assert_eq!(
            m.read_substrate_reference_reconciliation_status("repo")
                .expect("reference status")
                .pending,
            1
        );
        let core = m
            .write_tickets_project_create("repo", "studio", "core", "CORE", "Core", None)
            .expect("core project");
        let core_ticket = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "core",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &json!({"title": "Core dependency"}),
                    policy_labels: &[],
                    expected_root: Some(&core.profile_root),
                },
            )
            .expect("reference target");
        let reconciled = m
            .write_substrate_reference_reconcile("repo", "studio", 16)
            .expect("reconcile references");
        assert_eq!(reconciled.processed, 1);
        assert_eq!(reconciled.resolved, 1);
        assert_eq!(reconciled.pending, 0);
        let refs = m
            .read_substrate_refs("repo", &format!("ticket:{}", core_ticket.ticket_id))
            .expect("resolved references");
        assert_eq!(refs.inbound.len(), 1);
        assert_eq!(refs.inbound[0].source_facet, "tickets");
        assert_eq!(refs.inbound[0].source_id, eng_ticket.ticket_id);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn spaces_pages_update_publish_and_history_persist() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let space = m
            .write_spaces_create("repo", "studio", "eng", "Engineering", None)
            .expect("space");
        assert_eq!(space.space_id, "eng");
        assert!(Digest::parse(&space.profile_root).is_ok());
        assert_eq!(
            m.read_spaces_list("repo", "studio").expect("spaces").len(),
            1
        );
        assert_eq!(
            m.read_spaces_get("repo", "studio", "eng")
                .expect("space")
                .expect("space")
                .title,
            "Engineering"
        );
        let page = m
            .write_pages_create(
                "repo",
                PageCreateRequest {
                    workspace_id: "studio",
                    page_id: "page-1",
                    space_id: "eng",
                    parent_page_id: None,
                    title: "Roadmap",
                    expected_root: Some(&space.profile_root),
                },
            )
            .expect("page");
        assert_eq!(page.status, "empty");
        assert!(Digest::parse(&page.profile_root).is_ok());
        let stale_page = m
            .write_pages_create(
                "repo",
                PageCreateRequest {
                    workspace_id: "studio",
                    page_id: "page-2",
                    space_id: "eng",
                    parent_page_id: None,
                    title: "Stale",
                    expected_root: Some(&space.profile_root),
                },
            )
            .unwrap_err();
        assert_eq!(stale_page.code, Code::Conflict);
        let update = m
            .write_pages_update(
                "repo",
                "studio",
                "page-1",
                b"See !ticket:LOOM-1".to_vec(),
                Some(&page.profile_root),
            )
            .expect("update");
        assert_eq!(update.status, "draft");
        assert!(Digest::parse(&update.profile_root).is_ok());
        let space_history = m
            .read_substrate_history("repo", "studio", "space:eng")
            .expect("space operation history");
        assert!(space_history.index_present);
        assert_eq!(space_history.revisions.len(), 1);
        assert_eq!(
            space_history.revisions[0].body_media_type,
            "application/vnd.uldren.loom.pages.operation+cbor"
        );
        let draft_history = m
            .read_substrate_history("repo", "studio", "page:draft:page-1")
            .expect("draft operation history");
        assert!(draft_history.index_present);
        assert_eq!(draft_history.revisions.len(), 2);
        assert_eq!(
            draft_history
                .revisions
                .iter()
                .map(|entry| entry.revision)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        let draft_refs = m
            .read_substrate_refs("repo", "ticket:LOOM-1")
            .expect("draft refs");
        assert!(draft_refs.inbound.is_empty());
        let draft = m
            .read_pages_get("repo", "studio", "page-1")
            .expect("page")
            .expect("page");
        assert_eq!(draft.status, "draft");
        assert_eq!(draft.draft_body, Some(b"See !ticket:LOOM-1".to_vec()));
        let publish = m
            .write_pages_publish("repo", "studio", "page-1", None)
            .expect("publish");
        assert_eq!(publish.outcome, "published");
        assert_eq!(publish.revision, Some(1));
        assert!(Digest::parse(&publish.profile_root).is_ok());
        let published = m
            .read_pages_get("repo", "studio", "page-1")
            .expect("page")
            .expect("page");
        assert_eq!(published.status, "published");
        assert_eq!(published.body, Some(b"See !ticket:LOOM-1".to_vec()));
        assert_eq!(published.draft_body, None);
        let published_refs = m
            .read_substrate_refs("repo", "ticket:LOOM-1")
            .expect("published refs");
        assert!(!published_refs.degraded.is_degraded);
        assert_eq!(published_refs.indexed_facets, vec!["pages".to_string()]);
        assert_eq!(published_refs.inbound.len(), 1);
        assert_eq!(published_refs.inbound[0].source_facet, "pages");
        assert_eq!(published_refs.inbound[0].source_collection, "studio");
        assert_eq!(published_refs.inbound[0].source_id, "page-1");
        assert_eq!(published_refs.inbound[0].field, "published_body");
        let history = m
            .read_pages_history("repo", "studio", "page-1")
            .expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].kind, "revision");
        let substrate_history = m
            .read_substrate_history("repo", "studio", "page:page-1")
            .expect("substrate history");
        assert!(substrate_history.index_present);
        assert_eq!(substrate_history.revisions.len(), 1);
        assert_eq!(substrate_history.revisions[0].revision, 1);
        assert_eq!(substrate_history.revisions[0].entity_id, "page:page-1");
        assert!(
            substrate_history
                .checkpoints
                .iter()
                .any(|checkpoint| checkpoint.checkpoint_id == "page-1:1")
        );
        let second_update = m
            .write_pages_update_text(
                "repo",
                "studio",
                "page-1",
                "v2",
                Some(&publish.profile_root),
            )
            .expect("second update");
        assert_ne!(second_update.profile_root, publish.profile_root);
        let second_draft = m
            .read_pages_get("repo", "studio", "page-1")
            .expect("page")
            .expect("page");
        assert_eq!(second_draft.draft_body_text.as_deref(), Some("v2\n"));
        let draft_history = m
            .read_substrate_history("repo", "studio", "page:draft:page-1")
            .expect("updated draft operation history");
        assert_eq!(draft_history.revisions.len(), 3);
        assert_eq!(draft_history.revisions[2].revision, 3);
        let stale_update = m
            .write_pages_update(
                "repo",
                "studio",
                "page-1",
                b"v3".to_vec(),
                Some(&publish.profile_root),
            )
            .unwrap_err();
        assert_eq!(stale_update.code, Code::Conflict);
        let stale = m
            .write_pages_publish("repo", "studio", "page-1", Some(&publish.profile_root))
            .unwrap_err();
        assert_eq!(stale.code, Code::Conflict);
        let changes = m
            .read_substrate_changes("repo", "oplog:1:pages:studio", 10)
            .expect("page operation changes");
        assert_eq!(changes.next, "oplog:6:pages:studio");
        assert_eq!(changes.events.len(), 5);
        let kinds = changes
            .events
            .iter()
            .map(|event| match event {
                crate::reads::SubstrateChangeSummary::Operation { operation_kind, .. } => {
                    operation_kind.as_str()
                }
                _ => panic!("expected operation changes"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                "space.created",
                "page.created",
                "page.updated",
                "page.published",
                "page.updated"
            ]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pages_publish_projects_block_ref_transclusion_edges() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let space = m
            .write_spaces_create("repo", "studio", "eng", "Engineering", None)
            .expect("space");
        let target = m
            .write_pages_create(
                "repo",
                PageCreateRequest {
                    workspace_id: "studio",
                    page_id: "target",
                    space_id: "eng",
                    parent_page_id: None,
                    title: "Target",
                    expected_root: Some(&space.profile_root),
                },
            )
            .expect("target page");
        let target_body = Body::new(vec![
            Block::new(
                "intro",
                first_token(),
                BlockKind::Paragraph,
                vec![TextRun::new("hello", vec![]).unwrap()],
                vec![],
            )
            .unwrap(),
        ]);
        let target_update = m
            .write_pages_update(
                "repo",
                "studio",
                "target",
                target_body.encode().unwrap(),
                Some(&target.profile_root),
            )
            .expect("target update");
        let target_publish = m
            .write_pages_publish(
                "repo",
                "studio",
                "target",
                Some(&target_update.profile_root),
            )
            .expect("target publish");
        assert_eq!(target_publish.outcome, "published");
        let page = m
            .write_pages_create(
                "repo",
                PageCreateRequest {
                    workspace_id: "studio",
                    page_id: "source",
                    space_id: "eng",
                    parent_page_id: None,
                    title: "Source",
                    expected_root: Some(&target_publish.profile_root),
                },
            )
            .expect("page");
        let body = Body::new(vec![
            Block::new(
                "ref-block",
                first_token(),
                BlockKind::BlockRef {
                    entity_id: "page:target".to_string(),
                    block_id: Some("intro".to_string()),
                    section: false,
                    pin: Some(1),
                },
                vec![],
                vec![
                    Block::new(
                        "child",
                        first_token(),
                        BlockKind::Paragraph,
                        vec![TextRun::new("child text", vec![]).unwrap()],
                        vec![],
                    )
                    .unwrap(),
                ],
            )
            .unwrap(),
        ]);
        let update = m
            .write_pages_update(
                "repo",
                "studio",
                "source",
                body.encode().unwrap(),
                Some(&page.profile_root),
            )
            .expect("update");
        let publish = m
            .write_pages_publish("repo", "studio", "source", Some(&update.profile_root))
            .expect("publish");
        assert_eq!(publish.outcome, "published");

        let refs = m.read_substrate_refs("repo", "page:target").expect("refs");

        assert_eq!(refs.inbound.len(), 1);
        assert_eq!(refs.inbound[0].source_facet, "pages");
        assert_eq!(refs.inbound[0].source_collection, "studio");
        assert_eq!(refs.inbound[0].source_id, "source");
        assert_eq!(refs.inbound[0].field, "block_ref");
        assert_eq!(refs.inbound[0].relation, "transcludes");
        let rendered = m
            .read_pages_get("repo", "studio", "source")
            .expect("source page")
            .expect("source page");
        assert_eq!(rendered.rendered_body, Some("hello\n".to_string()));
        assert!(rendered.render_issues.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn structures_project_to_graph_and_persist() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let space = m
            .write_spaces_create("repo", "studio", "eng", "Engineering", None)
            .expect("space");
        let stale_profile_root = space.profile_root.clone();
        let mut profile_root = space.profile_root;
        let structure = m
            .write_structures_create(
                "repo",
                StructureCreateRequest {
                    workspace_id: "studio",
                    structure_id: "roadmap",
                    space_id: "eng",
                    kind: "mindmap",
                    title: "Roadmap",
                    expected_root: Some(&profile_root),
                },
            )
            .expect("structure");
        assert_eq!(structure.structure.kind, "mindmap");
        assert_eq!(structure.nodes.len(), 0);
        profile_root = structure.structure.profile_root.clone();
        let stale_add = m
            .write_structures_add_node(
                "repo",
                StructureNodeRequest {
                    workspace_id: "studio",
                    structure_id: "roadmap",
                    node_id: "stale",
                    kind: "topic",
                    label: "Stale",
                    body_digest: None,
                    entity_ref: None,
                    expected_root: Some(&stale_profile_root),
                },
            )
            .unwrap_err();
        assert_eq!(stale_add.code, Code::Conflict);
        let root = m
            .write_structures_add_node(
                "repo",
                StructureNodeRequest {
                    workspace_id: "studio",
                    structure_id: "roadmap",
                    node_id: "root",
                    kind: "topic",
                    label: "Root",
                    body_digest: None,
                    entity_ref: None,
                    expected_root: Some(&profile_root),
                },
            )
            .expect("root");
        profile_root = root.profile_root;
        let feature = m
            .write_structures_add_node(
                "repo",
                StructureNodeRequest {
                    workspace_id: "studio",
                    structure_id: "roadmap",
                    node_id: "feature",
                    kind: "topic",
                    label: "Feature",
                    body_digest: None,
                    entity_ref: Some("page:page-1".to_string()),
                    expected_root: Some(&profile_root),
                },
            )
            .expect("feature");
        profile_root = feature.profile_root;
        let updated = m
            .write_structures_update_node(
                "repo",
                StructureNodeRequest {
                    workspace_id: "studio",
                    structure_id: "roadmap",
                    node_id: "feature",
                    kind: "feature",
                    label: "Feature updated",
                    body_digest: None,
                    entity_ref: Some("page:page-2".to_string()),
                    expected_root: Some(&profile_root),
                },
            )
            .expect("updated feature");
        assert_eq!(updated.kind, "feature");
        assert_eq!(updated.label, "Feature updated");
        assert_eq!(updated.entity_ref, Some("page:page-2".to_string()));
        profile_root = updated.profile_root;
        let bound = m
            .write_structures_bind(
                "repo",
                StructureBindRequest {
                    workspace_id: "studio",
                    structure_id: "roadmap",
                    node_id: "feature",
                    entity_ref: Some("ticket:LOOM-1".to_string()),
                    expected_root: Some(&profile_root),
                },
            )
            .expect("bound feature");
        assert_eq!(bound.entity_ref, Some("ticket:LOOM-1".to_string()));
        profile_root = bound.profile_root;
        let moved = m
            .write_structures_move_node(
                "repo",
                StructureMoveRequest {
                    workspace_id: "studio",
                    structure_id: "roadmap",
                    node_id: "feature",
                    parent_node_id: Some("root"),
                    label: None,
                    expected_root: Some(&profile_root),
                },
            )
            .expect("move feature");
        assert_eq!(moved.parent_node_id, Some("root".to_string()));
        assert_eq!(moved.label, "child_of");
        profile_root = moved.profile_root;
        let edge = m
            .write_structures_link_node(
                "repo",
                StructureLinkRequest {
                    workspace_id: "studio",
                    structure_id: "roadmap",
                    edge_id: "edge-1",
                    src_node_id: "root",
                    dst_node_id: "feature",
                    label: "child_of",
                    target_ref: None,
                    expected_root: Some(&profile_root),
                },
            )
            .expect("edge");
        assert_eq!(edge.label, "child_of");
        profile_root = edge.profile_root;
        let render = m
            .read_structures_get("repo", "studio", "roadmap")
            .expect("structure")
            .expect("structure");
        assert_eq!(render.structure.profile_root, profile_root);
        assert_eq!(render.structure.root_node_id, Some("root".to_string()));
        assert_eq!(render.nodes.len(), 2);
        assert!(
            render
                .edges
                .iter()
                .any(|edge| edge.edge_id == "roadmap:child_of:root:feature")
        );
        assert!(
            m.read_graph_get_node("repo", &render.graph_collection, "feature")
                .expect("graph node")
                .is_some()
        );
        let feature = render
            .nodes
            .iter()
            .find(|node| node.node_id == "feature")
            .expect("feature node");
        assert_eq!(feature.label, "Feature updated");
        assert_eq!(feature.entity_ref, Some("ticket:LOOM-1".to_string()));
        let structure_history = m
            .read_substrate_history("repo", "studio", "structure:roadmap")
            .expect("structure history");
        assert!(structure_history.index_present);
        assert_eq!(structure_history.revisions.len(), 1);
        assert_eq!(
            structure_history.revisions[0].body_media_type,
            "application/vnd.uldren.loom.pages.operation+cbor"
        );
        let node_history = m
            .read_substrate_history("repo", "studio", "structure-node:feature")
            .expect("structure node history");
        assert_eq!(node_history.revisions.len(), 4);
        assert_eq!(
            node_history
                .revisions
                .iter()
                .map(|entry| entry.revision)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        let edge_history = m
            .read_substrate_history("repo", "studio", "structure-edge:edge-1")
            .expect("structure edge history");
        assert_eq!(edge_history.revisions.len(), 1);
        let changes = m
            .read_substrate_changes("repo", "oplog:1:pages:studio", 20)
            .expect("structure operation changes");
        assert_eq!(changes.next, "oplog:9:pages:studio");
        let kinds = changes
            .events
            .iter()
            .map(|event| match event {
                crate::reads::SubstrateChangeSummary::Operation { operation_kind, .. } => {
                    operation_kind.as_str()
                }
                _ => panic!("expected operation changes"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                "space.created",
                "structure.created",
                "structure.node_added",
                "structure.node_added",
                "structure.node_updated",
                "structure.node_bound",
                "structure.node_moved",
                "structure.node_linked"
            ]
        );
        let project = m
            .write_tickets_project_create("repo", "studio", "eng", "ENG", "Engineering", None)
            .expect("ticket project");
        define_optional_string_fields(&m, "repo", "studio", "eng", &["source_ref"]);
        let decompose_fields = json!({ "title": "Override title", "priority": "high" });
        let policy_labels = vec!["planning".to_string()];
        let decompose_items = vec![StructureDecomposeItem {
            node_id: "feature",
            project_id: "eng",
            ticket_type: Some("story"),
            fields: Some(&decompose_fields),
            policy_labels: &policy_labels,
        }];
        let decompose = m
            .write_structures_decompose_to_tickets(
                "repo",
                StructureDecomposeRequest {
                    workspace_id: "studio",
                    structure_id: "roadmap",
                    items: &decompose_items,
                },
            )
            .expect("decompose");
        assert_eq!(project.sequence, 1);
        assert_eq!(decompose.tickets.len(), 1);
        assert!(!decompose.tickets[0].ticket_id.is_empty());
        assert_eq!(decompose.tickets[0].ticket_type, "story");
        assert_eq!(
            decompose.tickets[0].fields["title"],
            json!("Override title")
        );
        assert_eq!(
            decompose.implemented_by_edges,
            vec![format!(
                "roadmap:implemented_by:feature:{}",
                decompose.tickets[0].ticket_id
            )]
        );
        assert!(
            m.read_graph_get_node(
                "repo",
                &decompose.graph_collection,
                &format!("ticket:{}", decompose.tickets[0].ticket_id)
            )
            .expect("ticket graph node")
            .is_some()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn workspace_create_then_use() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let id = m
            .write_workspace_create(Some("notes"), "document")
            .expect("create");
        assert!(!id.is_empty());
        // The new workspace is usable for writes.
        m.write_document_put("notes", "c", "d1", b"{}".to_vec())
            .expect("doc put");
        assert_eq!(
            m.read_document_get_text("notes", "c", "d1")
                .expect("get")
                .map(|document| document.text),
            Some("{}".to_string())
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn document_writes_update_existing_reference_projection() {
        let path = temp_path();
        fresh(&path);
        update_store(&path, |loom| {
            let ns = WorkspaceId::v4_from_bytes([3u8; 16]);
            let mut index = ReferenceIndex::new();
            index
                .add_text_refs(
                    ReferenceSource::new("tickets", "studio", "ticket-1", "description").unwrap(),
                    "refers_to",
                    "See !ticket:KEEP.",
                )
                .unwrap();
            loom.create_directory_reserved(ns, crate::substrate_refs::REF_INDEX_DIR, true)
                .unwrap();
            loom.write_file_reserved(
                ns,
                crate::substrate_refs::REF_INDEX_PATH,
                &index.encode().unwrap(),
                0o100644,
            )
            .unwrap();
        });
        let m = mcp(&path);

        m.write_document_put("repo", "pages", "intro", b"See !ticket:OLD.".to_vec())
            .unwrap();
        assert_eq!(
            m.read_substrate_refs("repo", "ticket:OLD")
                .unwrap()
                .inbound
                .len(),
            1
        );
        let base = Digest::hash(Algo::Blake3, b"See !ticket:OLD.").to_string();
        m.write_document_replace_text(DocumentReplaceTextRequest {
            workspace: "repo",
            name: "pages",
            id: "intro",
            base_digest: &base,
            find: "!ticket:OLD",
            replace: "!ticket:NEW",
            replace_all: true,
        })
        .unwrap();
        assert_eq!(
            m.read_substrate_refs("repo", "ticket:OLD")
                .unwrap()
                .inbound
                .len(),
            0
        );
        assert_eq!(
            m.read_substrate_refs("repo", "ticket:NEW").unwrap().inbound[0].source_facet,
            "document"
        );
        assert_eq!(
            m.read_substrate_refs("repo", "ticket:KEEP")
                .unwrap()
                .inbound[0]
                .source_facet,
            "tickets"
        );

        assert!(m.write_document_delete("repo", "pages", "intro").unwrap());
        assert_eq!(
            m.read_substrate_refs("repo", "ticket:NEW")
                .unwrap()
                .inbound
                .len(),
            0
        );
        assert_eq!(
            m.read_substrate_refs("repo", "ticket:KEEP")
                .unwrap()
                .inbound
                .len(),
            1
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn document_write_bootstraps_missing_reference_projection() {
        let path = temp_path();
        fresh(&path);
        update_store(&path, |loom| {
            let ns = super::resolve_ns(loom, "repo").unwrap();
            loom_core::document::doc_put(
                loom,
                ns,
                "pages",
                "existing",
                b"Existing link !ticket:EXISTING.".to_vec(),
            )
            .unwrap();
        });
        let m = mcp(&path);

        m.write_document_put("repo", "pages", "new", b"New link !ticket:NEW.".to_vec())
            .unwrap();

        let existing = m.read_substrate_refs("repo", "ticket:EXISTING").unwrap();
        assert!(!existing.degraded.is_degraded);
        assert_eq!(existing.inbound.len(), 1);
        assert_eq!(existing.inbound[0].source_id, "existing");
        let new = m.read_substrate_refs("repo", "ticket:NEW").unwrap();
        assert!(!new.degraded.is_degraded);
        assert_eq!(new.inbound.len(), 1);
        assert_eq!(new.inbound[0].source_id, "new");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn graph_edge_writes_update_existing_reference_projection() {
        let path = temp_path();
        fresh(&path);
        update_store(&path, |loom| {
            let ns = WorkspaceId::v4_from_bytes([3u8; 16]);
            let index = ReferenceIndex::new();
            loom.create_directory_reserved(ns, crate::substrate_refs::REF_INDEX_DIR, true)
                .unwrap();
            loom.write_file_reserved(
                ns,
                crate::substrate_refs::REF_INDEX_PATH,
                &index.encode().unwrap(),
                0o100644,
            )
            .unwrap();
        });
        let m = mcp(&path);

        m.write_graph_upsert_node(
            "repo",
            "links",
            "page:Roadmap",
            &wire(WireValue::Map(vec![])),
        )
        .unwrap();
        m.write_graph_upsert_node(
            "repo",
            "links",
            "ticket:LOOM-1",
            &wire(WireValue::Map(vec![])),
        )
        .unwrap();
        m.write_graph_upsert_edge(
            "repo",
            "links",
            GraphEdgeWrite {
                id: "edge-1",
                src: "page:Roadmap",
                dst: "ticket:LOOM-1",
                label: "refers_to",
                props: &wire(WireValue::Map(vec![])),
            },
        )
        .unwrap();

        let result = m.read_substrate_refs("repo", "ticket:LOOM-1").unwrap();
        assert!(!result.degraded.is_degraded);
        assert_eq!(result.inbound.len(), 1);
        assert_eq!(result.inbound[0].source_facet, "graph");
        assert_eq!(result.inbound[0].source_collection, "links");
        assert_eq!(result.inbound[0].source_id, "edge-1");
        assert_eq!(
            result.inbound[0].evidence,
            "page:Roadmap refers_to ticket:LOOM-1"
        );

        assert!(
            m.write_graph_remove_edge("repo", "links", "edge-1")
                .unwrap()
        );
        assert_eq!(
            m.read_substrate_refs("repo", "ticket:LOOM-1")
                .unwrap()
                .inbound
                .len(),
            0
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn graph_edge_write_bootstraps_missing_reference_projection() {
        let path = temp_path();
        fresh(&path);
        update_store(&path, |loom| {
            let ns = super::resolve_ns(loom, "repo").unwrap();
            let props = std::collections::BTreeMap::new();
            loom_core::graph_upsert_node(loom, ns, "links", "page:Existing", props.clone())
                .unwrap();
            loom_core::graph_upsert_node(loom, ns, "links", "ticket:EXISTING", props.clone())
                .unwrap();
            loom_core::graph_upsert_edge(
                loom,
                ns,
                "links",
                "edge-existing",
                "page:Existing",
                "ticket:EXISTING",
                "refers_to",
                props,
            )
            .unwrap();
        });
        let m = mcp(&path);
        m.write_graph_upsert_node("repo", "links", "page:New", &wire(WireValue::Map(vec![])))
            .unwrap();
        m.write_graph_upsert_node("repo", "links", "ticket:NEW", &wire(WireValue::Map(vec![])))
            .unwrap();
        m.write_graph_upsert_edge(
            "repo",
            "links",
            GraphEdgeWrite {
                id: "edge-new",
                src: "page:New",
                dst: "ticket:NEW",
                label: "refers_to",
                props: &wire(WireValue::Map(vec![])),
            },
        )
        .unwrap();

        let existing = m.read_substrate_refs("repo", "ticket:EXISTING").unwrap();
        assert!(!existing.degraded.is_degraded);
        assert_eq!(existing.inbound.len(), 1);
        assert_eq!(existing.inbound[0].source_id, "edge-existing");
        let new = m.read_substrate_refs("repo", "ticket:NEW").unwrap();
        assert!(!new.degraded.is_degraded);
        assert_eq!(new.inbound.len(), 1);
        assert_eq!(new.inbound[0].source_id, "edge-new");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn workspace_writes_require_global_admin_in_authenticated_mode() {
        let path = temp_path();
        fresh(&path);
        let mut loom = open_loom_unlocked(&path, None).unwrap();
        let root = WorkspaceId::v4_from_bytes([118u8; 16]);
        let mut identity = loom_core::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);
        let m = crate::LoomMcp::new(StoreAccess::persistent(loom));

        assert_eq!(
            m.write_workspace_create(Some("notes"), "document")
                .unwrap_err()
                .code,
            loom_core::error::Code::PermissionDenied
        );
        assert_eq!(
            m.write_workspace_rename("repo", "repo2").unwrap_err().code,
            loom_core::error::Code::PermissionDenied
        );
        assert_eq!(
            m.write_workspace_delete("repo").unwrap_err().code,
            loom_core::error::Code::PermissionDenied
        );

        m.store()
            .write(|loom| {
                loom.acl_store_mut().allow(
                    loom_core::AclSubject::Principal(root),
                    None,
                    None,
                    [loom_core::AclRight::Admin],
                )
            })
            .unwrap();
        m.write_workspace_rename("repo", "repo2").unwrap();
        assert!(m.read_workspace_get("repo2").unwrap().is_some());
        m.write_workspace_delete("repo2").unwrap();
        assert!(m.read_workspace_get("repo2").unwrap().is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn per_request_mcp_rejects_ephemeral_kv_maps() {
        let path = temp_path();
        fresh(&path);
        update_store(&path, |loom| {
            let ns = crate::reads::resolve_ns(loom, "cache-ns").unwrap();
            loom.configure_kv_map(ns, "sessions", KvMapConfig::EPHEMERAL)
                .unwrap();
        });
        let m = mcp(&path);
        let key = key_to_cbor(&loom_core::Value::Text("k".into()));
        let err = m
            .write_kv_put("cache-ns", "sessions", &key, b"v".to_vec())
            .unwrap_err();
        assert_eq!(err.code, loom_core::error::Code::Unsupported);
        let err = m.read_kv_get("cache-ns", "sessions", &key).unwrap_err();
        assert_eq!(err.code, loom_core::error::Code::Unsupported);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persistent_mcp_preserves_ephemeral_kv_maps() {
        let path = temp_path();
        fresh(&path);
        let mut loom = open_loom_unlocked(&path, None).unwrap();
        let ns = crate::reads::resolve_ns(&loom, "cache-ns").unwrap();
        loom.configure_kv_map(
            ns,
            "sessions",
            KvMapConfig {
                tier: KvTier::Ephemeral,
                ..KvMapConfig::EPHEMERAL
            },
        )
        .unwrap();
        let m = crate::LoomMcp::new(StoreAccess::persistent(loom));
        let key = key_to_cbor(&loom_core::Value::Text("k".into()));
        m.write_kv_put("cache-ns", "sessions", &key, b"v".to_vec())
            .unwrap();
        assert_eq!(
            m.read_kv_get("cache-ns", "sessions", &key).unwrap(),
            Some(b"v".to_vec())
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn calendar_create_and_put_entry() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let id = m
            .write_workspace_create(Some("cal"), "calendar")
            .expect("create");
        assert!(!id.is_empty());
        m.write_calendar_create_collection("cal", "alice", "work", "Work", "event,todo")
            .expect("create_collection");
        let entry =
            loom_core::calendar::CalendarEntry::event("uid-1", "Standup", "20260101T090000");
        let addr = m
            .write_calendar_put_entry("cal", "alice", "work", &entry.encode())
            .expect("put_entry");
        assert!(addr.contains(':'));
        let ics = concat!(
            "BEGIN:VCALENDAR\r\n",
            "VERSION:2.0\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:uid-2\r\n",
            "SUMMARY:Planning\r\n",
            "DTSTART:20260102T100000\r\n",
            "END:VEVENT\r\n",
            "END:VCALENDAR\r\n",
        );
        let imported = m
            .write_calendar_put_ics("cal", "alice", "work", ics)
            .expect("put_ics");
        assert!(imported.contains(':'));
        let exported = m
            .read_calendar_to_ics("cal", "alice", "work", "uid-2")
            .expect("to_ics")
            .expect("imported event");
        assert!(exported.contains("UID:uid-2"));
        assert!(exported.contains("SUMMARY:Planning"));
        assert!(
            m.write_calendar_delete_entry("cal", "alice", "work", "uid-1")
                .expect("del")
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn sql_exec_persists_and_commits() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        m.write_sql_exec(
            "appdb",
            "main",
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)",
        )
        .expect("create table");
        m.write_sql_exec("appdb", "main", "INSERT INTO t VALUES (1, 'a')")
            .expect("insert");
        // The staged table reads back via the SQL reader (read tool), proving persistence.
        let rt = m
            .read_sql_read_table("appdb", "main", "t")
            .expect("read_table");
        assert!(!rt.is_empty());
        let commit = m.write_sql_commit("appdb", "me", "c1", 0).expect("commit");
        assert!(commit.contains(':'));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn promoted_facets_cross_mcp_boundary_as_cbor() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);

        m.write_workspace_create(Some("graph-ns"), "graph")
            .expect("create graph workspace");
        let alice_props = wire(WireValue::Map(vec![(
            WireValue::Text("kind".to_string()),
            WireValue::Bytes(b"person".to_vec()),
        )]));
        let empty_props = wire(WireValue::Map(Vec::new()));
        m.write_graph_upsert_node("graph-ns", "people", "alice", &alice_props)
            .expect("graph upsert alice");
        m.write_graph_upsert_node("graph-ns", "people", "bob", &empty_props)
            .expect("graph upsert bob");
        m.write_graph_upsert_edge(
            "graph-ns",
            "people",
            super::GraphEdgeWrite {
                id: "e1",
                src: "alice",
                dst: "bob",
                label: "knows",
                props: &empty_props,
            },
        )
        .expect("graph upsert edge");
        assert_eq!(
            wire_back(
                &m.read_graph_get_node("graph-ns", "people", "alice")
                    .expect("graph get node")
                    .expect("node present")
            ),
            wire_back(&alice_props)
        );
        assert_eq!(
            wire_back(
                &m.read_graph_neighbors("graph-ns", "people", "alice")
                    .expect("graph neighbors")
            ),
            WireValue::Array(vec![WireValue::Text("bob".to_string())])
        );
        assert_eq!(
            wire_back(
                &m.read_graph_shortest_path("graph-ns", "people", "alice", "bob", None)
                    .expect("graph shortest path")
                    .expect("path present")
            ),
            WireValue::Array(vec![
                WireValue::Text("alice".to_string()),
                WireValue::Text("bob".to_string()),
            ])
        );

        m.write_workspace_create(Some("vector-ns"), "vector")
            .expect("create vector workspace");
        m.write_vector_create("vector-ns", "emb", 2, 1)
            .expect("vector create");
        let vector = vector_bytes(&[1.0, 0.0]);
        let metadata = wire(WireValue::Map(vec![(
            WireValue::Text("lang".to_string()),
            cell_text("en"),
        )]));
        m.write_vector_upsert_source(
            "vector-ns",
            "emb",
            super::VectorSourceWrite {
                id: "doc1",
                vector: &vector,
                metadata: &metadata,
                source_text: b"loom vector",
                model_id: Some("test-embedding"),
                weights_digest: Some("w1"),
            },
        )
        .expect("vector upsert source");
        assert_eq!(
            wire_back(
                &m.read_vector_get("vector-ns", "emb", "doc1")
                    .expect("vector get")
                    .expect("vector present")
            ),
            WireValue::Array(vec![WireValue::Bytes(vector.clone()), wire_back(&metadata)])
        );
        assert_eq!(
            m.read_vector_source_text("vector-ns", "emb", "doc1")
                .expect("vector source")
                .expect("source present"),
            b"loom vector".to_vec()
        );
        let WireValue::Array(model) = wire_back(
            &m.read_vector_embedding_model("vector-ns", "emb")
                .expect("vector model")
                .expect("model present"),
        ) else {
            panic!("vector model shape");
        };
        assert_eq!(model[1], WireValue::Text("test-embedding".to_string()));
        assert_eq!(
            wire_back(
                &m.read_vector_ids("vector-ns", "emb", None)
                    .expect("vector ids")
            ),
            WireValue::Array(vec![WireValue::Text("doc1".to_string())])
        );
        assert!(
            m.write_vector_create_metadata_index("vector-ns", "emb", "lang")
                .expect("vector create metadata index")
        );
        assert_eq!(
            wire_back(
                &m.read_vector_metadata_index_keys("vector-ns", "emb")
                    .expect("vector index keys")
            ),
            WireValue::Array(vec![WireValue::Text("lang".to_string())])
        );
        let filter = wire(WireValue::Array(vec![
            WireValue::Uint(1),
            WireValue::Text("lang".to_string()),
            cell_text("en"),
        ]));
        let hits = wire_back(
            &m.read_vector_search("vector-ns", "emb", &vector, 10, &filter)
                .expect("vector search"),
        );
        let WireValue::Array(hits) = hits else {
            panic!("vector hits shape");
        };
        let WireValue::Array(hit) = hits.first().cloned().expect("one vector hit") else {
            panic!("vector hit shape");
        };
        assert_eq!(hit.first(), Some(&WireValue::Text("doc1".to_string())));
        assert_eq!(
            wire_back(
                &m.read_vector_search_policy(crate::reads::VectorSearchPolicyRead {
                    workspace: "vector-ns",
                    name: "emb",
                    query: &vector,
                    k: 10,
                    filter: &filter,
                    policy: 0,
                    threshold: 0,
                    ef: 8,
                    pq_m: 1,
                    pq_k: 2,
                    pq_iters: 1,
                })
                .expect("vector search policy")
            ),
            WireValue::Array(vec![WireValue::Array(hit)])
        );
        let semantic = m
            .read_fts_search(FtsSearchReadRequest {
                workspace: "vector-ns",
                name: "emb",
                query: "",
                query_vector: Some(&[1.0, 0.0]),
                query_model_id: Some("test-embedding"),
                query_weights_digest: Some("w1"),
                field: None,
                limit: 10,
                offset: 0,
            })
            .expect("substrate semantic search");
        assert!(!semantic.reduced);
        assert!(!semantic.degraded.is_degraded);
        assert_eq!(semantic.index_status.semantic, "ready");
        assert_eq!(semantic.engine.rungs_available, vec!["semantic"]);
        assert_eq!(semantic.hits.len(), 1);
        assert_eq!(semantic.hits[0].facet, "vector");
        assert_eq!(semantic.hits[0].entity_id, "doc1");
        assert_eq!(semantic.hits[0].field, "source_text");
        assert_eq!(semantic.hits[0].match_via, "semantic");
        assert_eq!(semantic.hits[0].snippet, "loom vector");
        let stale = m
            .read_fts_search(FtsSearchReadRequest {
                workspace: "vector-ns",
                name: "emb",
                query: "",
                query_vector: Some(&[1.0, 0.0]),
                query_model_id: Some("other-embedding"),
                query_weights_digest: Some("w1"),
                field: None,
                limit: 10,
                offset: 0,
            })
            .expect("substrate semantic mismatch");
        assert!(stale.reduced);
        assert!(stale.degraded.is_degraded);
        assert_eq!(stale.degraded.reason, "semantic_model_mismatch");
        assert_eq!(stale.index_status.semantic, "stale");
        assert!(stale.hits.is_empty());

        m.write_workspace_create(Some("columnar-ns"), "columnar")
            .expect("create columnar workspace");
        let columns = wire(WireValue::Array(vec![
            WireValue::Array(vec![WireValue::Text("id".to_string()), WireValue::Uint(1)]),
            WireValue::Array(vec![
                WireValue::Text("name".to_string()),
                WireValue::Uint(3),
            ]),
        ]));
        m.write_columnar_create("columnar-ns", "users", &columns, 1024)
            .expect("columnar create");
        let row = wire(WireValue::Array(vec![cell_int(7), cell_text("alpha")]));
        m.write_columnar_append("columnar-ns", "users", &row)
            .expect("columnar append");
        assert_eq!(
            m.read_columnar_rows("columnar-ns", "users")
                .expect("columnar rows"),
            1
        );
        let inspect = wire_back(
            &m.read_columnar_inspect("columnar-ns", "users")
                .expect("columnar inspect"),
        );
        let WireValue::Array(inspect_items) = inspect else {
            panic!("columnar inspect must be an array");
        };
        assert_eq!(inspect_items[1], WireValue::Uint(1));
        assert!(
            m.read_columnar_source_digest("columnar-ns", "users")
                .expect("columnar source digest")
                .starts_with("blake3:")
        );
        m.write_columnar_compact("columnar-ns", "users")
            .expect("columnar compact");
        assert_eq!(
            wire_back(
                &m.read_columnar_scan("columnar-ns", "users")
                    .expect("columnar scan")
            ),
            WireValue::Array(vec![WireValue::Array(vec![
                cell_int(7),
                cell_text("alpha")
            ])])
        );
        let select_columns = wire(WireValue::Array(vec![WireValue::Text("name".to_string())]));
        let filter = wire(WireValue::Array(vec![
            WireValue::Text("id".to_string()),
            WireValue::Uint(0),
            cell_int(7),
        ]));
        assert_eq!(
            wire_back(
                &m.read_columnar_select("columnar-ns", "users", &select_columns, &filter)
                    .expect("columnar select")
            ),
            WireValue::Array(vec![WireValue::Array(vec![cell_text("alpha")])])
        );
        let aggregates = wire(WireValue::Array(vec![
            WireValue::Array(vec![WireValue::Uint(0), WireValue::Null]),
            WireValue::Array(vec![WireValue::Uint(4), WireValue::Text("id".to_string())]),
        ]));
        assert_eq!(
            wire_back(
                &m.read_columnar_aggregate("columnar-ns", "users", &aggregates, &[])
                    .expect("columnar aggregate")
            ),
            WireValue::Array(vec![cell_u64(1), cell_int(7)])
        );

        m.write_workspace_create(Some("search-ns"), "search")
            .expect("create search workspace");
        let mapping = wire(WireValue::Map(vec![(
            WireValue::Text("body".to_string()),
            WireValue::Array(vec![
                WireValue::Uint(0),
                WireValue::Bool(true),
                WireValue::Bool(false),
            ]),
        )]));
        let doc = wire(WireValue::Map(vec![(
            WireValue::Text("body".to_string()),
            WireValue::Text("loom search".to_string()),
        )]));
        m.write_fts_create("search-ns", "docs", &mapping)
            .expect("search create");
        m.write_fts_index("search-ns", "docs", b"doc1".to_vec(), &doc)
            .expect("search index");
        assert_eq!(
            wire_back(
                &m.read_fts_ids("search-ns", "docs", None)
                    .expect("search ids")
            ),
            WireValue::Array(vec![WireValue::Bytes(b"doc1".to_vec())])
        );
        assert!(
            m.read_fts_source_digest("search-ns", "docs")
                .expect("search source digest")
                .contains(':')
        );
        assert_eq!(
            wire_back(
                &m.read_fts_get("search-ns", "docs", b"doc1")
                    .expect("search get")
                    .expect("doc present")
            ),
            wire_back(&doc)
        );
        let request = wire(WireValue::Array(vec![
            WireValue::Array(vec![
                WireValue::Uint(0),
                WireValue::Text("body".to_string()),
                WireValue::Text("loom".to_string()),
            ]),
            WireValue::Uint(10),
            WireValue::Uint(0),
        ]));
        let WireValue::Array(mut response) = wire_back(
            &m.read_fts_query("search-ns", "docs", &request)
                .expect("search query"),
        ) else {
            panic!("search response shape");
        };
        assert_eq!(response.remove(0), WireValue::Bool(true));
        let WireValue::Array(hits) = response.remove(0) else {
            panic!("search hits shape");
        };
        let WireValue::Array(hit) = hits.first().cloned().expect("one hit") else {
            panic!("search hit shape");
        };
        assert_eq!(hit.first(), Some(&WireValue::Bytes(b"doc1".to_vec())));
        let substrate = m
            .read_fts_search(FtsSearchReadRequest {
                workspace: "search-ns",
                name: "docs",
                query: "loom",
                query_vector: None,
                query_model_id: None,
                query_weights_digest: None,
                field: None,
                limit: 10,
                offset: 0,
            })
            .expect("substrate search");
        assert!(substrate.reduced);
        assert!(substrate.degraded.is_degraded);
        assert_eq!(substrate.degraded.reason, "scan_backed_lexical");
        assert_eq!(substrate.engine.rungs_available, vec!["lexical"]);
        assert_eq!(substrate.index_status.lexical, "ready");
        assert_eq!(substrate.hits.len(), 1);
        assert_eq!(substrate.hits[0].facet, "search");
        assert_eq!(substrate.hits[0].collection, "docs");
        assert_eq!(substrate.hits[0].field, "body");
        assert_eq!(substrate.hits[0].entity_id, "646f6331");
        assert!(substrate.hits[0].snippet.contains("loom"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ticket_create_receipt_wraps_resource_and_change_metadata() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        let project = m
            .write_tickets_project_create("repo", "studio", "eng", "ENG", "Engineering", None)
            .expect("project");
        let envelope = m
            .write_tickets_create_receipt(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "eng",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &json!({"title": "Receipt"}),
                    policy_labels: &[],
                    expected_root: Some(&project.profile_root),
                },
            )
            .expect("ticket receipt");
        assert_eq!(envelope.resource.primary_key, "ENG-1");
        assert_eq!(envelope.receipt.operation, "ticket.created");
        assert_eq!(envelope.receipt.resource_kind, "ticket");
        assert_eq!(envelope.receipt.resource_id, "ENG-1");
        assert_eq!(envelope.receipt.root_before, Some(project.profile_root));
        assert_eq!(
            envelope.receipt.root_after,
            Some(envelope.resource.profile_root.clone())
        );
        assert!(matches!(
            envelope.receipt.changes.as_slice(),
            [loom_types::MutationChange::ResourceCreated]
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lane_ticket_add_receipt_wraps_public_coordination_change() {
        let path = temp_path();
        fresh(&path);
        let m = mcp(&path);
        m.write_lanes_create(
            "repo",
            LaneCreateRequest {
                lane_id: "agent-7",
                lane_key: "agent-7",
                title: "Agent 7",
                description: "",
                lane_kind: loom_lanes::LaneKind::Assignment.as_str(),
                owner_principal: Some("agent:7"),
                lane_status: "ready",
                lane_tickets: &[],
                active_ticket_id: None,
                status_report: "",
                reviewer_feedback: "",
                updated_by: Some("agent:7"),
            },
        )
        .expect("lane");
        let envelope = m
            .write_lanes_ticket_add_receipt(
                "repo",
                LaneTicketUpdateRequest {
                    lane_id: "agent-7",
                    ticket_id: "MX-700",
                    placement: LaneTicketPlacement::Append,
                    updated_by: Some("agent:7"),
                },
            )
            .expect("lane receipt");
        assert_eq!(envelope.resource.lane_id, "agent-7");
        assert_eq!(envelope.receipt.operation, "lane.ticket_added");
        assert_eq!(envelope.receipt.resource_kind, "lane");
        assert_eq!(envelope.receipt.resource_id, "agent-7");
        assert!(matches!(
            envelope.receipt.changes.as_slice(),
            [loom_types::MutationChange::FieldSet { field, after }]
                if field == "ticket_id" && after == "MX-700"
        ));
        let _ = std::fs::remove_file(&path);
    }
}
