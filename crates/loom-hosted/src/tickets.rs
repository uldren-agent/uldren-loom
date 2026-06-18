use loom_core::error::Result;
use loom_core::workspace::WorkspaceId;
use loom_core::{Digest, Loom};
use loom_store::FileStore;
use loom_tickets::{
    TicketComment, TicketCommentDeleteRequest, TicketCommentRequest, TicketCommentUpdateRequest,
    TicketCreateRequest, TicketDeleteRequest, TicketHistoryRecord, TicketProjectSummary,
    TicketProjectionProfile, TicketRelationKind, TicketRelationRemoveRequest,
    TicketRelationRequest, TicketRelationSummary, TicketSummary, TicketUpdateFieldsRequest,
    TicketUpdateRequest, normalize_ticket_delete_fields_for_projection,
    normalize_ticket_fields_for_projection, parse_ticket_projection,
};
use serde_json::Value;

pub struct HostedTicketProjectWrite<'a> {
    pub workspace_id: &'a str,
    pub project_id: &'a str,
    pub key_prefix: &'a str,
    pub name: Option<&'a str>,
    pub expected_root: Option<&'a str>,
}

pub fn project_create(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedTicketProjectWrite<'_>,
) -> Result<TicketProjectSummary> {
    loom_tickets::create_project(
        loom,
        workspace,
        input.workspace_id,
        input.project_id,
        input.key_prefix,
        input.name.unwrap_or(input.project_id),
        input.expected_root,
    )
}

pub fn project_rekey(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedTicketProjectWrite<'_>,
) -> Result<TicketProjectSummary> {
    loom_tickets::rekey_project(
        loom,
        workspace,
        input.workspace_id,
        input.project_id,
        input.key_prefix,
        input.expected_root,
    )
}

pub fn project_settings_get(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    project_id: &str,
) -> Result<Option<TicketProjectSummary>> {
    loom_tickets::get_project(loom, workspace, workspace_id, project_id)
}

pub struct HostedTicketProjectSettings<'a> {
    pub workspace_id: &'a str,
    pub project_id: &'a str,
    pub default_projection: Option<TicketProjectionProfile>,
    pub enable_projections: &'a [TicketProjectionProfile],
    pub disable_projections: &'a [TicketProjectionProfile],
    pub actor_enforcement: Option<loom_tickets::TicketLifecycleAuthorizationPolicy>,
    pub project_owner_principal: Option<&'a str>,
    pub clear_project_owner_principal: bool,
    pub acceptance_authorities: Option<&'a [String]>,
    pub acceptance_evidence_enforcement: Option<bool>,
    pub required_acceptance_evidence_keys: Option<&'a [loom_tickets::TicketAcceptanceEvidenceKey]>,
    pub expected_root: Option<&'a str>,
}

pub fn project_settings_set(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedTicketProjectSettings<'_>,
) -> Result<TicketProjectSummary> {
    loom_tickets::set_project_settings(
        loom,
        workspace,
        loom_tickets::TicketProjectSettingsRequest {
            workspace_id: input.workspace_id,
            project_id: input.project_id,
            default_projection: input.default_projection,
            enable_projections: input.enable_projections,
            disable_projections: input.disable_projections,
            actor_enforcement: input.actor_enforcement,
            project_owner_principal: input.project_owner_principal,
            clear_project_owner_principal: input.clear_project_owner_principal,
            acceptance_authorities: input.acceptance_authorities,
            acceptance_evidence_enforcement: input.acceptance_evidence_enforcement,
            required_acceptance_evidence_keys: input.required_acceptance_evidence_keys,
            owner_contract_summary: None,
            owner_contract_details: None,
            worker_contract_summary: None,
            worker_contract_details: None,
            expected_root: input.expected_root,
        },
    )
}

pub struct HostedTicketCreate<'a> {
    pub workspace_id: &'a str,
    pub project_id: &'a str,
    pub ticket_type: &'a str,
    pub projection: Option<&'a str>,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub fields: &'a Value,
    pub policy_labels: &'a [String],
    pub expected_root: Option<&'a str>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub fn create(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedTicketCreate<'_>,
) -> Result<TicketSummary> {
    let projection = parse_ticket_projection(input.projection)?;
    let fields = normalize_ticket_fields_for_projection(input.fields, projection)?;
    let ticket = loom_tickets::create_ticket(
        loom,
        workspace,
        TicketCreateRequest {
            workspace_id: input.workspace_id,
            project_id: input.project_id,
            ticket_type: input.ticket_type,
            external_source: input.external_source,
            external_id: input.external_id,
            fields: &fields,
            policy_labels: input.policy_labels,
            expected_root: input.expected_root,
        },
    )?;
    update_ticket_references(loom, workspace, &ticket)?;
    Ok(ticket)
}

pub fn update_fields(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
    fields: &Value,
    expected_root: Option<&str>,
) -> Result<TicketSummary> {
    let ticket = loom_tickets::update_ticket_fields(
        loom,
        workspace,
        TicketUpdateFieldsRequest {
            workspace_id,
            ticket_id,
            fields,
            expected_root,
        },
    )?;
    update_ticket_references(loom, workspace, &ticket)?;
    Ok(ticket)
}

pub struct HostedTicketUpdate<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub projection: Option<&'a str>,
    pub set_fields: Option<&'a Value>,
    pub delete_fields: &'a [String],
    pub action: Option<loom_tickets::TicketLifecycleAction>,
    pub target_status: Option<&'a str>,
    pub observed_source_status: Option<&'a str>,
    pub observed_workflow_version: Option<&'a str>,
    pub assignee: Option<&'a str>,
    pub expected_root: Option<&'a str>,
    pub comment: Option<loom_tickets::TicketUpdateCommentRequest<'a>>,
    pub comments: &'a [loom_tickets::TicketUpdateCommentRequest<'a>],
    pub relation_sets: &'a [loom_tickets::TicketUpdateRelationSetRequest<'a>],
    pub relation_removes: &'a [loom_tickets::TicketUpdateRelationRemoveRequest<'a>],
}

pub fn update(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedTicketUpdate<'_>,
) -> Result<TicketSummary> {
    let projection = parse_ticket_projection(input.projection)?;
    let set_fields = input
        .set_fields
        .map(|fields| normalize_ticket_fields_for_projection(fields, projection))
        .transpose()?;
    let delete_fields =
        normalize_ticket_delete_fields_for_projection(input.delete_fields, projection);
    let ticket = loom_tickets::update_ticket(
        loom,
        workspace,
        TicketUpdateRequest {
            workspace_id: input.workspace_id,
            ticket_id: input.ticket_id,
            set_fields: set_fields.as_ref(),
            delete_fields: &delete_fields,
            action: input.action,
            target_status: input.target_status,
            observed_source_status: input.observed_source_status,
            observed_workflow_version: input.observed_workflow_version,
            assignee: input.assignee,
            expected_root: input.expected_root,
            comment: input.comment,
            comments: input.comments,
            relation_sets: input.relation_sets,
            relation_removes: input.relation_removes,
        },
    )?;
    update_ticket_references(loom, workspace, &ticket)?;
    Ok(ticket)
}

pub struct HostedTicketDelete<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub expected_root: Option<&'a str>,
}

pub fn delete(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedTicketDelete<'_>,
) -> Result<TicketSummary> {
    let ticket = loom_tickets::delete_ticket(
        loom,
        workspace,
        TicketDeleteRequest {
            workspace_id: input.workspace_id,
            ticket_id: input.ticket_id,
            expected_root: input.expected_root,
        },
    )?;
    update_ticket_references(loom, workspace, &ticket)?;
    Ok(ticket)
}

pub fn comments(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
) -> Result<Vec<TicketComment>> {
    loom_tickets::list_ticket_comments(loom, workspace, workspace_id, ticket_id)
}

pub struct HostedTicketCommentAdd<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub comment_id: Option<&'a str>,
    pub comment_type: Option<&'a str>,
    pub body: &'a str,
    pub evidence: Option<loom_tickets::TicketCommentEvidence>,
    pub expected_root: Option<&'a str>,
}

pub fn comment_add(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedTicketCommentAdd<'_>,
) -> Result<TicketSummary> {
    let ticket = loom_tickets::add_ticket_comment(
        loom,
        workspace,
        TicketCommentRequest {
            workspace_id: input.workspace_id,
            ticket_id: input.ticket_id,
            comment_id: input.comment_id,
            comment_type: input.comment_type,
            body: input.body,
            evidence: input.evidence,
            expected_root: input.expected_root,
        },
    )?;
    update_ticket_references(loom, workspace, &ticket)?;
    Ok(ticket)
}

pub struct HostedTicketCommentUpdate<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub comment_id: &'a str,
    pub comment_type: Option<&'a str>,
    pub body: Option<&'a str>,
    pub evidence: Option<Option<loom_tickets::TicketCommentEvidence>>,
    pub expected_root: Option<&'a str>,
}

pub fn comment_update(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedTicketCommentUpdate<'_>,
) -> Result<TicketSummary> {
    let ticket = loom_tickets::update_ticket_comment(
        loom,
        workspace,
        TicketCommentUpdateRequest {
            workspace_id: input.workspace_id,
            ticket_id: input.ticket_id,
            comment_id: input.comment_id,
            comment_type: input.comment_type,
            body: input.body,
            evidence: input.evidence,
            expected_root: input.expected_root,
        },
    )?;
    update_ticket_references(loom, workspace, &ticket)?;
    Ok(ticket)
}

pub struct HostedTicketCommentDelete<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub comment_id: &'a str,
    pub expected_root: Option<&'a str>,
}

pub fn comment_delete(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedTicketCommentDelete<'_>,
) -> Result<TicketSummary> {
    let ticket = loom_tickets::delete_ticket_comment(
        loom,
        workspace,
        TicketCommentDeleteRequest {
            workspace_id: input.workspace_id,
            ticket_id: input.ticket_id,
            comment_id: input.comment_id,
            expected_root: input.expected_root,
        },
    )?;
    update_ticket_references(loom, workspace, &ticket)?;
    Ok(ticket)
}

pub struct HostedTicketRelationWrite<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub relation_id: Option<&'a str>,
    pub kind: TicketRelationKind,
    pub target_id: &'a str,
    pub expected_root: Option<&'a str>,
}

pub fn relation_set(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedTicketRelationWrite<'_>,
) -> Result<TicketRelationSummary> {
    loom_tickets::add_ticket_relation(
        loom,
        workspace,
        TicketRelationRequest {
            workspace_id: input.workspace_id,
            ticket_id: input.ticket_id,
            relation_id: input.relation_id,
            kind: input.kind,
            target_id: input.target_id,
            expected_root: input.expected_root,
        },
    )
}

pub struct HostedTicketRelationRemove<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub relation_id: &'a str,
    pub expected_root: Option<&'a str>,
}

pub fn relation_remove(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    input: HostedTicketRelationRemove<'_>,
) -> Result<TicketRelationSummary> {
    loom_tickets::remove_ticket_relation(
        loom,
        workspace,
        TicketRelationRemoveRequest {
            workspace_id: input.workspace_id,
            ticket_id: input.ticket_id,
            relation_id: input.relation_id,
            expected_root: input.expected_root,
        },
    )
}

fn update_ticket_references(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    ticket: &TicketSummary,
) -> Result<()> {
    loom_tickets::update_ticket_field_references(
        loom,
        workspace,
        &ticket.workspace_id,
        &ticket.ticket_id,
        &ticket.fields,
    )?;
    if let Some(operation_id) = ticket.operation_id.as_deref() {
        loom_tickets::enqueue_ticket_reference_candidates(
            loom,
            workspace,
            loom_tickets::TicketReferenceCandidateRequest {
                workspace_id: &ticket.workspace_id,
                ticket_id: &ticket.ticket_id,
                operation_id,
                source_root: Digest::parse(&ticket.profile_root)?,
                fields: &ticket.fields,
                now_ms: now_ms(),
            },
        )?;
    }
    Ok(())
}

pub fn get(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
    projection: Option<&str>,
) -> Result<Option<TicketSummary>> {
    loom_tickets::get_ticket_with_projection(
        loom,
        workspace,
        workspace_id,
        ticket_id,
        parse_ticket_projection(projection)?,
    )
}

pub fn history(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: Option<&str>,
) -> Result<Vec<TicketHistoryRecord>> {
    loom_tickets::history(loom, workspace, workspace_id, ticket_id)
}
