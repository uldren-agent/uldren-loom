use std::collections::BTreeMap;

use loom_core::error::{Code, LoomError, Result};
use loom_core::mutable_overlay::OverlayKey;
use loom_core::provider::{ObjectStore, PlanningObjectStore};
use loom_core::workspace::{FacetKind, WorkspaceId};
use loom_core::{
    AclDomain, AclRight, AtomicityBoundary, AuditIntent, CompareToken, Digest, EnginePathSelector,
    EnginePlanningScope, EngineStateDelta, FacetSideEffects, FacetWrite, FacetWriteOp, GraphValue,
    Loom, OverlayDurabilityPolicy, Props, SecondaryIndexWrite, SecondaryIndexWriteOp,
    WorkflowPlanningSnapshot, WorkflowReferenceUpdate, WorkflowTransaction, graph_remove_edge,
    graph_upsert_edge, graph_upsert_node,
};
use loom_store::FileStore;
use loom_substrate::body::{
    Block, BlockKind, BlockRefResolution, BlockRefTarget, Body, BodyRender, BodyRenderIssue,
    BodyRenderIssueKind,
};
use loom_substrate::pages::{
    APP_ID, PageConflict, PageDraft, PageOperationLog, PageOperationRecord, PageRevision,
    PageSpace, PageStructure, PageWorkspace, PageWorkspaceSnapshot, PublishOutcome, StructureEdge,
    StructureNode, WorkspacePage, page_profile_operation_log_key, page_workspace_snapshot_key,
};
use loom_substrate::versioning::{BodyRef, Checkpoint, EntityRevision, RevisionIndexAppend};
use loom_substrate::{ActorKind, OperationEnvelope, OperationEnvelopeInput};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use loom_substrate::refs::{EntityRef, ReferenceEdge, ReferenceIndex, ReferenceSource};
use loom_substrate::versioning::{
    current_revision_index_append_writes, load_current_revision_index, load_latest_entity_revision,
    persist_current_revision_index,
};

const PAGE_CONTROL_OVERLAY_SCHEMA: &str = "loom.pages.control-overlay.v1";

struct PreparedPageReferences {
    owner_state: loom_core::WorkflowOwnerState,
    live_state: Option<EngineStateDelta>,
    snapshot: WorkflowPlanningSnapshot,
}

struct PlannedPagePublish {
    summary: PagePublishSummary,
    state: Vec<u8>,
    transaction: WorkflowTransaction,
}

struct OperationRecordRequest<'a> {
    workspace_id: &'a str,
    scope_id: &'a str,
    operation_kind: &'a str,
    target_entity_id: Option<&'a str>,
    base_snapshot: &'a [u8],
    root_after: Digest,
    payload: &'a [u8],
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceSummary {
    pub workspace_id: String,
    pub space_id: String,
    pub title: String,
    pub archived: bool,
    pub profile_root: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageSummary {
    pub workspace_id: String,
    pub page_id: String,
    pub space_id: String,
    pub parent_page_id: Option<String>,
    pub title: String,
    pub current_revision: Option<u64>,
    pub deleted: bool,
    pub status: String,
    pub body: Option<Vec<u8>>,
    pub draft_body: Option<Vec<u8>>,
    pub body_text: Option<String>,
    pub draft_body_text: Option<String>,
    pub rendered_body: Option<String>,
    pub draft_rendered_body: Option<String>,
    pub render_issues: Vec<PageRenderIssueSummary>,
    pub draft_render_issues: Vec<PageRenderIssueSummary>,
    pub profile_root: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRenderIssueSummary {
    pub kind: String,
    pub entity_id: String,
    pub block_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageUpdateSummary {
    pub workspace_id: String,
    pub page_id: String,
    pub status: String,
    pub base_revision: Option<u64>,
    pub updated_at_ms: u64,
    pub profile_root: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagePublishSummary {
    pub workspace_id: String,
    pub page_id: String,
    pub outcome: String,
    pub revision: Option<u64>,
    pub conflict_id: Option<String>,
    pub current_revision: Option<u64>,
    pub body_digest: Option<String>,
    pub profile_root: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PageHistoryEntry {
    pub kind: String,
    pub page_id: String,
    pub revision: Option<u64>,
    pub body_digest: Option<String>,
    pub author: Option<String>,
    pub published_at_ms: Option<u64>,
    pub conflict_id: Option<String>,
    pub base_revision: Option<u64>,
    pub current_revision: Option<u64>,
    pub candidate_digest: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageCreateRequest<'a> {
    pub workspace_id: &'a str,
    pub page_id: &'a str,
    pub space_id: &'a str,
    pub parent_page_id: Option<&'a str>,
    pub title: &'a str,
    pub expected_root: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructureCreateRequest<'a> {
    pub workspace_id: &'a str,
    pub structure_id: &'a str,
    pub space_id: &'a str,
    pub kind: &'a str,
    pub title: &'a str,
    pub expected_root: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StructureSummary {
    pub workspace_id: String,
    pub structure_id: String,
    pub space_id: String,
    pub kind: String,
    pub title: String,
    pub root_node_id: Option<String>,
    pub field_ids: Vec<String>,
    pub profile_root: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StructureNodeSummary {
    pub workspace_id: String,
    pub structure_id: String,
    pub node_id: String,
    pub kind: String,
    pub label: String,
    pub body_digest: Option<String>,
    pub entity_ref: Option<String>,
    pub profile_root: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StructureEdgeSummary {
    pub workspace_id: String,
    pub structure_id: String,
    pub edge_id: String,
    pub src_node_id: String,
    pub dst_node_id: String,
    pub label: String,
    pub target_ref: Option<String>,
    pub profile_root: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StructureRenderSummary {
    pub structure: StructureSummary,
    pub nodes: Vec<StructureNodeSummary>,
    pub edges: Vec<StructureEdgeSummary>,
    pub graph_collection: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StructureMoveSummary {
    pub workspace_id: String,
    pub structure_id: String,
    pub node_id: String,
    pub parent_node_id: Option<String>,
    pub label: String,
    pub edge: Option<StructureEdgeSummary>,
    pub graph_collection: String,
    pub profile_root: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StructureDecomposeSummary {
    pub workspace_id: String,
    pub structure_id: String,
    pub tickets: Vec<loom_tickets::TicketSummary>,
    pub implemented_by_edges: Vec<String>,
    pub graph_collection: String,
}

pub fn create_space(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    space_id: &str,
    title: &str,
    expected_root: Option<&str>,
) -> Result<SpaceSummary> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Write)?;
    let mut organization = load_workspace(loom.store(), workspace_id)?;
    enforce_expected_root(loom, workspace_id, &organization, expected_root)?;
    let base_bytes = workspace_snapshot_bytes(workspace_id, &organization)?;
    let space = organization.create_space(space_id, title)?.clone();
    let root_after = profile_snapshot_digest(loom, workspace_id, &organization)?;
    let payload = space.encode()?;
    let record = operation_record(
        loom,
        workspace,
        OperationRecordRequest {
            workspace_id,
            scope_id: space_id,
            operation_kind: "space.created",
            target_entity_id: Some(space_id),
            base_snapshot: &base_bytes,
            root_after,
            payload: &payload,
        },
    )?;
    save_workspace_and_append_operation(
        loom,
        workspace,
        workspace_id,
        &organization,
        &record,
        None,
        loom_core::WorkflowOwnerState::default(),
        None,
    )?;
    Ok(space_summary(workspace_id, &space, root_after))
}

pub fn list_spaces(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<SpaceSummary>> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Read)?;
    let organization = load_workspace(loom.store(), workspace_id)?;
    let profile_root = profile_snapshot_digest(loom, workspace_id, &organization)?;
    Ok(organization
        .spaces
        .values()
        .map(|space| space_summary(workspace_id, space, profile_root))
        .collect())
}

pub fn get_space(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    space_id: &str,
) -> Result<Option<SpaceSummary>> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Read)?;
    let organization = load_workspace(loom.store(), workspace_id)?;
    let profile_root = profile_snapshot_digest(loom, workspace_id, &organization)?;
    Ok(organization
        .spaces
        .get(space_id)
        .map(|space| space_summary(workspace_id, space, profile_root)))
}

pub fn create_page(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: PageCreateRequest<'_>,
) -> Result<PageSummary> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Write)?;
    let mut organization = load_workspace(loom.store(), request.workspace_id)?;
    enforce_expected_root(
        loom,
        request.workspace_id,
        &organization,
        request.expected_root,
    )?;
    let base_bytes = workspace_snapshot_bytes(request.workspace_id, &organization)?;
    organization.create_page(
        request.page_id,
        request.space_id,
        request.parent_page_id.map(str::to_string),
        request.title,
    )?;
    let page = page_by_id(&organization, request.page_id)?.clone();
    let root_after = profile_snapshot_digest(loom, request.workspace_id, &organization)?;
    let payload = page.encode()?;
    let record = operation_record(
        loom,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.space_id,
            operation_kind: "page.created",
            target_entity_id: Some(request.page_id),
            base_snapshot: &base_bytes,
            root_after,
            payload: &payload,
        },
    )?;
    save_workspace_and_append_operation(
        loom,
        workspace,
        request.workspace_id,
        &organization,
        &record,
        None,
        loom_core::WorkflowOwnerState::default(),
        None,
    )?;
    Ok(page_summary(
        request.workspace_id,
        &organization,
        &page,
        workspace,
        root_after,
    ))
}

pub fn update_page(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    page_id: &str,
    body: Vec<u8>,
    updated_at_ms: u64,
    expected_root: Option<&str>,
) -> Result<PageUpdateSummary> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Write)?;
    let principal = loom.effective_principal()?.unwrap_or(workspace);
    let mut organization = load_workspace(loom.store(), workspace_id)?;
    enforce_expected_root(loom, workspace_id, &organization, expected_root)?;
    let base_bytes = workspace_snapshot_bytes(workspace_id, &organization)?;
    let draft_id = page_draft_id(page_id, principal);
    let draft = if organization.drafts.contains_key(&draft_id) {
        organization.update_draft(&draft_id, body, updated_at_ms)?
    } else {
        organization.create_draft(&draft_id, page_id, principal, body, updated_at_ms)?
    }
    .clone();
    let root_after = profile_snapshot_digest(loom, workspace_id, &organization)?;
    let payload = draft.encode()?;
    let record = operation_record(
        loom,
        workspace,
        OperationRecordRequest {
            workspace_id,
            scope_id: page_id,
            operation_kind: "page.updated",
            target_entity_id: Some(page_id),
            base_snapshot: &base_bytes,
            root_after,
            payload: &payload,
        },
    )?;
    save_workspace_and_append_operation(
        loom,
        workspace,
        workspace_id,
        &organization,
        &record,
        None,
        loom_core::WorkflowOwnerState::default(),
        None,
    )?;
    Ok(PageUpdateSummary {
        workspace_id: workspace_id.to_string(),
        page_id: page_id.to_string(),
        status: "draft".to_string(),
        base_revision: draft.base_revision,
        updated_at_ms: draft.updated_at_ms,
        profile_root: root_after.to_string(),
    })
}

pub fn update_page_text(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    page_id: &str,
    body_text: &str,
    updated_at_ms: u64,
    expected_root: Option<&str>,
) -> Result<PageUpdateSummary> {
    update_page(
        loom,
        workspace,
        workspace_id,
        page_id,
        Body::from_plain_text(body_text)?.encode()?,
        updated_at_ms,
        expected_root,
    )
}

pub fn publish_page(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    page_id: &str,
    published_at_ms: u64,
    expected_root: Option<&str>,
) -> Result<PagePublishSummary> {
    publish_page_with_before_commit(
        loom,
        workspace,
        workspace_id,
        page_id,
        published_at_ms,
        expected_root,
        |_| Ok(()),
    )
}

fn publish_page_with_before_commit(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    page_id: &str,
    published_at_ms: u64,
    expected_root: Option<&str>,
    before_commit: impl FnOnce(&mut Loom<FileStore>) -> Result<()>,
) -> Result<PagePublishSummary> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Write)?;
    let mut candidate = loom.fork_state_into(PlanningObjectStore::new(loom.store()))?;
    let planned = plan_page_publish(
        &mut candidate,
        workspace,
        workspace_id,
        page_id,
        published_at_ms,
        expected_root,
    )?;
    let summary = planned.summary.clone();
    drop(candidate);
    before_commit(loom)?;
    let receipt = loom
        .store()
        .commit_workflow_transaction(planned.transaction)?;
    loom.import_state(&planned.state)?;
    synchronize_page_transaction_receipt(loom, &receipt)?;
    Ok(summary)
}

fn plan_page_publish(
    loom: &mut Loom<PlanningObjectStore<'_, FileStore>>,
    workspace: WorkspaceId,
    workspace_id: &str,
    page_id: &str,
    published_at_ms: u64,
    expected_root: Option<&str>,
) -> Result<PlannedPagePublish> {
    let principal = loom.effective_principal()?.unwrap_or(workspace);
    let mut organization = load_workspace(loom.store(), workspace_id)?;
    enforce_expected_root(loom, workspace_id, &organization, expected_root)?;
    let base_bytes = workspace_snapshot_bytes(workspace_id, &organization)?;
    let draft_id = page_draft_id(page_id, principal);
    let outcome =
        organization.publish_draft(loom.store().digest_algo(), &draft_id, published_at_ms)?;
    let profile_root = profile_snapshot_digest(loom, workspace_id, &organization)?;
    let mut published_revision = None;
    let (summary, operation_kind, payload) = match outcome {
        PublishOutcome::Published(revision) => {
            published_revision = Some(revision.clone());
            let payload = revision.encode()?;
            (
                PagePublishSummary {
                    workspace_id: workspace_id.to_string(),
                    page_id: page_id.to_string(),
                    outcome: "published".to_string(),
                    revision: Some(revision.revision),
                    conflict_id: None,
                    current_revision: Some(revision.revision),
                    body_digest: Some(revision.body_digest.to_string()),
                    profile_root: profile_root.to_string(),
                },
                "page.published",
                payload,
            )
        }
        PublishOutcome::ConflictRecorded(conflict) => {
            let payload = conflict.encode()?;
            (
                PagePublishSummary {
                    workspace_id: workspace_id.to_string(),
                    page_id: page_id.to_string(),
                    outcome: "conflict".to_string(),
                    revision: None,
                    conflict_id: Some(conflict.conflict_id),
                    current_revision: conflict.current_revision,
                    body_digest: Some(conflict.candidate_digest.to_string()),
                    profile_root: profile_root.to_string(),
                },
                "page.publish_conflict",
                payload,
            )
        }
    };
    let record = operation_record(
        loom,
        workspace,
        OperationRecordRequest {
            workspace_id,
            scope_id: page_id,
            operation_kind,
            target_entity_id: Some(page_id),
            base_snapshot: &base_bytes,
            root_after: profile_root,
            payload: &payload,
        },
    )?;
    let prepared_references = published_revision
        .as_ref()
        .map(|revision| {
            prepare_page_published_refs(loom, workspace, workspace_id, page_id, &revision.body)
        })
        .transpose()?;
    let owner_state = prepared_references
        .as_ref()
        .map(|prepared| prepared.owner_state.clone())
        .unwrap_or_default();
    let planning_snapshot = prepared_references
        .as_ref()
        .map(|prepared| &prepared.snapshot);
    save_workspace_and_append_operation(
        loom,
        workspace,
        workspace_id,
        &organization,
        &record,
        published_revision.as_ref(),
        owner_state,
        planning_snapshot,
    )?;
    if let Some(prepared) = prepared_references
        && let Some(live_state) = prepared.live_state
    {
        loom.apply_engine_state_delta(live_state)?;
    }
    let (root, state_objects) = loom.save_state_objects()?;
    let mut objects = loom.store().objects()?;
    objects.extend(state_objects);
    let state = loom.export_state();
    let direct_controls = loom.store().direct_control_writes()?;
    if !direct_controls.is_empty() {
        return Err(LoomError::corrupt(
            "page publish planning candidate left direct control writes",
        ));
    }
    let mut workflow_transactions = loom.store().workflow_transactions()?;
    if workflow_transactions.len() != 1 {
        return Err(LoomError::corrupt(
            "page publish planning candidate must publish exactly one workflow transaction",
        ));
    }
    let mut transaction = workflow_transactions.remove(0);
    transaction.owner_state.objects.extend(objects);
    transaction.owner_state.reference = WorkflowReferenceUpdate::Set(Some(root));
    Ok(PlannedPagePublish {
        summary,
        state,
        transaction,
    })
}

pub fn get_page(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    page_id: &str,
) -> Result<Option<PageSummary>> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Read)?;
    let principal = loom.effective_principal()?.unwrap_or(workspace);
    let organization = load_workspace(loom.store(), workspace_id)?;
    let profile_root = profile_snapshot_digest(loom, workspace_id, &organization)?;
    Ok(organization
        .pages
        .get(page_id)
        .map(|page| page_summary(workspace_id, &organization, page, principal, profile_root)))
}

pub fn list_pages(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<PageSummary>> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Read)?;
    let principal = loom.effective_principal()?.unwrap_or(workspace);
    let organization = load_workspace(loom.store(), workspace_id)?;
    let profile_root = profile_snapshot_digest(loom, workspace_id, &organization)?;
    Ok(organization
        .pages
        .values()
        .map(|page| page_summary(workspace_id, &organization, page, principal, profile_root))
        .collect())
}

pub fn page_history(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    page_id: &str,
) -> Result<Vec<PageHistoryEntry>> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Read)?;
    let organization = load_workspace(loom.store(), workspace_id)?;
    if !organization.pages.contains_key(page_id) {
        return Err(LoomError::not_found("page not found"));
    }
    let mut out = organization
        .revisions
        .values()
        .filter(|revision| revision.page_id == page_id)
        .map(revision_history_entry)
        .collect::<Vec<_>>();
    out.extend(
        organization
            .conflicts
            .values()
            .filter(|conflict| conflict.page_id == page_id)
            .map(conflict_history_entry),
    );
    Ok(out)
}

pub fn create_structure(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: StructureCreateRequest<'_>,
) -> Result<StructureRenderSummary> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Write)?;
    let mut organization = load_workspace(loom.store(), request.workspace_id)?;
    enforce_expected_root(
        loom,
        request.workspace_id,
        &organization,
        request.expected_root,
    )?;
    let base_bytes = workspace_snapshot_bytes(request.workspace_id, &organization)?;
    let structure = organization
        .create_structure(
            request.structure_id,
            request.space_id,
            request.kind,
            request.title,
        )?
        .clone();
    let payload = structure.encode()?;
    let profile_root = profile_snapshot_digest(loom, request.workspace_id, &organization)?;
    record_organization_operation(
        loom,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.structure_id,
            operation_kind: "structure.created",
            target_entity_id: Some(request.structure_id),
            base_snapshot: &base_bytes,
            root_after: profile_root,
            payload: &payload,
        },
    )?;
    save_workspace(loom.store(), workspace, request.workspace_id, &organization)?;
    structure_render_summary(
        request.workspace_id,
        &organization,
        request.structure_id,
        profile_root,
    )
}

pub fn add_structure_node(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: StructureNodeRequest<'_>,
) -> Result<StructureNodeSummary> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Write)?;
    let body_digest = request.body_digest.map(Digest::parse).transpose()?;
    let mut organization = load_workspace(loom.store(), request.workspace_id)?;
    enforce_expected_root(
        loom,
        request.workspace_id,
        &organization,
        request.expected_root,
    )?;
    let base_bytes = workspace_snapshot_bytes(request.workspace_id, &organization)?;
    let node = organization
        .add_structure_node(
            request.node_id,
            request.structure_id,
            request.kind,
            request.label,
            body_digest,
            request.entity_ref,
        )?
        .clone();
    graph_upsert_node(
        loom,
        workspace,
        &structure_graph_collection(request.workspace_id, request.structure_id),
        request.node_id,
        node_props(&node),
    )?;
    let profile_root = profile_snapshot_digest(loom, request.workspace_id, &organization)?;
    let payload = node.encode()?;
    record_organization_operation(
        loom,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.structure_id,
            operation_kind: "structure.node_added",
            target_entity_id: Some(request.node_id),
            base_snapshot: &base_bytes,
            root_after: profile_root,
            payload: &payload,
        },
    )?;
    save_workspace(loom.store(), workspace, request.workspace_id, &organization)?;
    Ok(node_summary(request.workspace_id, &node, profile_root))
}

pub fn update_structure_node(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: StructureNodeRequest<'_>,
) -> Result<StructureNodeSummary> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Write)?;
    let body_digest = request.body_digest.map(Digest::parse).transpose()?;
    let mut organization = load_workspace(loom.store(), request.workspace_id)?;
    enforce_expected_root(
        loom,
        request.workspace_id,
        &organization,
        request.expected_root,
    )?;
    let base_bytes = workspace_snapshot_bytes(request.workspace_id, &organization)?;
    let node = organization
        .update_structure_node(
            request.structure_id,
            request.node_id,
            request.kind,
            request.label,
            body_digest,
            request.entity_ref,
        )?
        .clone();
    graph_upsert_node(
        loom,
        workspace,
        &structure_graph_collection(request.workspace_id, request.structure_id),
        request.node_id,
        node_props(&node),
    )?;
    let profile_root = profile_snapshot_digest(loom, request.workspace_id, &organization)?;
    let payload = node.encode()?;
    record_organization_operation(
        loom,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.structure_id,
            operation_kind: "structure.node_updated",
            target_entity_id: Some(request.node_id),
            base_snapshot: &base_bytes,
            root_after: profile_root,
            payload: &payload,
        },
    )?;
    save_workspace(loom.store(), workspace, request.workspace_id, &organization)?;
    Ok(node_summary(request.workspace_id, &node, profile_root))
}

pub fn bind_structure_node(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: StructureBindRequest<'_>,
) -> Result<StructureNodeSummary> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Write)?;
    let mut organization = load_workspace(loom.store(), request.workspace_id)?;
    enforce_expected_root(
        loom,
        request.workspace_id,
        &organization,
        request.expected_root,
    )?;
    let base_bytes = workspace_snapshot_bytes(request.workspace_id, &organization)?;
    let node = organization
        .bind_structure_node(request.structure_id, request.node_id, request.entity_ref)
        .cloned()?;
    graph_upsert_node(
        loom,
        workspace,
        &structure_graph_collection(request.workspace_id, request.structure_id),
        request.node_id,
        node_props(&node),
    )?;
    let profile_root = profile_snapshot_digest(loom, request.workspace_id, &organization)?;
    let payload = node.encode()?;
    record_organization_operation(
        loom,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.structure_id,
            operation_kind: "structure.node_bound",
            target_entity_id: Some(request.node_id),
            base_snapshot: &base_bytes,
            root_after: profile_root,
            payload: &payload,
        },
    )?;
    save_workspace(loom.store(), workspace, request.workspace_id, &organization)?;
    Ok(node_summary(request.workspace_id, &node, profile_root))
}

pub fn link_structure_node(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: StructureLinkRequest<'_>,
) -> Result<StructureEdgeSummary> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Write)?;
    let mut organization = load_workspace(loom.store(), request.workspace_id)?;
    enforce_expected_root(
        loom,
        request.workspace_id,
        &organization,
        request.expected_root,
    )?;
    let base_bytes = workspace_snapshot_bytes(request.workspace_id, &organization)?;
    let edge = organization
        .link_structure_node(
            request.edge_id,
            request.structure_id,
            request.src_node_id,
            request.dst_node_id,
            request.label,
            request.target_ref,
        )?
        .clone();
    graph_upsert_edge(
        loom,
        workspace,
        &structure_graph_collection(request.workspace_id, request.structure_id),
        request.edge_id,
        request.src_node_id,
        request.dst_node_id,
        request.label,
        edge_props(&edge),
    )?;
    let profile_root = profile_snapshot_digest(loom, request.workspace_id, &organization)?;
    let payload = edge.encode()?;
    record_organization_operation(
        loom,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.structure_id,
            operation_kind: "structure.node_linked",
            target_entity_id: Some(request.edge_id),
            base_snapshot: &base_bytes,
            root_after: profile_root,
            payload: &payload,
        },
    )?;
    save_workspace(loom.store(), workspace, request.workspace_id, &organization)?;
    Ok(edge_summary(request.workspace_id, &edge, profile_root))
}

pub struct StructureLinkRequest<'a> {
    pub workspace_id: &'a str,
    pub structure_id: &'a str,
    pub edge_id: &'a str,
    pub src_node_id: &'a str,
    pub dst_node_id: &'a str,
    pub label: &'a str,
    pub target_ref: Option<String>,
    pub expected_root: Option<&'a str>,
}

pub struct StructureNodeRequest<'a> {
    pub workspace_id: &'a str,
    pub structure_id: &'a str,
    pub node_id: &'a str,
    pub kind: &'a str,
    pub label: &'a str,
    pub body_digest: Option<&'a str>,
    pub entity_ref: Option<String>,
    pub expected_root: Option<&'a str>,
}

pub struct StructureBindRequest<'a> {
    pub workspace_id: &'a str,
    pub structure_id: &'a str,
    pub node_id: &'a str,
    pub entity_ref: Option<String>,
    pub expected_root: Option<&'a str>,
}

pub struct StructureMoveRequest<'a> {
    pub workspace_id: &'a str,
    pub structure_id: &'a str,
    pub node_id: &'a str,
    pub parent_node_id: Option<&'a str>,
    pub label: Option<&'a str>,
    pub expected_root: Option<&'a str>,
}

pub struct StructureDecomposeItem<'a> {
    pub node_id: &'a str,
    pub project_id: &'a str,
    pub ticket_type: Option<&'a str>,
    pub fields: Option<&'a Value>,
    pub policy_labels: &'a [String],
}

pub struct StructureDecomposeRequest<'a> {
    pub workspace_id: &'a str,
    pub structure_id: &'a str,
    pub items: &'a [StructureDecomposeItem<'a>],
}

pub fn move_structure_node(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: StructureMoveRequest<'_>,
) -> Result<StructureMoveSummary> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Write)?;
    let label = request.label.unwrap_or("child_of");
    let mut organization = load_workspace(loom.store(), request.workspace_id)?;
    enforce_expected_root(
        loom,
        request.workspace_id,
        &organization,
        request.expected_root,
    )?;
    let base_bytes = workspace_snapshot_bytes(request.workspace_id, &organization)?;
    let old_parent_edges = organization
        .structure_edges
        .values()
        .filter(|edge| {
            edge.structure_id == request.structure_id
                && edge.dst_node_id == request.node_id
                && edge.label == label
        })
        .map(|edge| edge.edge_id.clone())
        .collect::<Vec<_>>();
    let edge = organization.move_structure_node(
        request.structure_id,
        request.node_id,
        request.parent_node_id,
        label,
    )?;
    let graph_collection = structure_graph_collection(request.workspace_id, request.structure_id);
    for edge_id in old_parent_edges {
        graph_remove_edge(loom, workspace, &graph_collection, &edge_id)?;
    }
    if let Some(edge) = &edge {
        graph_upsert_edge(
            loom,
            workspace,
            &graph_collection,
            &edge.edge_id,
            &edge.src_node_id,
            &edge.dst_node_id,
            &edge.label,
            edge_props(edge),
        )?;
    }
    let profile_root = profile_snapshot_digest(loom, request.workspace_id, &organization)?;
    let moved_node = organization
        .structure_nodes
        .get(request.node_id)
        .ok_or_else(|| LoomError::not_found("structure node not found"))?
        .clone();
    let payload = moved_node.encode()?;
    record_organization_operation(
        loom,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.structure_id,
            operation_kind: "structure.node_moved",
            target_entity_id: Some(request.node_id),
            base_snapshot: &base_bytes,
            root_after: profile_root,
            payload: &payload,
        },
    )?;
    save_workspace(loom.store(), workspace, request.workspace_id, &organization)?;
    Ok(StructureMoveSummary {
        workspace_id: request.workspace_id.to_string(),
        structure_id: request.structure_id.to_string(),
        node_id: request.node_id.to_string(),
        parent_node_id: request.parent_node_id.map(str::to_string),
        label: label.to_string(),
        edge: edge
            .as_ref()
            .map(|edge| edge_summary(request.workspace_id, edge, profile_root)),
        graph_collection,
        profile_root: profile_root.to_string(),
    })
}

pub fn decompose_to_tickets(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: StructureDecomposeRequest<'_>,
) -> Result<StructureDecomposeSummary> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Write)?;
    let organization = load_workspace(loom.store(), request.workspace_id)?;
    if !organization.structures.contains_key(request.structure_id) {
        return Err(LoomError::not_found("structure not found"));
    }
    let graph_collection = structure_graph_collection(request.workspace_id, request.structure_id);
    let mut tickets = Vec::new();
    let mut implemented_by_edges = Vec::new();
    for item in request.items {
        let node = organization
            .structure_nodes
            .get(item.node_id)
            .ok_or_else(|| LoomError::not_found("structure node not found"))?;
        if node.structure_id != request.structure_id {
            return Err(LoomError::not_found("structure node not found"));
        }
        let external_id = format!("{}:{}", request.structure_id, item.node_id);
        let fields = decompose_fields(node, item.fields)?;
        ensure_decompose_ticket_fields(
            loom,
            workspace,
            request.workspace_id,
            item.project_id,
            &fields,
        )?;
        let ticket = loom_tickets::create_ticket(
            loom,
            workspace,
            loom_tickets::TicketCreateRequest {
                workspace_id: request.workspace_id,
                project_id: item.project_id,
                ticket_type: item.ticket_type.unwrap_or("task"),
                external_source: Some("structure-node"),
                external_id: Some(&external_id),
                fields: &fields,
                policy_labels: item.policy_labels,
                expected_root: None,
            },
        )?;
        let ticket_ref = format!("ticket:{}", ticket.ticket_id);
        graph_upsert_node(
            loom,
            workspace,
            &graph_collection,
            &ticket_ref,
            ticket_node_props(&ticket),
        )?;
        let edge_id = format!(
            "{}:implemented_by:{}:{}",
            request.structure_id, item.node_id, ticket.ticket_id
        );
        graph_upsert_edge(
            loom,
            workspace,
            &graph_collection,
            &edge_id,
            item.node_id,
            &ticket_ref,
            "implemented_by",
            implemented_by_props(request.structure_id, item.node_id, &ticket.ticket_id),
        )?;
        implemented_by_edges.push(edge_id);
        tickets.push(ticket);
    }
    Ok(StructureDecomposeSummary {
        workspace_id: request.workspace_id.to_string(),
        structure_id: request.structure_id.to_string(),
        tickets,
        implemented_by_edges,
        graph_collection,
    })
}

pub fn get_structure(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    structure_id: &str,
) -> Result<Option<StructureRenderSummary>> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Read)?;
    let organization = load_workspace(loom.store(), workspace_id)?;
    if !organization.structures.contains_key(structure_id) {
        return Ok(None);
    }
    let profile_root = profile_snapshot_digest(loom, workspace_id, &organization)?;
    structure_render_summary(workspace_id, &organization, structure_id, profile_root).map(Some)
}

pub fn list_structures(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<StructureSummary>> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Read)?;
    let organization = load_workspace(loom.store(), workspace_id)?;
    let profile_root = profile_snapshot_digest(loom, workspace_id, &organization)?;
    Ok(organization
        .structures
        .values()
        .map(|structure| structure_summary(workspace_id, structure, profile_root))
        .collect())
}

fn load_workspace<S: ObjectStore>(store: &S, workspace_id: &str) -> Result<PageWorkspace> {
    let key = page_workspace_snapshot_key(workspace_id)?;
    match page_control_get(store, workspace_id, "workspace-snapshot", &key)? {
        Some(bytes) => PageWorkspaceSnapshot::decode(&bytes)?.organization(store.digest_algo()),
        None => Ok(PageWorkspace::default()),
    }
}

fn save_workspace<S: ObjectStore>(
    store: &S,
    workspace: WorkspaceId,
    workspace_id: &str,
    organization: &PageWorkspace,
) -> Result<()> {
    let snapshot = PageWorkspaceSnapshot::from_workspace(workspace_id, organization)?;
    page_control_set(
        store,
        workspace,
        workspace_id,
        "workspace-snapshot",
        &page_workspace_snapshot_key(workspace_id)?,
        snapshot.encode()?,
    )
}

fn workspace_snapshot_bytes(workspace_id: &str, organization: &PageWorkspace) -> Result<Vec<u8>> {
    PageWorkspaceSnapshot::from_workspace(workspace_id, organization)?.encode()
}

fn profile_snapshot_digest<S: ObjectStore>(
    loom: &Loom<S>,
    workspace_id: &str,
    organization: &PageWorkspace,
) -> Result<Digest> {
    let snapshot = PageWorkspaceSnapshot::from_workspace(workspace_id, organization)?;
    Ok(Digest::hash(
        loom.store().digest_algo(),
        &snapshot.encode()?,
    ))
}

fn enforce_expected_root<S: ObjectStore>(
    loom: &Loom<S>,
    workspace_id: &str,
    organization: &PageWorkspace,
    expected_root: Option<&str>,
) -> Result<()> {
    let Some(expected_root) = expected_root else {
        return Ok(());
    };
    let expected = Digest::parse(expected_root)?;
    let actual = profile_snapshot_digest(loom, workspace_id, organization)?;
    if expected != actual {
        return Err(LoomError::new(
            Code::Conflict,
            "page profile root does not match expected_root",
        ));
    }
    Ok(())
}

fn load_log<S: ObjectStore>(store: &S, workspace_id: &str) -> Result<PageOperationLog> {
    let key = page_profile_operation_log_key(workspace_id)?;
    if store.uses_mutable_overlay_current_records() {
        let head = store.retained_history_head(&key)?;
        if head > 0 {
            let records = store
                .retained_history_records(&key, 1, usize::MAX)?
                .into_iter()
                .map(|bytes| PageOperationRecord::decode(&bytes))
                .collect::<Result<Vec<_>>>()?;
            return PageOperationLog::new(workspace_id, records);
        }
    }
    match page_control_get(store, workspace_id, "operation-log", &key)? {
        Some(bytes) => PageOperationLog::decode(&bytes),
        None => PageOperationLog::new(workspace_id, Vec::new()),
    }
}

pub fn page_operation_log(store: &FileStore, workspace_id: &str) -> Result<PageOperationLog> {
    load_log(store, workspace_id)
}

fn retained_operation_append<S: ObjectStore>(
    store: &S,
    workspace_id: &str,
    record: &PageOperationRecord,
) -> Result<Vec<loom_core::WorkflowControlWrite>> {
    let key = page_profile_operation_log_key(workspace_id)?;
    let head = store.retained_history_head(&key)?;
    let expected_next_sequence = head
        .checked_add(1)
        .ok_or_else(|| LoomError::invalid("page operation sequence overflow"))?;
    let mut records = Vec::new();
    let mut remove_legacy = false;
    let record_sequence = if head == 0 {
        let legacy = match page_control_get(store, workspace_id, "operation-log", &key)? {
            Some(bytes) => {
                remove_legacy = true;
                PageOperationLog::decode(&bytes)?.records
            }
            None => Vec::new(),
        };
        let next = legacy
            .last()
            .map(|record| record.sequence.saturating_add(1))
            .unwrap_or(1);
        records.extend(
            legacy
                .into_iter()
                .map(|record| record.encode())
                .collect::<Result<Vec<_>>>()?,
        );
        next
    } else {
        expected_next_sequence
    };
    if record.sequence != record_sequence {
        return Err(LoomError::new(
            Code::Conflict,
            format!(
                "page operation sequence must be {record_sequence}, got {}",
                record.sequence
            ),
        ));
    }
    records.push(record.encode()?);
    let mut writes = vec![loom_core::WorkflowControlWrite::AppendRetained {
        key: key.clone(),
        expected_next_sequence,
        records,
    }];
    if remove_legacy {
        writes.push(loom_core::WorkflowControlWrite::Delete { key });
    }
    Ok(writes)
}

fn append_operation<S: ObjectStore>(
    store: &S,
    workspace: WorkspaceId,
    workspace_id: &str,
    record: &PageOperationRecord,
) -> Result<()> {
    let mut log = load_log(store, workspace_id)?;
    log.records.push(record.clone());
    page_control_set(
        store,
        workspace,
        workspace_id,
        "operation-log",
        &page_profile_operation_log_key(workspace_id)?,
        log.encode()?,
    )
}

fn save_workspace_and_append_operation<S: ObjectStore>(
    loom: &mut Loom<S>,
    workspace: WorkspaceId,
    workspace_id: &str,
    organization: &PageWorkspace,
    record: &PageOperationRecord,
    published_revision: Option<&PageRevision>,
    mut owner_state: loom_core::WorkflowOwnerState,
    planning_snapshot: Option<&WorkflowPlanningSnapshot>,
) -> Result<()> {
    let index = next_page_revision_index(
        loom,
        workspace,
        workspace_id,
        Some(organization),
        record,
        published_revision,
    )?;
    let store = loom.store();
    if store.uses_mutable_overlay_current_records() {
        let snapshot = PageWorkspaceSnapshot::from_workspace(workspace_id, organization)?;
        owner_state
            .controls
            .extend(retained_operation_append(store, workspace_id, record)?);
        let (revision_writes, revision_controls) = current_revision_index_append_writes(
            loom,
            workspace,
            workspace_id,
            FacetKind::Document,
            &index,
        )?;
        owner_state.controls.extend(revision_controls);
        let receipt = page_control_set_many(
            store,
            workspace,
            workspace_id,
            vec![(
                page_control_overlay_key(workspace_id, "workspace-snapshot")?,
                snapshot.encode()?,
            )],
            revision_writes,
            owner_state,
            planning_snapshot,
        )?;
        synchronize_page_transaction_receipt(loom, &receipt)?;
        Ok(())
    } else {
        save_workspace(store, workspace, workspace_id, organization)?;
        append_operation(store, workspace, workspace_id, record)?;
        let full = index.apply_to(load_current_revision_index(loom, workspace, workspace_id)?)?;
        persist_current_revision_index(loom, workspace, workspace_id, FacetKind::Document, &full)
    }
}

fn page_control_overlay_key(workspace_id: &str, kind: &str) -> Result<OverlayKey> {
    OverlayKey::from_segments([
        PAGE_CONTROL_OVERLAY_SCHEMA.as_bytes(),
        workspace_id.as_bytes(),
        kind.as_bytes(),
        b"",
        b"",
        b"v1",
    ])
}

fn page_control_get<S: ObjectStore>(
    store: &S,
    workspace_id: &str,
    kind: &str,
    legacy_key: &[u8],
) -> Result<Option<Vec<u8>>> {
    if store.uses_mutable_overlay_current_records() {
        let overlay_key = page_control_overlay_key(workspace_id, kind)?;
        if let Some(entry) = store.mutable_overlay_current_entry(&overlay_key)? {
            Ok(Some(entry.payload))
        } else {
            store.control_get(legacy_key)
        }
    } else {
        store.control_get(legacy_key)
    }
}

fn page_control_set<S: ObjectStore>(
    store: &S,
    workspace: WorkspaceId,
    workspace_id: &str,
    kind: &str,
    legacy_key: &[u8],
    value: Vec<u8>,
) -> Result<()> {
    if store.uses_mutable_overlay_current_records() {
        let _ = page_control_set_many(
            store,
            workspace,
            workspace_id,
            vec![(page_control_overlay_key(workspace_id, kind)?, value)],
            Vec::new(),
            loom_core::WorkflowOwnerState::default(),
            None,
        )?;
        Ok(())
    } else {
        store.control_set(legacy_key, value)
    }
}

fn page_control_set_many<S: ObjectStore>(
    store: &S,
    workspace: WorkspaceId,
    workspace_id: &str,
    values: Vec<(OverlayKey, Vec<u8>)>,
    mut extra_writes: Vec<FacetWrite>,
    owner_state: loom_core::WorkflowOwnerState,
    planning_snapshot: Option<&WorkflowPlanningSnapshot>,
) -> Result<loom_core::CommitReceipt> {
    let owned_snapshot = planning_snapshot
        .is_none()
        .then(|| WorkflowPlanningSnapshot::open(store, Some("pages.control.write")))
        .transpose()?;
    let snapshot = planning_snapshot
        .or(owned_snapshot.as_ref())
        .expect("planning snapshot is supplied or opened");
    let mut writes = values
        .into_iter()
        .map(|(key, payload)| {
            Ok(FacetWrite {
                facet: FacetKind::Document,
                secondary_indexes: page_control_secondary_indexes(workspace_id, &key, &payload),
                expected: snapshot.owner_token(&key)?.map(CompareToken),
                target: key,
                op: FacetWriteOp::Put { payload },
                durability: None,
                audit: Some(AuditIntent {
                    operation: "pages.control.put".to_string(),
                }),
                side_effects: FacetSideEffects::default(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    writes.append(&mut extra_writes);
    let expected_generation = snapshot.expected_generation();
    snapshot.release()?;
    store.commit_workflow_transaction(WorkflowTransaction {
        workspace,
        actor: workspace,
        expected_generation: Some(expected_generation),
        writes,
        prepared_operations: Vec::new(),
        revision_metadata: Vec::new(),
        delivery_intents: Vec::new(),
        durability: OverlayDurabilityPolicy::Normal,
        boundary: AtomicityBoundary::Single,
        idempotency: None,
        owner_state,
        post_commit_delta: None,
    })
}

fn synchronize_page_transaction_receipt<S: ObjectStore>(
    loom: &mut Loom<S>,
    receipt: &loom_core::CommitReceipt,
) -> Result<()> {
    for outcome in &receipt.writes {
        let current = loom
            .store()
            .mutable_overlay_current_entry(&outcome.target)?
            .ok_or_else(|| {
                LoomError::corrupt("page transaction receipt target is absent after commit")
            })?;
        loom.mutable_overlay_mut()
            .synchronize_current_entry(current)?;
    }
    Ok(())
}

fn page_control_secondary_indexes(
    workspace_id: &str,
    key: &OverlayKey,
    payload: &[u8],
) -> Vec<SecondaryIndexWrite> {
    let Ok(segments) = key.segments() else {
        return Vec::new();
    };
    let [schema, key_workspace_id, kind, _, _, version] = segments.as_slice() else {
        return Vec::new();
    };
    if *schema != PAGE_CONTROL_OVERLAY_SCHEMA.as_bytes()
        || *key_workspace_id != workspace_id.as_bytes()
        || *version != b"v1"
    {
        return Vec::new();
    }
    let Ok(kind) = std::str::from_utf8(kind) else {
        return Vec::new();
    };
    let Ok(index) = OverlayKey::from_segments([
        PAGE_CONTROL_OVERLAY_SCHEMA.as_bytes(),
        workspace_id.as_bytes(),
        b"control-kind",
        kind.as_bytes(),
        b"",
        b"v1",
    ]) else {
        return Vec::new();
    };
    vec![SecondaryIndexWrite {
        index,
        op: SecondaryIndexWriteOp::Put {
            payload: payload.to_vec(),
        },
    }]
}

fn record_organization_operation(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: OperationRecordRequest<'_>,
) -> Result<()> {
    let workspace_id = request.workspace_id;
    let record = operation_record(loom, workspace, request)?;
    let index = next_page_revision_index(loom, workspace, workspace_id, None, &record, None)?;
    if loom.store().uses_mutable_overlay_current_records() {
        let (revision_writes, revision_controls) = current_revision_index_append_writes(
            loom,
            workspace,
            workspace_id,
            FacetKind::Document,
            &index,
        )?;
        let mut controls = retained_operation_append(loom.store(), workspace_id, &record)?;
        controls.extend(revision_controls);
        let receipt = page_control_set_many(
            loom.store(),
            workspace,
            workspace_id,
            Vec::new(),
            revision_writes,
            loom_core::WorkflowOwnerState {
                controls,
                ..loom_core::WorkflowOwnerState::default()
            },
            None,
        )?;
        synchronize_page_transaction_receipt(loom, &receipt)?;
        Ok(())
    } else {
        append_operation(loom.store(), workspace, workspace_id, &record)?;
        let full = index.apply_to(load_current_revision_index(loom, workspace, workspace_id)?)?;
        persist_current_revision_index(loom, workspace, workspace_id, FacetKind::Document, &full)
    }
}

fn operation_record<S: ObjectStore>(
    loom: &Loom<S>,
    workspace: WorkspaceId,
    request: OperationRecordRequest<'_>,
) -> Result<PageOperationRecord> {
    let sequence = if loom.store().uses_mutable_overlay_current_records() {
        let key = page_profile_operation_log_key(request.workspace_id)?;
        let head = loom.store().retained_history_head(&key)?;
        if head == 0 {
            load_log(loom.store(), request.workspace_id)?
                .records
                .last()
                .map(|record| record.sequence.saturating_add(1))
                .unwrap_or(1)
        } else {
            head.checked_add(1)
                .ok_or_else(|| LoomError::invalid("page operation sequence overflow"))?
        }
    } else {
        load_log(loom.store(), request.workspace_id)?
            .records
            .last()
            .map(|record| record.sequence.saturating_add(1))
            .unwrap_or(1)
    };
    let operation_id = format!("{}:{sequence}", request.workspace_id);
    let actor_principal = loom.effective_principal()?.unwrap_or(workspace);
    let envelope = OperationEnvelope::new(
        loom.store().digest_algo(),
        OperationEnvelopeInput {
            workspace_id: request.workspace_id,
            app_id: APP_ID,
            scope_id: request.scope_id,
            operation_id: &operation_id,
            operation_kind: request.operation_kind,
            sequence,
            actor_principal,
            actor_kind: ActorKind::User,
            timestamp_ms: now_ms(),
            idempotency_key: &operation_id,
            base_root: Digest::hash(loom.store().digest_algo(), request.base_snapshot),
            base_entity_version: None,
            target_entity_id: request.target_entity_id,
            payload: request.payload,
            policy_labels: &[],
            signature: None,
            agent: None,
        },
    )?;
    PageOperationRecord::new(
        sequence,
        operation_id,
        request.operation_kind,
        request.target_entity_id.map(str::to_string),
        request.root_after,
        envelope.encode()?,
    )
}

pub fn operation_changes(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    cursor: &loom_substrate::changes::OperationChangeCursor,
    max: usize,
) -> Result<loom_substrate::changes::OperationChangeBatch> {
    loom.authorize_domain(workspace, AclDomain::Pages, AclRight::Read)?;
    let workspace_id = cursor
        .scope_id
        .strip_prefix("pages:")
        .ok_or_else(|| LoomError::invalid("unsupported page operation cursor"))?;
    let log = load_log(loom.store(), workspace_id)?;
    log.changes(cursor, max)
}

fn next_page_revision_index<S: ObjectStore>(
    loom: &Loom<S>,
    workspace: WorkspaceId,
    workspace_id: &str,
    organization: Option<&PageWorkspace>,
    record: &PageOperationRecord,
    published_revision: Option<&PageRevision>,
) -> Result<RevisionIndexAppend> {
    let Some(target_entity_id) = record.target_entity_id.as_deref() else {
        return Ok(RevisionIndexAppend::new());
    };
    let entity_id =
        page_operation_revision_entity_id(record.operation_kind.as_str(), target_entity_id);
    let envelope = OperationEnvelope::decode(&record.envelope)?;
    let expected_latest_revision =
        load_latest_entity_revision(loom, workspace, workspace_id, &entity_id)?
            .map(|entry| entry.revision)
            .unwrap_or(0);
    let revision = expected_latest_revision
        .checked_add(1)
        .ok_or_else(|| LoomError::invalid("page entity revision overflow"))?;
    let mut index = RevisionIndexAppend::new();
    index.push_revision(EntityRevision::new(
        entity_id.clone(),
        revision,
        record.operation_id.clone(),
        BodyRef::new(
            Digest::hash(loom.store().digest_algo(), &record.envelope),
            record.envelope.len() as u64,
            "application/vnd.uldren.loom.pages.operation+cbor",
        )?,
        record.root_after,
        envelope.timestamp_ms,
    )?)?;
    index.push_checkpoint(Checkpoint::new(
        workspace_id,
        format!("pages:{workspace_id}:{entity_id}:{revision}"),
        record.root_after,
        revision,
        record.operation_id.clone(),
        envelope.timestamp_ms,
    )?)?;
    if let Some(revision) = published_revision {
        let organization = organization.ok_or_else(|| {
            LoomError::invalid("published revision requires page workspace state")
        })?;
        index = apply_published_page_revision(
            loom,
            workspace,
            workspace_id,
            organization,
            index,
            revision,
        )?;
    }
    Ok(index)
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

fn apply_published_page_revision<S: ObjectStore>(
    loom: &Loom<S>,
    workspace: WorkspaceId,
    workspace_id: &str,
    organization: &PageWorkspace,
    index: RevisionIndexAppend,
    revision: &PageRevision,
) -> Result<RevisionIndexAppend> {
    let snapshot = PageWorkspaceSnapshot::from_workspace(workspace_id, organization)?;
    let snapshot_bytes = snapshot.encode()?;
    let root = Digest::hash(loom.store().digest_algo(), &snapshot_bytes);
    let operation_id = format!(
        "pages:{workspace_id}:{}:{}",
        revision.page_id, revision.revision
    );
    let entity_id = format!("page:{}", revision.page_id);
    let current_revision = load_latest_entity_revision(loom, workspace, workspace_id, &entity_id)?
        .map(|revision| revision.revision)
        .unwrap_or(0);
    if current_revision != revision.revision.saturating_sub(1) {
        return Err(LoomError::new(
            Code::Conflict,
            "published page revision does not follow the current entity revision",
        ));
    }
    let mut index = index;
    index.push_revision(EntityRevision::new(
        entity_id,
        revision.revision,
        operation_id.clone(),
        BodyRef::new(
            revision.body_digest,
            revision.body.len() as u64,
            "application/octet-stream",
        )?,
        root,
        revision.published_at_ms,
    )?)?;
    index.push_checkpoint(Checkpoint::new(
        workspace_id,
        format!("{}:{}", revision.page_id, revision.revision),
        root,
        revision.revision,
        operation_id,
        revision.published_at_ms,
    )?)?;
    Ok(index)
}

fn prepare_page_published_refs<S: ObjectStore>(
    loom: &mut Loom<S>,
    workspace: WorkspaceId,
    workspace_id: &str,
    page_id: &str,
    body: &[u8],
) -> Result<PreparedPageReferences> {
    let snapshot =
        WorkflowPlanningSnapshot::open(loom.store(), Some("pages.reference-publication.plan"))?;
    let graph_path =
        loom_core::workspace::facet_path(FacetKind::Graph, loom_reference::REFERENCE_GRAPH);
    let scope = EnginePlanningScope::new(
        workspace,
        [
            EnginePathSelector::Exact(loom_reference::INDEX_PATH.to_string()),
            EnginePathSelector::Exact(graph_path),
        ],
    )?;
    let Some(base_root) = snapshot.immutable_base_root() else {
        let mut index = loom_reference::load_or_rebuild_index(loom, workspace)?;
        update_published_page_reference_index(&mut index, workspace_id, page_id, body)?;
        loom_reference::save_index(loom, workspace, &index)?;
        loom_reference::project_reference_index_edges(loom, workspace, &index)?;
        return Ok(PreparedPageReferences {
            owner_state: loom_core::WorkflowOwnerState::default(),
            live_state: None,
            snapshot,
        });
    };
    let (mut planner, mut index) = loom_reference::bounded_planner_with_reference_index(
        &*loom,
        scope,
        base_root,
        snapshot.fork_overlay(),
    )?;
    update_published_page_reference_index(&mut index, workspace_id, page_id, body)?;
    loom_reference::save_index(planner.engine_mut(), workspace, &index)?;
    loom_reference::project_reference_index_edges(planner.engine_mut(), workspace, &index)?;
    let (live_state, planned_objects) = planner.finish()?;
    let (reference_root, objects) =
        loom.prepare_engine_state_delta_reference(&live_state, base_root, planned_objects)?;
    Ok(PreparedPageReferences {
        owner_state: loom_core::WorkflowOwnerState {
            objects,
            reference: loom_core::WorkflowReferenceUpdate::Set(Some(reference_root)),
            controls: Vec::new(),
            audits: Vec::new(),
        },
        live_state: Some(live_state),
        snapshot,
    })
}

fn update_published_page_reference_index(
    index: &mut ReferenceIndex,
    workspace_id: &str,
    page_id: &str,
    body: &[u8],
) -> Result<()> {
    let text_source = ReferenceSource::new("pages", workspace_id, page_id, "published_body")?;
    let block_ref_source = ReferenceSource::new("pages", workspace_id, page_id, "block_ref")?;
    index.remove_source(&text_source);
    index.remove_source(&block_ref_source);
    if let Ok(text) = std::str::from_utf8(body) {
        index.add_text_refs(text_source.clone(), "refers_to", text)?;
    }
    if let Ok(decoded) = Body::decode(body) {
        let rendered = decoded.render_text_with_refs(|_| Ok(BlockRefResolution::Missing))?;
        index.add_text_refs(text_source, "refers_to", &rendered.text)?;
        add_block_ref_edges(index, block_ref_source, &decoded)?;
    }
    Ok(())
}

fn add_block_ref_edges(
    index: &mut ReferenceIndex,
    source: ReferenceSource,
    body: &Body,
) -> Result<()> {
    for block in &body.blocks {
        add_block_ref_edges_from_block(index, &source, block)?;
    }
    Ok(())
}

fn add_block_ref_edges_from_block(
    index: &mut ReferenceIndex,
    source: &ReferenceSource,
    block: &Block,
) -> Result<()> {
    if let BlockKind::BlockRef {
        entity_id,
        block_id,
        section,
        pin,
    } = &block.kind
        && let Ok(target) = EntityRef::parse(entity_id)
    {
        let evidence = block_ref_evidence(
            &block.block_id,
            entity_id,
            block_id.as_deref(),
            *section,
            *pin,
        );
        let span_start = evidence
            .find(entity_id)
            .ok_or_else(|| LoomError::corrupt("block_ref evidence target missing"))?;
        let span_end = span_start + entity_id.len();
        index.add(ReferenceEdge::new(
            source.clone(),
            target,
            "transcludes",
            span_start,
            span_end,
            evidence,
        )?);
    }
    for child in &block.children {
        add_block_ref_edges_from_block(index, source, child)?;
    }
    Ok(())
}

fn block_ref_evidence(
    source_block_id: &str,
    entity_id: &str,
    target_block_id: Option<&str>,
    section: bool,
    pin: Option<u64>,
) -> String {
    let mut evidence = format!("block_ref {source_block_id} -> {entity_id}");
    if let Some(target_block_id) = target_block_id {
        evidence.push('#');
        evidence.push_str(target_block_id);
    }
    if section {
        evidence.push_str(" section");
    }
    if let Some(pin) = pin {
        evidence.push('@');
        evidence.push_str(&pin.to_string());
    }
    evidence
}

fn page_by_id<'a>(organization: &'a PageWorkspace, page_id: &str) -> Result<&'a WorkspacePage> {
    organization
        .pages
        .get(page_id)
        .ok_or_else(|| LoomError::not_found("page not found"))
}

fn space_summary(workspace_id: &str, space: &PageSpace, profile_root: Digest) -> SpaceSummary {
    SpaceSummary {
        workspace_id: workspace_id.to_string(),
        space_id: space.space_id.clone(),
        title: space.title.clone(),
        archived: space.archived,
        profile_root: profile_root.to_string(),
    }
}

fn page_summary(
    workspace_id: &str,
    organization: &PageWorkspace,
    page: &WorkspacePage,
    principal: WorkspaceId,
    profile_root: Digest,
) -> PageSummary {
    let draft = organization
        .drafts
        .get(&page_draft_id(&page.page_id, principal));
    let body = page
        .current_revision
        .and_then(|revision| {
            organization
                .revisions
                .get(&(page.page_id.clone(), revision))
        })
        .map(|revision| revision.body.clone());
    let body_render = body
        .as_ref()
        .and_then(|body| render_page_body(organization, body));
    let draft_render = draft.and_then(|draft| render_page_body(organization, &draft.body));
    PageSummary {
        workspace_id: workspace_id.to_string(),
        page_id: page.page_id.clone(),
        space_id: page.space_id.clone(),
        parent_page_id: page.parent_page_id.clone(),
        title: page.title.clone(),
        current_revision: page.current_revision,
        deleted: page.deleted,
        status: page_status(page, draft),
        body,
        draft_body: draft.map(|draft| draft.body.clone()),
        body_text: body_render.as_ref().map(|render| render.text.clone()),
        draft_body_text: draft_render.as_ref().map(|render| render.text.clone()),
        rendered_body: body_render.as_ref().map(|render| render.text.clone()),
        draft_rendered_body: draft_render.as_ref().map(|render| render.text.clone()),
        render_issues: body_render
            .map(|render| render_issues(&render.tickets))
            .unwrap_or_default(),
        draft_render_issues: draft_render
            .map(|render| render_issues(&render.tickets))
            .unwrap_or_default(),
        profile_root: profile_root.to_string(),
    }
}

fn render_page_body(organization: &PageWorkspace, body: &[u8]) -> Option<BodyRender> {
    let Ok(decoded) = Body::decode(body) else {
        return None;
    };
    decoded
        .render_text_with_refs(|target| resolve_page_block_ref(organization, target))
        .ok()
}

fn resolve_page_block_ref(
    organization: &PageWorkspace,
    target: &BlockRefTarget,
) -> Result<BlockRefResolution> {
    let Some(page_id) = target.entity_id.strip_prefix("page:") else {
        return Ok(BlockRefResolution::Missing);
    };
    let Some(page) = organization.pages.get(page_id) else {
        return Ok(BlockRefResolution::Missing);
    };
    let revision_id = match target.pin.or(page.current_revision) {
        Some(revision_id) => revision_id,
        None => return Ok(BlockRefResolution::Missing),
    };
    let Some(revision) = organization
        .revisions
        .get(&(page.page_id.clone(), revision_id))
    else {
        return Ok(BlockRefResolution::Missing);
    };
    match Body::decode(&revision.body) {
        Ok(body) => Ok(BlockRefResolution::Found(body)),
        Err(_) => Ok(BlockRefResolution::Missing),
    }
}

fn render_issues(tickets: &[BodyRenderIssue]) -> Vec<PageRenderIssueSummary> {
    tickets
        .iter()
        .map(|ticket| PageRenderIssueSummary {
            kind: render_issue_kind(&ticket.kind).to_string(),
            entity_id: ticket.entity_id.clone(),
            block_id: ticket.block_id.clone(),
        })
        .collect()
}

fn render_issue_kind(kind: &BodyRenderIssueKind) -> &'static str {
    match kind {
        BodyRenderIssueKind::MissingTarget => "missing_target",
        BodyRenderIssueKind::MissingBlock => "missing_block",
        BodyRenderIssueKind::Shredded => "shredded",
        BodyRenderIssueKind::Cycle => "cycle",
        BodyRenderIssueKind::DepthLimit => "depth_limit",
    }
}

fn page_status(page: &WorkspacePage, draft: Option<&PageDraft>) -> String {
    if page.deleted {
        "deleted"
    } else if draft.is_some() {
        "draft"
    } else if page.current_revision.is_some() {
        "published"
    } else {
        "empty"
    }
    .to_string()
}

fn page_draft_id(page_id: &str, principal: WorkspaceId) -> String {
    format!("{page_id}:{principal}")
}

fn revision_history_entry(revision: &PageRevision) -> PageHistoryEntry {
    PageHistoryEntry {
        kind: "revision".to_string(),
        page_id: revision.page_id.clone(),
        revision: Some(revision.revision),
        body_digest: Some(revision.body_digest.to_string()),
        author: Some(revision.author.to_string()),
        published_at_ms: Some(revision.published_at_ms),
        conflict_id: None,
        base_revision: None,
        current_revision: Some(revision.revision),
        candidate_digest: None,
    }
}

fn conflict_history_entry(conflict: &PageConflict) -> PageHistoryEntry {
    PageHistoryEntry {
        kind: "conflict".to_string(),
        page_id: conflict.page_id.clone(),
        revision: None,
        body_digest: None,
        author: None,
        published_at_ms: None,
        conflict_id: Some(conflict.conflict_id.clone()),
        base_revision: conflict.base_revision,
        current_revision: conflict.current_revision,
        candidate_digest: Some(conflict.candidate_digest.to_string()),
    }
}

fn structure_render_summary(
    workspace_id: &str,
    organization: &PageWorkspace,
    structure_id: &str,
    profile_root: Digest,
) -> Result<StructureRenderSummary> {
    let structure = organization
        .structures
        .get(structure_id)
        .ok_or_else(|| LoomError::not_found("structure not found"))?;
    let mut nodes = organization
        .structure_nodes
        .values()
        .filter(|node| node.structure_id == structure_id)
        .map(|node| node_summary(workspace_id, node, profile_root))
        .collect::<Vec<_>>();
    nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    let mut edges = organization
        .structure_edges
        .values()
        .filter(|edge| edge.structure_id == structure_id)
        .map(|edge| edge_summary(workspace_id, edge, profile_root))
        .collect::<Vec<_>>();
    edges.sort_by(|a, b| a.edge_id.cmp(&b.edge_id));
    Ok(StructureRenderSummary {
        structure: structure_summary(workspace_id, structure, profile_root),
        nodes,
        edges,
        graph_collection: structure_graph_collection(workspace_id, structure_id),
    })
}

fn structure_summary(
    workspace_id: &str,
    structure: &PageStructure,
    profile_root: Digest,
) -> StructureSummary {
    StructureSummary {
        workspace_id: workspace_id.to_string(),
        structure_id: structure.structure_id.clone(),
        space_id: structure.space_id.clone(),
        kind: structure.kind.clone(),
        title: structure.title.clone(),
        root_node_id: structure.root_node_id.clone(),
        field_ids: structure.field_ids.clone(),
        profile_root: profile_root.to_string(),
    }
}

fn node_summary(
    workspace_id: &str,
    node: &StructureNode,
    profile_root: Digest,
) -> StructureNodeSummary {
    StructureNodeSummary {
        workspace_id: workspace_id.to_string(),
        structure_id: node.structure_id.clone(),
        node_id: node.node_id.clone(),
        kind: node.kind.clone(),
        label: node.label.clone(),
        body_digest: node.body_digest.map(|digest| digest.to_string()),
        entity_ref: node.entity_ref.clone(),
        profile_root: profile_root.to_string(),
    }
}

fn edge_summary(
    workspace_id: &str,
    edge: &StructureEdge,
    profile_root: Digest,
) -> StructureEdgeSummary {
    StructureEdgeSummary {
        workspace_id: workspace_id.to_string(),
        structure_id: edge.structure_id.clone(),
        edge_id: edge.edge_id.clone(),
        src_node_id: edge.src_node_id.clone(),
        dst_node_id: edge.dst_node_id.clone(),
        label: edge.label.clone(),
        target_ref: edge.target_ref.clone(),
        profile_root: profile_root.to_string(),
    }
}

fn structure_graph_collection(workspace_id: &str, structure_id: &str) -> String {
    format!("pages.{workspace_id}.structure.{structure_id}")
}

fn node_props(node: &StructureNode) -> Props {
    let mut props = BTreeMap::new();
    props.insert(
        "structure_id".to_string(),
        GraphValue::Text(node.structure_id.clone()),
    );
    props.insert("kind".to_string(), GraphValue::Text(node.kind.clone()));
    props.insert("label".to_string(), GraphValue::Text(node.label.clone()));
    if let Some(digest) = node.body_digest {
        props.insert(
            "body_digest".to_string(),
            GraphValue::Text(digest.to_string()),
        );
    }
    if let Some(entity_ref) = &node.entity_ref {
        props.insert(
            "entity_ref".to_string(),
            GraphValue::Text(entity_ref.clone()),
        );
    }
    props
}

fn edge_props(edge: &StructureEdge) -> Props {
    let mut props = BTreeMap::new();
    props.insert(
        "structure_id".to_string(),
        GraphValue::Text(edge.structure_id.clone()),
    );
    if let Some(target_ref) = &edge.target_ref {
        props.insert(
            "target_ref".to_string(),
            GraphValue::Text(target_ref.clone()),
        );
    }
    props
}

fn decompose_fields(node: &StructureNode, fields: Option<&Value>) -> Result<Value> {
    let mut map = match fields {
        Some(Value::Object(map)) => map.clone(),
        Some(_) => {
            return Err(LoomError::invalid(
                "decompose ticket fields must be an object",
            ));
        }
        None => Map::new(),
    };
    map.entry("title".to_string())
        .or_insert_with(|| Value::String(node.label.clone()));
    if let Some(entity_ref) = &node.entity_ref {
        map.entry("source_ref".to_string())
            .or_insert_with(|| Value::String(entity_ref.clone()));
    }
    Ok(Value::Object(map))
}

fn ensure_decompose_ticket_fields(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    project_id: &str,
    fields: &Value,
) -> Result<()> {
    let Value::Object(fields) = fields else {
        return Ok(());
    };
    if !fields.contains_key("source_ref") {
        return Ok(());
    }
    let catalog = loom_tickets::ticket_field_catalog_for_project(
        loom,
        workspace,
        workspace_id,
        project_id,
        None,
        Some("write"),
    )?;
    if catalog
        .fields
        .iter()
        .any(|field| field.native_field == "source_ref")
    {
        return Ok(());
    }
    let applicable_type_ids: Vec<String> = Vec::new();
    loom_tickets::put_ticket_field_definition(
        loom,
        workspace,
        loom_tickets::TicketFieldDefinitionWriteRequest {
            workspace_id,
            project_id,
            field_id: "source_ref",
            key: "source_ref",
            name: "Source reference",
            description: Some("Entity reference carried from page structure decomposition."),
            field_type: "string",
            option_set: None,
            max_length: None,
            required: false,
            searchable: true,
            orderable: false,
            cardinality: loom_tickets::TicketFieldCardinality::Single,
            applicable_type_ids: &applicable_type_ids,
            expected_root: None,
        },
    )?;
    Ok(())
}

fn ticket_node_props(ticket: &loom_tickets::TicketSummary) -> Props {
    let mut props = BTreeMap::new();
    props.insert("kind".to_string(), GraphValue::Text("ticket".to_string()));
    props.insert(
        "ticket_id".to_string(),
        GraphValue::Text(ticket.ticket_id.clone()),
    );
    props.insert(
        "primary_key".to_string(),
        GraphValue::Text(ticket.primary_key.clone()),
    );
    props.insert(
        "project_id".to_string(),
        GraphValue::Text(ticket.project_id.clone()),
    );
    props
}

fn implemented_by_props(structure_id: &str, node_id: &str, ticket_id: &str) -> Props {
    let mut props = BTreeMap::new();
    props.insert(
        "structure_id".to_string(),
        GraphValue::Text(structure_id.to_string()),
    );
    props.insert("node_id".to_string(), GraphValue::Text(node_id.to_string()));
    props.insert(
        "ticket_id".to_string(),
        GraphValue::Text(ticket_id.to_string()),
    );
    props
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::graph_edges;
    use loom_store::{open_loom, save_loom};
    use loom_substrate::changes::OperationChangeCursor;
    use loom_substrate::pages::page_operation_cursor_scope;
    use std::path::PathBuf;

    fn test_store_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("loom-pages-{name}-{}.loom", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn assert_prior_published_page_state(
        loom: &Loom<FileStore>,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) {
        let page = get_page(loom, workspace, workspace_id, "page-1")
            .unwrap()
            .unwrap();
        assert_eq!(page.current_revision, Some(1));
        let revisions = load_current_revision_index(loom, workspace, workspace_id).unwrap();
        assert_eq!(
            revisions.latest("page:page-1").map(|entry| entry.revision),
            Some(1)
        );
        let source =
            ReferenceSource::new("pages", workspace_id, "page-1", "published_body").unwrap();
        let references = loom_reference::references_from(loom, workspace, &source).unwrap();
        assert!(!references.is_empty());
        assert!(
            references
                .iter()
                .all(|edge| { edge.target == EntityRef::parse("ticket:MX-OLD").unwrap() })
        );
        let projected = graph_edges(loom, workspace, loom_reference::REFERENCE_GRAPH).unwrap();
        assert!(!projected.is_empty());
        assert!(
            projected
                .iter()
                .all(|(_, edge)| edge.dst == "ticket:MX-OLD")
        );
    }

    fn seed_page_workspace(loom: &mut Loom<FileStore>, workspace: WorkspaceId, workspace_id: &str) {
        create_space(loom, workspace, workspace_id, "space", "Space", None).unwrap();
        create_page(
            loom,
            workspace,
            PageCreateRequest {
                workspace_id,
                page_id: "page-1",
                space_id: "space",
                parent_page_id: None,
                title: "Page",
                expected_root: None,
            },
        )
        .unwrap();
    }

    fn install_authenticated_pages_sessions(loom: &mut Loom<FileStore>, workspace: WorkspaceId) {
        let root = WorkspaceId::from_bytes([60; 16]);
        let alice = WorkspaceId::from_bytes([61; 16]);
        let bob = WorkspaceId::from_bytes([62; 16]);
        let mut identity = loom_core::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        identity
            .add_principal(alice, "alice", loom_core::PrincipalKind::User)
            .unwrap();
        identity
            .add_principal(bob, "bob", loom_core::PrincipalKind::User)
            .unwrap();
        identity
            .set_passphrase(alice, "alice", b"abcdefgh")
            .unwrap();
        identity.set_passphrase(bob, "bob", b"ijklmnop").unwrap();
        identity
            .authenticate_passphrase(alice, "alice", "alice-session")
            .unwrap();
        identity
            .authenticate_passphrase(bob, "bob", "bob-session")
            .unwrap();
        let mut acl = loom_core::AclStore::new();
        acl.grant(loom_core::AclGrant {
            subject: loom_core::AclSubject::Everyone,
            workspace: Some(workspace),
            domain: Some(AclDomain::Pages),
            ref_glob: None,
            scopes: vec![loom_core::AclScope::All],
            rights: [AclRight::Read, AclRight::Write].into_iter().collect(),
            effect: loom_core::AclEffect::Allow,
            predicate: None,
        })
        .unwrap();
        loom.set_identity_store(identity);
        loom.set_acl_store(acl);
    }

    #[test]
    fn page_get_authenticated_principal_sees_own_draft_after_update() {
        let path = test_store_path("page-principal-own-draft");
        let mut loom = open_loom(&path).unwrap();
        let workspace = loom
            .registry_mut()
            .create(
                FacetKind::Document,
                Some("pages-principal-own"),
                WorkspaceId::from_bytes([63; 16]),
            )
            .unwrap();
        let workspace_id = workspace.to_string();
        seed_page_workspace(&mut loom, workspace, &workspace_id);
        install_authenticated_pages_sessions(&mut loom, workspace);

        loom.set_session("alice-session");
        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "alice draft",
            10,
            None,
        )
        .unwrap();
        let page = get_page(&loom, workspace, &workspace_id, "page-1")
            .unwrap()
            .unwrap();
        assert_eq!(page.status, "draft");
        assert_eq!(page.draft_body_text.as_deref(), Some("alice draft\n"));

        save_loom(&mut loom).unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn page_get_different_principal_does_not_see_another_draft() {
        let path = test_store_path("page-principal-isolated-draft");
        let mut loom = open_loom(&path).unwrap();
        let workspace = loom
            .registry_mut()
            .create(
                FacetKind::Document,
                Some("pages-principal-isolated"),
                WorkspaceId::from_bytes([64; 16]),
            )
            .unwrap();
        let workspace_id = workspace.to_string();
        seed_page_workspace(&mut loom, workspace, &workspace_id);
        install_authenticated_pages_sessions(&mut loom, workspace);

        loom.set_session("alice-session");
        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "alice private draft",
            10,
            None,
        )
        .unwrap();
        loom.set_session("bob-session");
        let page = get_page(&loom, workspace, &workspace_id, "page-1")
            .unwrap()
            .unwrap();
        assert_eq!(page.status, "empty");
        assert_eq!(page.draft_body_text, None);

        save_loom(&mut loom).unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn page_get_workspace_fallback_sees_unauthenticated_draft() {
        let path = test_store_path("page-principal-workspace-fallback");
        let mut loom = open_loom(&path).unwrap();
        let workspace = loom
            .registry_mut()
            .create(
                FacetKind::Document,
                Some("pages-principal-fallback"),
                WorkspaceId::from_bytes([65; 16]),
            )
            .unwrap();
        let workspace_id = workspace.to_string();
        seed_page_workspace(&mut loom, workspace, &workspace_id);

        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "workspace draft",
            10,
            None,
        )
        .unwrap();
        let page = get_page(&loom, workspace, &workspace_id, "page-1")
            .unwrap()
            .unwrap();
        assert_eq!(page.status, "draft");
        assert_eq!(page.draft_body_text.as_deref(), Some("workspace draft\n"));

        save_loom(&mut loom).unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn page_list_and_get_use_same_principal_selection() {
        let path = test_store_path("page-principal-list-get");
        let mut loom = open_loom(&path).unwrap();
        let workspace = loom
            .registry_mut()
            .create(
                FacetKind::Document,
                Some("pages-principal-list"),
                WorkspaceId::from_bytes([66; 16]),
            )
            .unwrap();
        let workspace_id = workspace.to_string();
        seed_page_workspace(&mut loom, workspace, &workspace_id);
        install_authenticated_pages_sessions(&mut loom, workspace);

        loom.set_session("alice-session");
        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "list draft",
            10,
            None,
        )
        .unwrap();
        let listed = list_pages(&loom, workspace, &workspace_id).unwrap();
        let got = get_page(&loom, workspace, &workspace_id, "page-1")
            .unwrap()
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].status, got.status);
        assert_eq!(listed[0].draft_body_text, got.draft_body_text);
        assert_eq!(got.draft_body_text.as_deref(), Some("list draft\n"));

        loom.set_session("bob-session");
        let listed = list_pages(&loom, workspace, &workspace_id).unwrap();
        let got = get_page(&loom, workspace, &workspace_id, "page-1")
            .unwrap()
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].status, got.status);
        assert_eq!(listed[0].draft_body_text, got.draft_body_text);
        assert_eq!(got.status, "empty");
        assert_eq!(got.draft_body_text, None);

        save_loom(&mut loom).unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn page_owner_publication_preserves_ticket_profile_control_state() {
        let path = test_store_path("cross-facet-owner-state");
        let mut loom = open_loom(&path).unwrap();
        let workspace = loom
            .registry_mut()
            .create(
                FacetKind::Document,
                Some("cross-facet"),
                WorkspaceId::from_bytes([47; 16]),
            )
            .unwrap();
        let workspace_id = workspace.to_string();
        loom_tickets::create_project(
            &mut loom,
            workspace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket_state_key = loom_tickets::ticket_profile_state_key(&workspace_id).unwrap();
        assert!(
            loom.store()
                .control_get(&ticket_state_key)
                .unwrap()
                .is_some()
        );

        create_space(&mut loom, workspace, &workspace_id, "space", "Space", None).unwrap();

        assert!(
            loom.store()
                .control_get(&ticket_state_key)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            loom_tickets::TicketProfileReader::open(&loom, workspace, &workspace_id)
                .unwrap()
                .unwrap()
                .projects()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn page_update_and_publish_keep_operation_revision_and_reference_side_effects() {
        let path = test_store_path("page-side-effects");
        let mut loom = open_loom(&path).unwrap();
        let workspace = loom
            .registry_mut()
            .create(
                FacetKind::Document,
                Some("pages-test"),
                WorkspaceId::from_bytes([42; 16]),
            )
            .unwrap();
        let workspace_id = workspace.to_string();

        create_space(&mut loom, workspace, &workspace_id, "space", "Space", None).unwrap();
        create_page(
            &mut loom,
            workspace,
            PageCreateRequest {
                workspace_id: &workspace_id,
                page_id: "page-1",
                space_id: "space",
                parent_page_id: None,
                title: "Page",
                expected_root: None,
            },
        )
        .unwrap();
        let operation_log_key = page_profile_operation_log_key(&workspace_id).unwrap();
        assert!(
            loom.store()
                .retained_history_head(&operation_log_key)
                .unwrap()
                > 0
        );
        assert!(
            !page_operation_log(loom.store(), &workspace_id)
                .unwrap()
                .records
                .is_empty()
        );
        let before_update_generation = loom.store().mutable_overlay_generation().unwrap();
        let before_update_fsync = loom.store().group_commit_diagnostics().unwrap().fsync_count;
        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "See !ticket:MX-420.",
            10,
            None,
        )
        .unwrap();
        let after_update_generation = loom.store().mutable_overlay_generation().unwrap();
        let after_update_fsync = loom.store().group_commit_diagnostics().unwrap().fsync_count;
        assert_eq!(
            after_update_generation.as_u64(),
            before_update_generation.as_u64() + 1
        );
        assert_eq!(after_update_fsync, before_update_fsync + 2);
        let before_publish_generation = after_update_generation;
        let before_publish_fsync = after_update_fsync;
        let published =
            publish_page(&mut loom, workspace, &workspace_id, "page-1", 20, None).unwrap();
        let after_publish_generation = loom.store().mutable_overlay_generation().unwrap();
        let after_publish_fsync = loom.store().group_commit_diagnostics().unwrap().fsync_count;
        assert_eq!(
            after_publish_generation.as_u64(),
            before_publish_generation.as_u64() + 1
        );
        assert_eq!(after_publish_fsync, before_publish_fsync + 2);

        let cursor =
            OperationChangeCursor::new(page_operation_cursor_scope(&workspace_id), 1).unwrap();
        let changes = operation_changes(&loom, workspace, &cursor, 10).unwrap();
        let kinds = changes
            .events
            .iter()
            .map(|event| event.operation_kind.as_str())
            .collect::<Vec<_>>();
        assert!(kinds.contains(&"page.updated"));
        assert!(kinds.contains(&"page.published"));

        let revisions = load_current_revision_index(&loom, workspace, &workspace_id).unwrap();
        assert!(revisions.latest("page:draft:page-1").is_some());
        assert!(revisions.latest("page:page-1").is_some());
        assert!(
            loom_reference::load_index(&loom, workspace)
                .unwrap()
                .is_some()
        );
        drop(loom);

        let mut loom = open_loom(&path).unwrap();
        let page = get_page(&loom, workspace, &workspace_id, "page-1")
            .unwrap()
            .unwrap();
        assert_eq!(page.current_revision, Some(1));
        let changes = operation_changes(&loom, workspace, &cursor, 10).unwrap();
        let kinds = changes
            .events
            .iter()
            .map(|event| event.operation_kind.as_str())
            .collect::<Vec<_>>();
        assert!(kinds.contains(&"page.updated"));
        assert!(kinds.contains(&"page.published"));
        let revisions = load_current_revision_index(&loom, workspace, &workspace_id).unwrap();
        assert!(revisions.latest("page:draft:page-1").is_some());
        assert!(revisions.latest("page:page-1").is_some());
        assert!(
            loom_reference::load_index(&loom, workspace)
                .unwrap()
                .is_some()
        );

        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "Second draft.",
            30,
            None,
        )
        .unwrap();
        let before_stale_generation = loom.store().mutable_overlay_generation().unwrap();
        let before_stale_fsync = loom.store().group_commit_diagnostics().unwrap().fsync_count;
        let stale_error = publish_page(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            40,
            Some(&published.profile_root),
        )
        .unwrap_err();
        assert_eq!(stale_error.code, Code::Conflict);
        assert_eq!(
            loom.store().mutable_overlay_generation().unwrap(),
            before_stale_generation
        );
        assert_eq!(
            loom.store().group_commit_diagnostics().unwrap().fsync_count,
            before_stale_fsync
        );
        let page = get_page(&loom, workspace, &workspace_id, "page-1")
            .unwrap()
            .unwrap();
        assert_eq!(page.current_revision, Some(1));
        assert_eq!(page.draft_body_text.as_deref(), Some("Second draft.\n"));

        save_loom(&mut loom).unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn page_revision_index_survives_reopen_between_publish_operations() {
        let path = test_store_path("page-revision-reopen");
        let mut loom = open_loom(&path).unwrap();
        let workspace = loom
            .registry_mut()
            .create(
                FacetKind::Document,
                Some("pages-reopen"),
                WorkspaceId::from_bytes([43; 16]),
            )
            .unwrap();
        let workspace_id = workspace.to_string();
        create_space(&mut loom, workspace, &workspace_id, "space", "Space", None).unwrap();
        create_page(
            &mut loom,
            workspace,
            PageCreateRequest {
                workspace_id: &workspace_id,
                page_id: "page-1",
                space_id: "space",
                parent_page_id: None,
                title: "Page",
                expected_root: None,
            },
        )
        .unwrap();
        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "first",
            10,
            None,
        )
        .unwrap();
        publish_page(&mut loom, workspace, &workspace_id, "page-1", 20, None).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let mut loom = open_loom(&path).unwrap();
        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "second",
            30,
            None,
        )
        .unwrap();
        let published =
            publish_page(&mut loom, workspace, &workspace_id, "page-1", 40, None).unwrap();
        assert_eq!(published.revision, Some(2));
        let revisions = load_current_revision_index(&loom, workspace, &workspace_id).unwrap();
        assert_eq!(
            revisions.latest("page:page-1").map(|entry| entry.revision),
            Some(2)
        );
        save_loom(&mut loom).unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn page_publish_preserves_unrelated_mutable_overlay_state() {
        let path = test_store_path("page-publish-preserves-overlay");
        let mut loom = open_loom(&path).unwrap();
        let workspace = loom
            .registry_mut()
            .create(
                FacetKind::Document,
                Some("pages-preserves-overlay"),
                WorkspaceId::from_bytes([45; 16]),
            )
            .unwrap();
        let workspace_id = workspace.to_string();
        create_space(&mut loom, workspace, &workspace_id, "space", "Space", None).unwrap();
        create_page(
            &mut loom,
            workspace,
            PageCreateRequest {
                workspace_id: &workspace_id,
                page_id: "page-1",
                space_id: "space",
                parent_page_id: None,
                title: "Page",
                expected_root: None,
            },
        )
        .unwrap();
        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "body",
            10,
            None,
        )
        .unwrap();

        let unrelated = OverlayKey::from_segments([
            b"workspace".as_slice(),
            workspace_id.as_bytes(),
            b"lanes".as_slice(),
            b"lane".as_slice(),
            b"unrelated".as_slice(),
            b"v1".as_slice(),
        ])
        .unwrap();
        loom.store()
            .put_mutable_overlay_value(unrelated.clone(), b"unrelated lane".to_vec())
            .unwrap();
        let current = loom
            .store()
            .mutable_overlay_current_entry(&unrelated)
            .unwrap()
            .unwrap();
        loom.mutable_overlay_mut()
            .synchronize_current_entry(current)
            .unwrap();

        publish_page(&mut loom, workspace, &workspace_id, "page-1", 20, None).unwrap();

        let current = loom
            .mutable_overlay()
            .current_entry(&unrelated)
            .expect("unrelated current entry");
        assert_eq!(current.payload, b"unrelated lane");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn interrupted_publish_keeps_revision_and_references_at_prior_state() {
        let path = test_store_path("page-publish-atomic");
        let mut loom = open_loom(&path).unwrap();
        let workspace = loom
            .registry_mut()
            .create(
                FacetKind::Document,
                Some("pages-atomic"),
                WorkspaceId::from_bytes([44; 16]),
            )
            .unwrap();
        let workspace_id = workspace.to_string();
        create_space(&mut loom, workspace, &workspace_id, "space", "Space", None).unwrap();
        create_page(
            &mut loom,
            workspace,
            PageCreateRequest {
                workspace_id: &workspace_id,
                page_id: "page-1",
                space_id: "space",
                parent_page_id: None,
                title: "Page",
                expected_root: None,
            },
        )
        .unwrap();
        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "See !ticket:MX-OLD.",
            10,
            None,
        )
        .unwrap();
        publish_page(&mut loom, workspace, &workspace_id, "page-1", 20, None).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let mut loom = open_loom(&path).unwrap();
        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "See !ticket:MX-NEW.",
            30,
            None,
        )
        .unwrap();
        let generation_before_interruption = loom.store().mutable_overlay_generation().unwrap();
        let fsync_before_interruption =
            loom.store().group_commit_diagnostics().unwrap().fsync_count;
        let enumerations_before = loom.store().mutable_overlay_enumeration_count();
        let engine_io_before = loom.engine_state_io_counts();
        let error = publish_page_with_before_commit(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            40,
            None,
            |_| Err(LoomError::new(Code::Internal, "interrupted before commit")),
        )
        .unwrap_err();
        assert_eq!(error.code, Code::Internal);
        assert_eq!(
            loom.store().mutable_overlay_generation().unwrap(),
            generation_before_interruption
        );
        assert_eq!(
            loom.store().group_commit_diagnostics().unwrap().fsync_count,
            fsync_before_interruption
        );
        assert_eq!(
            loom.store().mutable_overlay_enumeration_count(),
            enumerations_before
        );
        let engine_io_after_interruption = loom.engine_state_io_counts();
        assert_eq!(
            engine_io_after_interruption.full_exports,
            engine_io_before.full_exports + 1
        );
        assert_eq!(
            engine_io_after_interruption.full_imports,
            engine_io_before.full_imports
        );
        assert_eq!(engine_io_after_interruption.unrelated_section_rewrites, 0);
        assert_prior_published_page_state(&loom, workspace, &workspace_id);
        drop(loom);

        let mut loom = open_loom(&path).unwrap();
        assert_prior_published_page_state(&loom, workspace, &workspace_id);
        let source =
            ReferenceSource::new("pages", &workspace_id, "page-1", "published_body").unwrap();

        let enumerations_before = loom.store().mutable_overlay_enumeration_count();
        let engine_io_before = loom.engine_state_io_counts();
        publish_page(&mut loom, workspace, &workspace_id, "page-1", 50, None).unwrap();
        assert_eq!(
            loom.store().mutable_overlay_enumeration_count(),
            enumerations_before
        );
        let engine_io_after_publish = loom.engine_state_io_counts();
        assert_eq!(
            engine_io_after_publish.full_exports,
            engine_io_before.full_exports + 1
        );
        assert_eq!(
            engine_io_after_publish.full_imports,
            engine_io_before.full_imports + 1
        );
        assert_eq!(engine_io_after_publish.unrelated_section_rewrites, 0);
        let page = get_page(&loom, workspace, &workspace_id, "page-1")
            .unwrap()
            .unwrap();
        assert_eq!(page.current_revision, Some(2));
        let references = loom_reference::references_from(&loom, workspace, &source).unwrap();
        assert!(
            references
                .iter()
                .any(|edge| edge.target == EntityRef::parse("ticket:MX-NEW").unwrap())
        );
        assert!(
            references
                .iter()
                .all(|edge| edge.target != EntityRef::parse("ticket:MX-OLD").unwrap())
        );
        let projected = graph_edges(&loom, workspace, loom_reference::REFERENCE_GRAPH).unwrap();
        assert!(
            projected
                .iter()
                .any(|(_, edge)| edge.dst == "ticket:MX-NEW")
        );
        assert!(
            projected
                .iter()
                .all(|(_, edge)| edge.dst != "ticket:MX-OLD")
        );

        save_loom(&mut loom).unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn page_publish_excludes_unpublished_live_reference_state() {
        let path = test_store_path("page-publish-snapshot-references");
        let mut loom = open_loom(&path).unwrap();
        let workspace = loom
            .registry_mut()
            .create(
                FacetKind::Document,
                Some("pages-snapshot-references"),
                WorkspaceId::from_bytes([45; 16]),
            )
            .unwrap();
        let workspace_id = workspace.to_string();
        create_space(&mut loom, workspace, &workspace_id, "space", "Space", None).unwrap();
        create_page(
            &mut loom,
            workspace,
            PageCreateRequest {
                workspace_id: &workspace_id,
                page_id: "page-1",
                space_id: "space",
                parent_page_id: None,
                title: "Page",
                expected_root: None,
            },
        )
        .unwrap();
        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "See !ticket:MX-OLD.",
            10,
            None,
        )
        .unwrap();
        publish_page(&mut loom, workspace, &workspace_id, "page-1", 20, None).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);

        let mut loom = open_loom(&path).unwrap();
        update_page_text(
            &mut loom,
            workspace,
            &workspace_id,
            "page-1",
            "See !ticket:MX-NEW.",
            30,
            None,
        )
        .unwrap();
        let live_source = ReferenceSource::new("document", "live-only", "ghost", "body").unwrap();
        let mut live_index = loom_reference::load_or_rebuild_index(&loom, workspace).unwrap();
        live_index
            .add_text_refs(live_source, "refers_to", "Unpublished !ticket:MX-LIVE.")
            .unwrap();
        loom_reference::save_index(&mut loom, workspace, &live_index).unwrap();
        loom_reference::project_reference_index_edges(&mut loom, workspace, &live_index).unwrap();

        let enumerations_before = loom.store().mutable_overlay_enumeration_count();
        let engine_io_before = loom.engine_state_io_counts();
        publish_page(&mut loom, workspace, &workspace_id, "page-1", 40, None).unwrap();
        assert_eq!(
            loom.store().mutable_overlay_enumeration_count(),
            enumerations_before
        );
        let engine_io_after = loom.engine_state_io_counts();
        assert_eq!(
            engine_io_after.full_exports,
            engine_io_before.full_exports + 1
        );
        assert_eq!(
            engine_io_after.full_imports,
            engine_io_before.full_imports + 1
        );
        assert_eq!(engine_io_after.unrelated_section_rewrites, 0);
        assert_reference_snapshot_result(&loom, workspace, &workspace_id);
        drop(loom);

        let loom = open_loom(&path).unwrap();
        assert_reference_snapshot_result(&loom, workspace, &workspace_id);
        drop(loom);
        let _ = std::fs::remove_file(&path);
    }

    fn assert_reference_snapshot_result(
        loom: &Loom<FileStore>,
        workspace: WorkspaceId,
        workspace_id: &str,
    ) {
        let index = loom_reference::load_index(loom, workspace)
            .unwrap()
            .expect("reference index");
        let page_source =
            ReferenceSource::new("pages", workspace_id, "page-1", "published_body").unwrap();
        let page_edges = index.outbound(&page_source);
        assert!(
            page_edges
                .iter()
                .any(|edge| edge.target == EntityRef::parse("ticket:MX-NEW").unwrap())
        );
        assert!(
            index
                .edges()
                .iter()
                .all(|edge| edge.target != EntityRef::parse("ticket:MX-LIVE").unwrap())
        );
        let projected = graph_edges(loom, workspace, loom_reference::REFERENCE_GRAPH).unwrap();
        assert!(
            projected
                .iter()
                .any(|(_, edge)| edge.dst == "ticket:MX-NEW")
        );
        assert!(
            projected
                .iter()
                .all(|(_, edge)| edge.dst != "ticket:MX-LIVE")
        );
    }
}
