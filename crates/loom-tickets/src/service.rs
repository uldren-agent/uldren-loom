use std::collections::{BTreeMap, BTreeSet};

use crate::{
    APP_ID, BoardCardPlacement, BoardColumn, BoardMode, BoardScope, BoardStatus, BoardSwimlane,
    ExternalTicketIdentity, IndexedTicketProfile, NORMALIZED_TICKET_STATUSES,
    TICKET_COMPACT_TEXT_MAX_BYTES, TICKET_DEFAULT_FIELD_TEXT_MAX_BYTES, TICKET_RICH_TEXT_MAX_BYTES,
    Ticket, TicketAcceptanceEvidenceKey, TicketAttachment, TicketBoard, TicketComment,
    TicketCommentEvidence, TicketCustomFieldDefinition, TicketFieldCardinality, TicketFieldValue,
    TicketInput, TicketLifecycleAction, TicketLifecycleAuthorizationPolicy, TicketOperationRecord,
    TicketProfileReader, TicketProject, TicketProjectionProfile, TicketProjectionRequestContext,
    TicketProjectionSelection, TicketProjectionSelectionSource, TicketRelation, TicketRelationKind,
    TicketRelationTargetType, TicketType, TransitionOperation, WorkflowDefinition,
    WorkflowValidationContext, WorkflowValidationRecord, WorkflowValidationState,
    validate_ticket_comment_type, validate_transition,
};
use loom_core::delivery::{DeliveryEnvelope, DeliveryProduceRequest, delivery_produce};
use loom_core::error::{Code, LoomError, Result};
use loom_core::graph::{
    GraphValue, graph_edges, graph_in_edges, graph_remove_edge, graph_upsert_edge,
    graph_upsert_node,
};
#[cfg(test)]
use loom_core::workspace::FacetKind;
use loom_core::workspace::WorkspaceId;
use loom_core::{AclDomain, AclRight, Digest, IdentityStore, Loom};
use loom_store::FileStore;
use loom_substrate::changes::{OperationChangeBatch, OperationChangeCursor};
use loom_substrate::facilities::{DateTimeValue, FieldDefinition, FieldType};
use loom_substrate::refs::{
    EntityRef, MarkdownReferenceKind, ReferenceEdge, ReferenceIndex, ReferenceSource,
    UnresolvedReference, extract_markdown_reference_candidates,
};
use loom_substrate::versioning::{
    BodyRef, ProfileRevisionUpdate, ProfileTransaction, ProfileTransactionState, RevisionIndex,
};
use loom_substrate::{ActorKind, OperationEnvelope, OperationEnvelopeInput};
use serde::Serialize;
use serde_json::json;
use serde_json::{Map, Value};

const REVISION_INDEX_DIR: &str = ".loom/substrate/revisions";

fn revision_index_path(scope_id: &str) -> Result<String> {
    loom_substrate::view::validate_view_id(scope_id)?;
    Ok(format!("{REVISION_INDEX_DIR}/{scope_id}.lri"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct TicketProjectSummary {
    pub workspace_id: String,
    pub project_id: String,
    pub key_prefix: String,
    pub name: String,
    pub next_ticket_number: u64,
    pub default_projection: String,
    pub enabled_projections: Vec<String>,
    pub lifecycle_authorization_policy: String,
    pub project_owner_principal: Option<String>,
    pub acceptance_authorities: Vec<String>,
    pub acceptance_evidence_enforcement: bool,
    pub required_acceptance_evidence_keys: Vec<String>,
    pub contracts: TicketProjectContractsSummary,
    pub active_workflow_version: Option<String>,
    pub profile_root: String,
    pub operation_id: String,
    pub sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketProjectContractsSummary {
    pub note: String,
    pub owner: TicketProjectContractSummary,
    pub worker: TicketProjectContractSummary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketProjectContractSummary {
    pub summary: String,
    pub details: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TicketSummary {
    pub workspace_id: String,
    pub ticket_id: String,
    pub project_id: String,
    pub primary_key: String,
    pub ticket_type: String,
    pub projection_profile: String,
    pub projection_kind: String,
    pub projection_source: String,
    pub projection_selection_source: String,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
    pub fields: BTreeMap<String, Value>,
    pub policy_labels: Vec<String>,
    pub relations: Vec<TicketRelationCompact>,
    pub relation_rollup: TicketRelationRollup,
    pub depends_on: Vec<String>,
    pub blocks: Vec<String>,
    pub comments: Vec<TicketCommentCompact>,
    pub profile_root: String,
    pub operation_id: Option<String>,
    pub sequence: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TicketCommentCompact {
    pub comment_id: String,
    pub comment_type: String,
    pub author_principal: String,
    pub created_at_ms: u64,
    pub updated_at_ms: Option<u64>,
    pub redacted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TicketHistoryRecord {
    pub sequence: u64,
    pub operation_id: String,
    pub operation_kind: String,
    pub target_entity_id: Option<String>,
    pub comments: Vec<TicketCommentCompact>,
    pub envelope: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BoardColumnSummary {
    pub column_id: String,
    pub name: String,
    pub mapped_statuses: Vec<String>,
    pub wip_limit: Option<u32>,
    pub hidden: bool,
    pub rank: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BoardSwimlaneSummary {
    pub swimlane_id: String,
    pub name: String,
    pub predicate: Option<String>,
    pub rank: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BoardCardPlacementSummary {
    pub board_id: String,
    pub ticket_id: String,
    pub column_id: String,
    pub rank_token: String,
    pub swimlane_id: Option<String>,
    pub updated_at: u64,
    pub updated_by: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BoardSummary {
    pub workspace_id: String,
    pub board_id: String,
    pub board_key: String,
    pub name: String,
    pub description: String,
    pub project_id: String,
    pub scope: Value,
    pub mode: String,
    pub columns: Vec<BoardColumnSummary>,
    pub swimlanes: Vec<BoardSwimlaneSummary>,
    pub card_display_fields: Vec<String>,
    pub owner_principal: Option<String>,
    pub coordinator_principal: Option<String>,
    pub board_status: String,
    pub cards: Vec<BoardCardPlacementSummary>,
    pub profile_root: String,
    pub operation_id: Option<String>,
    pub sequence: Option<u64>,
}

pub struct BoardCreateRequest<'a> {
    pub workspace_id: &'a str,
    pub board_id: &'a str,
    pub board_key: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub project_id: &'a str,
    pub scope: BoardScope,
    pub mode: BoardMode,
    pub columns: &'a [BoardColumn],
    pub swimlanes: &'a [BoardSwimlane],
    pub card_display_fields: &'a [String],
    pub owner_principal: Option<&'a str>,
    pub coordinator_principal: Option<&'a str>,
    pub updated_by: &'a str,
    pub expected_root: Option<&'a str>,
}

pub struct BoardUpdateRequest<'a> {
    pub workspace_id: &'a str,
    pub board_id: &'a str,
    pub board_key: Option<&'a str>,
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub scope: Option<BoardScope>,
    pub owner_principal: Option<Option<&'a str>>,
    pub coordinator_principal: Option<Option<&'a str>>,
    pub card_display_fields: Option<&'a [String]>,
    pub board_status: Option<BoardStatus>,
    pub updated_by: &'a str,
    pub expected_root: Option<&'a str>,
}

pub struct BoardColumnConfigureRequest<'a> {
    pub workspace_id: &'a str,
    pub board_id: &'a str,
    pub mode: Option<BoardMode>,
    pub columns: &'a [BoardColumn],
    pub swimlanes: &'a [BoardSwimlane],
    pub updated_by: &'a str,
    pub expected_root: Option<&'a str>,
}

pub struct BoardCardMoveRequest<'a> {
    pub workspace_id: &'a str,
    pub board_id: &'a str,
    pub ticket_id: &'a str,
    pub column_id: &'a str,
    pub rank_token: &'a str,
    pub swimlane_id: Option<&'a str>,
    pub updated_by: &'a str,
    pub expected_root: Option<&'a str>,
}

pub struct TicketFieldDefinitionWriteRequest<'a> {
    pub workspace_id: &'a str,
    pub project_id: &'a str,
    pub field_id: &'a str,
    pub key: &'a str,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub field_type: &'a str,
    pub option_set: Option<&'a str>,
    pub max_length: Option<u32>,
    pub required: bool,
    pub searchable: bool,
    pub orderable: bool,
    pub cardinality: TicketFieldCardinality,
    pub applicable_type_ids: &'a [String],
    pub expected_root: Option<&'a str>,
}

pub struct TicketFieldDefinitionRetireRequest<'a> {
    pub workspace_id: &'a str,
    pub project_id: &'a str,
    pub field_id: &'a str,
    pub expected_root: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TicketFieldCatalog {
    #[serde(rename = "projection")]
    pub projection_profile: String,
    pub operation: String,
    pub strict_unknown_fields: bool,
    pub custom_fields_source: String,
    pub fields: Vec<TicketFieldCatalogEntry>,
}

impl Serialize for TicketSummary {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ticket_summary_public_json(self).serialize(serializer)
    }
}

impl Serialize for TicketProjectSummary {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut object = Map::new();
        object.insert(
            "workspace_id".to_string(),
            Value::String(self.workspace_id.clone()),
        );
        object.insert(
            "project_id".to_string(),
            Value::String(self.project_id.clone()),
        );
        object.insert(
            "key_prefix".to_string(),
            Value::String(self.key_prefix.clone()),
        );
        object.insert("name".to_string(), Value::String(self.name.clone()));
        object.insert(
            "next_ticket_number".to_string(),
            Value::Number(self.next_ticket_number.into()),
        );
        object.insert(
            "default_projection".to_string(),
            Value::String(self.default_projection.clone()),
        );
        object.insert(
            "lifecycle_authorization_policy".to_string(),
            Value::String(self.lifecycle_authorization_policy.clone()),
        );
        object.insert(
            "project_owner_principal".to_string(),
            self.project_owner_principal
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        object.insert(
            "acceptance_authorities".to_string(),
            Value::Array(
                self.acceptance_authorities
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        object.insert(
            "acceptance_evidence_enforcement".to_string(),
            Value::Bool(self.acceptance_evidence_enforcement),
        );
        object.insert(
            "required_acceptance_evidence_keys".to_string(),
            Value::Array(
                self.required_acceptance_evidence_keys
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        object.insert(
            "contracts".to_string(),
            json!({
                "note": self.contracts.note,
                "owner": {
                    "summary": self.contracts.owner.summary,
                    "details": self.contracts.owner.details,
                },
                "worker": {
                    "summary": self.contracts.worker.summary,
                    "details": self.contracts.worker.details,
                },
            }),
        );
        object.insert(
            "active_workflow_version".to_string(),
            self.active_workflow_version
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        object.insert(
            "profile_root".to_string(),
            Value::String(self.profile_root.clone()),
        );
        object.insert(
            "operation_id".to_string(),
            Value::String(self.operation_id.clone()),
        );
        object.insert("sequence".to_string(), Value::Number(self.sequence.into()));
        Value::Object(object).serialize(serializer)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TicketFieldCatalogEntry {
    pub native_field: String,
    pub write_path: String,
    pub aliases: Vec<String>,
    pub field_type: String,
    pub cardinality: String,
    pub settable: bool,
    pub required_on_create: bool,
    pub required_on_update: bool,
    pub searchable: bool,
    pub orderable: bool,
    pub max_length: Option<u32>,
    pub enum_values: Vec<String>,
    pub write_semantics: String,
}

pub struct TicketCreateRequest<'a> {
    pub workspace_id: &'a str,
    pub project_id: &'a str,
    pub ticket_type: &'a str,
    pub external_source: Option<&'a str>,
    pub external_id: Option<&'a str>,
    pub fields: &'a Value,
    pub policy_labels: &'a [String],
    pub expected_root: Option<&'a str>,
}

pub struct TicketUpdateFieldsRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub fields: &'a Value,
    pub expected_root: Option<&'a str>,
}

pub struct TicketUpdateRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub set_fields: Option<&'a Value>,
    pub delete_fields: &'a [String],
    pub action: Option<TicketLifecycleAction>,
    pub target_status: Option<&'a str>,
    pub observed_source_status: Option<&'a str>,
    pub observed_workflow_version: Option<&'a str>,
    pub assignee: Option<&'a str>,
    pub expected_root: Option<&'a str>,
    pub comment: Option<TicketUpdateCommentRequest<'a>>,
    pub comments: &'a [TicketUpdateCommentRequest<'a>],
    pub relation_sets: &'a [TicketUpdateRelationSetRequest<'a>],
    pub relation_removes: &'a [TicketUpdateRelationRemoveRequest<'a>],
}

#[derive(Clone, Debug)]
pub struct TicketUpdateCommentRequest<'a> {
    pub comment_id: Option<&'a str>,
    pub comment_type: Option<&'a str>,
    pub body: &'a str,
    pub evidence: Option<TicketCommentEvidence>,
}

#[derive(Clone, Copy, Debug)]
pub struct TicketUpdateRelationSetRequest<'a> {
    pub relation_id: Option<&'a str>,
    pub kind: TicketRelationKind,
    pub target_id: &'a str,
}

#[derive(Clone, Copy, Debug)]
pub struct TicketUpdateRelationRemoveRequest<'a> {
    pub relation_id: &'a str,
}

pub struct TicketQueryRequest<'a> {
    pub workspace_id: &'a str,
    pub projection: Option<TicketProjectionProfile>,
    pub statuses: &'a [String],
    pub buckets: &'a [String],
    pub assignees: &'a [String],
    pub lane_owners: &'a [String],
    pub parent_tickets: &'a [String],
    pub dependency_tickets: &'a [String],
    pub queue_lanes: &'a [String],
    pub title_contains: Option<&'a str>,
    pub text_contains: Option<&'a str>,
    pub field_equals: &'a BTreeMap<String, String>,
    pub limit: Option<usize>,
    pub offset: usize,
}

/// Default number of ticket summaries returned by a bounded list when no limit is given.
pub const TICKET_LIST_DEFAULT_LIMIT: usize = 25;
/// Hard upper bound on a single bounded-list page, so the default listing is never unbounded.
pub const TICKET_LIST_MAX_LIMIT: usize = 100;

/// Filters + pagination for the single bounded ticket-list operation. This is the one query
/// contract (no separate `query` op). Lane and Board membership selectors are resolved before
/// sorting: Lane order wins when both are supplied, otherwise Board-scoped lists use stored Board
/// card order. `ready_only` is the derived actionable predicate (see `ticket_is_ready`).
#[derive(Clone, Debug, Default)]
pub struct TicketListQuery {
    pub projection: Option<TicketProjectionProfile>,
    pub statuses: Vec<String>,
    pub assignees: Vec<String>,
    pub priorities: Vec<String>,
    pub ticket_types: Vec<String>,
    pub labels: Vec<String>,
    pub policy_labels: Vec<String>,
    pub ready_only: bool,
    /// Include terminal (completed/rejected) tickets. When false, lane-scoped listings with no
    /// explicit `statuses` filter hide terminal tickets and show incomplete work only; this flag
    /// broadens terminal visibility without overriding any other filter.
    pub include_completed: bool,
    pub lane_member_ids: Option<Vec<String>>,
    pub board_id: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

/// One bounded, ordered, filtered page of compact ticket summaries. `total` is the count of tickets
/// that matched the filters before pagination; `next_cursor` is an opaque continuation token, `Some`
/// when more matches remain. Clients must treat `next_cursor` as opaque.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TicketListPage {
    pub items: Vec<TicketSummary>,
    pub total: usize,
    pub next_cursor: Option<String>,
}

pub struct TicketDeleteRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub expected_root: Option<&'a str>,
}

pub struct TicketProjectLifecyclePolicyRequest<'a> {
    pub workspace_id: &'a str,
    pub project_id: &'a str,
    pub policy: TicketLifecycleAuthorizationPolicy,
    pub project_owner_principal: Option<&'a str>,
    pub acceptance_authorities: &'a [String],
    pub expected_root: Option<&'a str>,
}

pub struct TicketProjectSettingsRequest<'a> {
    pub workspace_id: &'a str,
    pub project_id: &'a str,
    pub default_projection: Option<TicketProjectionProfile>,
    pub enable_projections: &'a [TicketProjectionProfile],
    pub disable_projections: &'a [TicketProjectionProfile],
    pub actor_enforcement: Option<TicketLifecycleAuthorizationPolicy>,
    pub project_owner_principal: Option<&'a str>,
    pub clear_project_owner_principal: bool,
    pub acceptance_authorities: Option<&'a [String]>,
    pub acceptance_evidence_enforcement: Option<bool>,
    pub required_acceptance_evidence_keys: Option<&'a [TicketAcceptanceEvidenceKey]>,
    pub owner_contract_summary: Option<&'a str>,
    pub owner_contract_details: Option<&'a str>,
    pub worker_contract_summary: Option<&'a str>,
    pub worker_contract_details: Option<&'a str>,
    pub expected_root: Option<&'a str>,
}

pub struct TicketProjectWorkflowRequest<'a> {
    pub workspace_id: &'a str,
    pub project_id: &'a str,
    pub workflow: &'a WorkflowDefinition,
    pub expected_root: Option<&'a str>,
}

pub fn parse_ticket_projection(profile: Option<&str>) -> Result<Option<TicketProjectionProfile>> {
    profile.map(TicketProjectionProfile::parse).transpose()
}

pub fn ticket_field_catalog(
    projection: Option<TicketProjectionProfile>,
    operation: Option<&str>,
) -> Result<TicketFieldCatalog> {
    let projection = projection.unwrap_or(TicketProjectionProfile::Native);
    let operation = operation.unwrap_or("write");
    match operation {
        "create" | "update" | "write" => {}
        _ => {
            return Err(LoomError::invalid(
                "ticket field catalog operation must be create, update, or write",
            ));
        }
    }
    let fields = ticket_core_field_specs()
        .into_iter()
        .map(|spec| ticket_field_catalog_entry(spec, projection, operation))
        .collect();
    Ok(TicketFieldCatalog {
        projection_profile: projection.profile_id().to_string(),
        operation: operation.to_string(),
        strict_unknown_fields: false,
        custom_fields_source: "project custom-field persistence is not surfaced yet".to_string(),
        fields,
    })
}

pub fn ticket_field_catalog_for_project(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    project_id: &str,
    projection: Option<TicketProjectionProfile>,
    operation: Option<&str>,
) -> Result<TicketFieldCatalog> {
    let mut catalog = ticket_field_catalog(projection, operation)?;
    if let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)?
        && let Some(project) = profile.project(project_id)?
    {
        catalog.strict_unknown_fields = true;
        catalog.custom_fields_source = "project".to_string();
        catalog.fields.extend(
            project
                .custom_field_definitions
                .values()
                .filter(|field| !field.retired)
                .map(ticket_custom_field_catalog_entry),
        );
    }
    Ok(catalog)
}

fn ticket_field_catalog_from_project(
    project: &TicketProject,
    projection: Option<TicketProjectionProfile>,
    operation: Option<&str>,
) -> Result<TicketFieldCatalog> {
    let mut catalog = ticket_field_catalog(projection, operation)?;
    catalog.strict_unknown_fields = true;
    catalog.custom_fields_source = "project".to_string();
    catalog.fields.extend(
        project
            .custom_field_definitions
            .values()
            .filter(|field| !field.retired)
            .map(ticket_custom_field_catalog_entry),
    );
    Ok(catalog)
}

pub fn put_ticket_field_definition(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketFieldDefinitionWriteRequest<'_>,
) -> Result<TicketFieldCatalog> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let mut project = profile
        .project(request.project_id)?
        .ok_or_else(|| LoomError::not_found("ticket project not found"))?;
    let mut definition = FieldDefinition::new(
        request.field_id,
        request.key,
        request.name,
        ticket_field_type_from_request(request.field_type, request.option_set)?,
        vec!["tickets".to_string()],
        request.required,
    )?;
    if let Some(description) = request.description {
        definition = definition.with_description(description)?;
    }
    let field = TicketCustomFieldDefinition::new(
        definition,
        request.max_length,
        request.searchable,
        request.orderable,
        request.cardinality,
        None,
        BTreeSet::from([request.project_id.to_string()]),
        request.applicable_type_ids.iter().cloned().collect(),
    )?;
    project.put_custom_field_definition(field)?;
    let base_root = profile.profile_root()?;
    profile.put_project(&project)?;
    let root_after = profile.next_profile_root()?;
    let payload = project.encode()?;
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.project_id,
            operation_kind: "ticket.field_definition_put",
            target_entity_id: Some(request.field_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &[],
            validation: None,
        },
    )?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    ticket_field_catalog_from_project(&project, None, Some("write"))
}

pub fn retire_ticket_field_definition(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketFieldDefinitionRetireRequest<'_>,
) -> Result<TicketFieldCatalog> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let mut project = profile
        .project(request.project_id)?
        .ok_or_else(|| LoomError::not_found("ticket project not found"))?;
    project.retire_custom_field_definition(request.field_id)?;
    let base_root = profile.profile_root()?;
    profile.put_project(&project)?;
    let root_after = profile.next_profile_root()?;
    let payload = project.encode()?;
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.project_id,
            operation_kind: "ticket.field_definition_retire",
            target_entity_id: Some(request.field_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &[],
            validation: None,
        },
    )?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    ticket_field_catalog_from_project(&project, None, Some("write"))
}

#[derive(Clone, Copy)]
struct TicketCoreFieldSpec {
    native_field: &'static str,
    field_type: &'static str,
    cardinality: &'static str,
    settable: bool,
    required_on_create: bool,
    searchable: bool,
    orderable: bool,
    max_length: Option<u32>,
    enum_values: &'static [&'static str],
    write_semantics: &'static str,
}

fn ticket_core_field_specs() -> Vec<TicketCoreFieldSpec> {
    vec![
        ticket_field(
            "title",
            "string",
            "single",
            true,
            true,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[],
            "Primary human-readable ticket title.",
        ),
        ticket_field(
            "description",
            "string",
            "optional",
            true,
            false,
            true,
            false,
            Some(TICKET_RICH_TEXT_MAX_BYTES as u32),
            &[],
            "Large ticket body text.",
        ),
        ticket_field(
            "status",
            "enum",
            "optional",
            true,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &NORMALIZED_TICKET_STATUSES,
            "Normalized lifecycle status. Workflow enforcement is project policy controlled.",
        ),
        ticket_field(
            "status_category",
            "enum",
            "optional",
            false,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &["todo", "in_progress", "done", "accepted"],
            "Derived reporting category for status.",
        ),
        ticket_field(
            "assignee",
            "principal",
            "optional",
            true,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[],
            "Principal assigned to work the ticket.",
        ),
        ticket_field(
            "reporter",
            "principal",
            "optional",
            true,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[],
            "Principal or imported user that reported the ticket.",
        ),
        ticket_field(
            "priority",
            "enum",
            "optional",
            true,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &["highest", "high", "medium", "low", "lowest"],
            "Project-local priority value.",
        ),
        ticket_field(
            "resolution",
            "enum",
            "optional",
            true,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[
                "done",
                "fixed",
                "duplicate",
                "declined",
                "cannot_reproduce",
                "rejected",
                "changes_requested",
                "deleted",
            ],
            "Project-local completion disposition.",
        ),
        ticket_field(
            "labels",
            "string",
            "list",
            true,
            false,
            true,
            false,
            Some(TICKET_DEFAULT_FIELD_TEXT_MAX_BYTES as u32),
            &[],
            "Project-local labels.",
        ),
        ticket_field(
            "start_date",
            "date",
            "optional",
            true,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[],
            "Start date.",
        ),
        ticket_field(
            "due_date",
            "date",
            "optional",
            true,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[],
            "Due date.",
        ),
        ticket_field(
            "original_estimate",
            "duration",
            "optional",
            true,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[],
            "Original estimate in duration form.",
        ),
        ticket_field(
            "remaining_estimate",
            "duration",
            "optional",
            true,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[],
            "Remaining estimate in duration form.",
        ),
        ticket_field(
            "time_spent",
            "duration",
            "optional",
            true,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[],
            "Time spent in duration form.",
        ),
        ticket_field(
            "story_points",
            "number",
            "optional",
            true,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[],
            "Story point estimate.",
        ),
        ticket_field(
            "security_level",
            "enum",
            "optional",
            true,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[],
            "Project-local security classification.",
        ),
        ticket_field(
            "policy_labels",
            "string",
            "list",
            true,
            false,
            true,
            false,
            Some(TICKET_DEFAULT_FIELD_TEXT_MAX_BYTES as u32),
            &[],
            "Policy labels attached to the ticket.",
        ),
        ticket_field(
            "source_anchors",
            "string",
            "list",
            true,
            false,
            true,
            false,
            Some(TICKET_DEFAULT_FIELD_TEXT_MAX_BYTES as u32),
            &[],
            TicketAcceptanceEvidenceKey::SourceAnchors.meaning(),
        ),
        ticket_field(
            "checks_run",
            "string",
            "list",
            true,
            false,
            true,
            false,
            Some(TICKET_DEFAULT_FIELD_TEXT_MAX_BYTES as u32),
            &[],
            TicketAcceptanceEvidenceKey::ChecksRun.meaning(),
        ),
        ticket_field(
            "not_run_rationale",
            "string",
            "optional",
            true,
            false,
            true,
            false,
            Some(TICKET_DEFAULT_FIELD_TEXT_MAX_BYTES as u32),
            &[],
            TicketAcceptanceEvidenceKey::NotRunRationale.meaning(),
        ),
        ticket_field(
            "files_changed",
            "string",
            "list",
            true,
            false,
            true,
            false,
            Some(TICKET_DEFAULT_FIELD_TEXT_MAX_BYTES as u32),
            &[],
            TicketAcceptanceEvidenceKey::FilesChanged.meaning(),
        ),
        ticket_field(
            "followups",
            "string",
            "list",
            true,
            false,
            true,
            false,
            Some(TICKET_DEFAULT_FIELD_TEXT_MAX_BYTES as u32),
            &[],
            TicketAcceptanceEvidenceKey::Followups.meaning(),
        ),
        ticket_field(
            "decision_points",
            "string",
            "list",
            true,
            false,
            true,
            false,
            Some(TICKET_DEFAULT_FIELD_TEXT_MAX_BYTES as u32),
            &[],
            TicketAcceptanceEvidenceKey::DecisionPoints.meaning(),
        ),
        ticket_field(
            "risk_notes",
            "string",
            "list",
            true,
            false,
            true,
            false,
            Some(TICKET_DEFAULT_FIELD_TEXT_MAX_BYTES as u32),
            &[],
            TicketAcceptanceEvidenceKey::RiskNotes.meaning(),
        ),
        ticket_field(
            "deleted_at",
            "datetime",
            "optional",
            false,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[],
            "UTC timestamp when the ticket was deleted.",
        ),
        ticket_field(
            "deleted_by",
            "principal",
            "optional",
            false,
            false,
            true,
            true,
            Some(TICKET_COMPACT_TEXT_MAX_BYTES as u32),
            &[],
            "Principal that deleted the ticket.",
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn ticket_field(
    native_field: &'static str,
    field_type: &'static str,
    cardinality: &'static str,
    settable: bool,
    required_on_create: bool,
    searchable: bool,
    orderable: bool,
    max_length: Option<u32>,
    enum_values: &'static [&'static str],
    write_semantics: &'static str,
) -> TicketCoreFieldSpec {
    TicketCoreFieldSpec {
        native_field,
        field_type,
        cardinality,
        settable,
        required_on_create,
        searchable,
        orderable,
        max_length,
        enum_values,
        write_semantics,
    }
}

fn ticket_field_catalog_entry(
    spec: TicketCoreFieldSpec,
    projection: TicketProjectionProfile,
    operation: &str,
) -> TicketFieldCatalogEntry {
    TicketFieldCatalogEntry {
        native_field: spec.native_field.to_string(),
        write_path: projected_output_field_path(spec.native_field, projection).to_string(),
        aliases: projected_field_aliases(spec.native_field, projection),
        field_type: spec.field_type.to_string(),
        cardinality: spec.cardinality.to_string(),
        settable: spec.settable,
        required_on_create: operation != "update" && spec.required_on_create,
        required_on_update: false,
        searchable: spec.searchable,
        orderable: spec.orderable,
        max_length: spec.max_length,
        enum_values: spec
            .enum_values
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        write_semantics: projected_write_semantics(
            spec.native_field,
            projection,
            spec.write_semantics,
        )
        .to_string(),
    }
}

fn projected_write_semantics<'a>(
    native_field: &str,
    projection: TicketProjectionProfile,
    default: &'a str,
) -> &'a str {
    match (projection, native_field) {
        (TicketProjectionProfile::Jira, "status") => {
            "Jira-compatible status updates use a transition payload. Submit transition.to.name or a transition object; Loom stores the normalized status after workflow validation."
        }
        _ => default,
    }
}

fn ticket_custom_field_catalog_entry(
    field: &TicketCustomFieldDefinition,
) -> TicketFieldCatalogEntry {
    TicketFieldCatalogEntry {
        native_field: field.definition.field_id.clone(),
        write_path: field.definition.key.clone(),
        aliases: Vec::new(),
        field_type: ticket_field_type_name(&field.definition.field_type),
        cardinality: ticket_cardinality_name(&field.cardinality),
        settable: !field.retired && !field.definition.imported_computed,
        required_on_create: field.definition.required,
        required_on_update: false,
        searchable: field.searchable,
        orderable: field.orderable,
        max_length: field.max_length,
        enum_values: Vec::new(),
        write_semantics: field
            .definition
            .description
            .clone()
            .unwrap_or_else(|| "Project custom ticket field.".to_string()),
    }
}

fn ticket_field_type_from_request(field_type: &str, option_set: Option<&str>) -> Result<FieldType> {
    match field_type {
        "string" => Ok(FieldType::String),
        "integer" => Ok(FieldType::Integer),
        "number" => Ok(FieldType::Number),
        "boolean" => Ok(FieldType::Boolean),
        "date" => Ok(FieldType::Date),
        "datetime" => Ok(FieldType::DateTime),
        "duration" => Ok(FieldType::Duration),
        "principal" => Ok(FieldType::Principal),
        "enum" => FieldType::enum_options(
            option_set
                .ok_or_else(|| LoomError::invalid("enum ticket field requires option_set"))?,
        ),
        "url" => Ok(FieldType::Url),
        "opaque_json" => Ok(FieldType::OpaqueJson),
        _ => Err(LoomError::invalid("unsupported ticket field type")),
    }
}

fn ticket_field_type_name(field_type: &FieldType) -> String {
    match field_type {
        FieldType::String => "string".to_string(),
        FieldType::Integer => "integer".to_string(),
        FieldType::Number => "number".to_string(),
        FieldType::Boolean => "boolean".to_string(),
        FieldType::Date => "date".to_string(),
        FieldType::DateTime => "datetime".to_string(),
        FieldType::DateRange => "date_range".to_string(),
        FieldType::Duration => "duration".to_string(),
        FieldType::Principal => "principal".to_string(),
        FieldType::EntityRef { kind } => match kind {
            Some(kind) => format!("entity_ref:{kind}"),
            None => "entity_ref".to_string(),
        },
        FieldType::Enum { option_set } => format!("enum:{option_set}"),
        FieldType::Url => "url".to_string(),
        FieldType::List(inner) => format!("list:{}", ticket_field_type_name(inner)),
        FieldType::OpaqueJson => "opaque_json".to_string(),
    }
}

fn ticket_cardinality_name(cardinality: &TicketFieldCardinality) -> String {
    match cardinality {
        TicketFieldCardinality::Single => "single".to_string(),
        TicketFieldCardinality::Optional => "optional".to_string(),
        TicketFieldCardinality::List { .. } => "list".to_string(),
    }
}

fn projected_output_field_path(field: &str, projection: TicketProjectionProfile) -> &str {
    match projection {
        TicketProjectionProfile::Native => field,
        TicketProjectionProfile::Jira => match field {
            "title" => "fields.summary",
            "description" => "fields.description",
            "status" => "transition.to.name",
            "assignee" => "fields.assignee",
            "reporter" => "fields.reporter",
            "priority" => "fields.priority",
            "resolution" => "fields.resolution",
            value => value,
        },
        TicketProjectionProfile::Asana => match field {
            "title" => "data.name",
            "description" => "data.notes",
            "assignee" => "data.assignee",
            "due_date" => "data.due_on",
            value => value,
        },
        TicketProjectionProfile::Notion => match field {
            "title" => "properties.Name.title",
            "description" => "properties.Description.rich_text",
            "status" => "properties.Status.status",
            "assignee" => "properties.Assignee.people",
            "priority" => "properties.Priority.select",
            "due_date" => "properties.Due.date",
            value => value,
        },
        TicketProjectionProfile::Redmine => match field {
            "title" => "issue.subject",
            "description" => "issue.description",
            "status" => "issue.status_id",
            "assignee" => "issue.assigned_to_id",
            "priority" => "issue.priority_id",
            "due_date" => "issue.due_date",
            value => value,
        },
    }
}

fn projected_field_aliases(field: &str, projection: TicketProjectionProfile) -> Vec<String> {
    match (projection, field) {
        (TicketProjectionProfile::Jira, "title") => vec!["summary".to_string()],
        (TicketProjectionProfile::Asana, "description") => vec!["html_notes".to_string()],
        (TicketProjectionProfile::Notion, "title") => vec!["Name".to_string(), "title".to_string()],
        (TicketProjectionProfile::Notion, "description") => {
            vec!["Description".to_string(), "description".to_string()]
        }
        (TicketProjectionProfile::Notion, "status") => {
            vec!["Status".to_string(), "status".to_string()]
        }
        (TicketProjectionProfile::Notion, "assignee") => {
            vec!["Assignee".to_string(), "assignee".to_string()]
        }
        (TicketProjectionProfile::Notion, "priority") => {
            vec!["Priority".to_string(), "priority".to_string()]
        }
        (TicketProjectionProfile::Notion, "due_date") => vec![
            "Due".to_string(),
            "Due date".to_string(),
            "due_date".to_string(),
        ],
        (TicketProjectionProfile::Redmine, "title") => vec!["subject".to_string()],
        (TicketProjectionProfile::Redmine, "assignee") => vec!["assigned_to".to_string()],
        (TicketProjectionProfile::Redmine, "priority") => vec!["priority".to_string()],
        _ => Vec::new(),
    }
}

pub fn normalize_ticket_fields_for_projection(
    fields: &Value,
    projection: Option<TicketProjectionProfile>,
) -> Result<Value> {
    let Some(projection) = projection else {
        return require_ticket_field_object(fields)
            .cloned()
            .map(Value::Object);
    };
    match projection {
        TicketProjectionProfile::Native => require_ticket_field_object(fields)
            .cloned()
            .map(Value::Object),
        TicketProjectionProfile::Jira => normalize_jira_fields(fields),
        TicketProjectionProfile::Asana => normalize_asana_fields(fields),
        TicketProjectionProfile::Notion => normalize_notion_fields(fields),
        TicketProjectionProfile::Redmine => normalize_redmine_fields(fields),
    }
}

pub fn normalize_ticket_delete_fields_for_projection(
    fields: &[String],
    projection: Option<TicketProjectionProfile>,
) -> Vec<String> {
    fields
        .iter()
        .map(|field| projected_input_field_key(field, projection).to_string())
        .collect()
}

pub struct TicketLifecycleRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub action: TicketLifecycleAction,
    pub target_status: Option<&'a str>,
    pub assignee: Option<&'a str>,
    pub expected_root: Option<&'a str>,
}

pub struct TicketRelationRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub relation_id: Option<&'a str>,
    pub kind: TicketRelationKind,
    pub target_id: &'a str,
    pub expected_root: Option<&'a str>,
}

pub struct TicketRelationRemoveRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub relation_id: &'a str,
    pub expected_root: Option<&'a str>,
}

pub struct TicketCommentRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub comment_id: Option<&'a str>,
    pub comment_type: Option<&'a str>,
    pub body: &'a str,
    pub evidence: Option<TicketCommentEvidence>,
    pub expected_root: Option<&'a str>,
}

pub struct TicketCommentUpdateRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub comment_id: &'a str,
    pub comment_type: Option<&'a str>,
    pub body: Option<&'a str>,
    pub evidence: Option<Option<TicketCommentEvidence>>,
    pub expected_root: Option<&'a str>,
}

pub struct TicketCommentDeleteRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub comment_id: &'a str,
    pub expected_root: Option<&'a str>,
}

pub struct TicketAttachmentRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub attachment_id: Option<&'a str>,
    pub digest: Digest,
    pub name: &'a str,
    pub media_type: &'a str,
    pub size: u64,
    pub shared: bool,
    pub expected_root: Option<&'a str>,
}

pub struct TicketWatchRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub principal: Option<&'a str>,
    pub watch: bool,
    pub expected_root: Option<&'a str>,
}

pub struct TicketRankRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub rank_token: &'a str,
    pub expected_root: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TicketRelationSummary {
    pub workspace_id: String,
    pub ticket_id: String,
    pub relation_id: String,
    pub kind: String,
    pub target_type: String,
    pub target_id: String,
    pub graph_edge_id: String,
    pub profile_root: String,
    pub operation_id: String,
    pub sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TicketRelationView {
    pub direction: String,
    pub kind: String,
    pub target_ticket_id: String,
    pub target_title: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TicketRelationCompact {
    pub relation_id: String,
    pub kind: String,
    pub target_type: String,
    pub target_id: String,
    pub target: Option<TicketRelationTargetState>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TicketRelationTargetState {
    pub primary_key: String,
    pub title: Option<String>,
    pub status: Option<String>,
    pub blocked: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TicketRelationRollup {
    pub total_children: usize,
    pub accepted_children: usize,
    pub blocked_children: usize,
    pub waiting_for_review_children: usize,
    pub feedback_available_children: usize,
    pub in_progress_children: usize,
}

pub struct TicketReferenceCandidateRequest<'a> {
    pub workspace_id: &'a str,
    pub ticket_id: &'a str,
    pub operation_id: &'a str,
    pub source_root: Digest,
    pub fields: &'a BTreeMap<String, Value>,
    pub now_ms: u64,
}

struct OperationRecordRequest<'a> {
    workspace_id: &'a str,
    scope_id: &'a str,
    operation_kind: &'a str,
    target_entity_id: Option<&'a str>,
    base_root: Digest,
    root_after: Digest,
    payload: &'a [u8],
    policy_labels: &'a [&'a str],
    validation: Option<&'a WorkflowValidationRecord>,
}

pub fn create_project(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    project_id: &str,
    key_prefix: &str,
    name: &str,
    expected_root: Option<&str>,
) -> Result<TicketProjectSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, workspace_id)?;
    profile.enforce_expected_root(expected_root)?;
    if profile.project(project_id)?.is_some() {
        return Err(LoomError::new(
            Code::AlreadyExists,
            "ticket project already exists",
        ));
    }
    let mut project = TicketProject::new(project_id, key_prefix, name)?;
    project.project_owner_principal = profile
        .effective_principal()?
        .map(|principal| principal.to_string());
    if profile.prefix_exists(&project.key_prefix)? {
        return Err(LoomError::new(
            Code::AlreadyExists,
            "ticket project key prefix exists",
        ));
    }
    let payload = project.encode()?;
    let base_root = profile.profile_root()?;
    profile.put_project(&project)?;
    let root_after = profile.next_profile_root()?;
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id,
            scope_id: project_id,
            operation_kind: "project.created",
            target_entity_id: Some(project_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &[],
            validation: None,
        },
    )?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    Ok(project_summary(
        workspace_id,
        &project,
        root_after,
        record.operation_id,
        record.sequence,
        false,
    ))
}

pub fn create_board(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: BoardCreateRequest<'_>,
) -> Result<BoardSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    if profile.board(request.board_id)?.is_some() {
        return Err(LoomError::new(
            Code::AlreadyExists,
            "ticket board already exists",
        ));
    }
    profile
        .project(request.project_id)?
        .ok_or_else(|| LoomError::not_found("ticket board project not found"))?;
    let board = TicketBoard::first_class(
        request.board_id,
        request.board_key,
        request.name,
        request.description,
        request.project_id,
        request.scope,
        request.mode,
        request.columns.to_vec(),
        request.swimlanes.to_vec(),
        request.card_display_fields.to_vec(),
        request.owner_principal.map(str::to_string),
        request.coordinator_principal.map(str::to_string),
        BoardStatus::Active,
        now_ms(),
        request.updated_by,
    )?;
    let payload = board.encode()?;
    let base_root = profile.profile_root()?;
    profile.put_board(&board)?;
    let root_after = profile.next_profile_root()?;
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: &board.board_id,
            operation_kind: "board.created",
            target_entity_id: Some(&board.board_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &[],
            validation: None,
        },
    )?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    Ok(board_summary(
        request.workspace_id,
        &board,
        Vec::new(),
        root_after,
        Some(record.operation_id),
        Some(record.sequence),
    ))
}

pub fn get_board(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    board_id: &str,
) -> Result<Option<BoardSummary>> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
        return Ok(None);
    };
    let Some(board) = profile.board(board_id)? else {
        return Ok(None);
    };
    let cards = profile.board_cards(&board.board_id)?;
    Ok(Some(board_summary(
        workspace_id,
        &board,
        cards,
        profile.profile_root()?,
        None,
        None,
    )))
}

pub fn list_boards(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    include_deleted: bool,
) -> Result<Vec<BoardSummary>> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
        return Ok(Vec::new());
    };
    let profile_root = profile.profile_root()?;
    let mut boards = profile
        .boards()?
        .into_iter()
        .filter(|board| include_deleted || board.board_status != BoardStatus::Deleted)
        .map(|board| {
            let cards = profile.board_cards(&board.board_id)?;
            Ok(board_summary(
                workspace_id,
                &board,
                cards,
                profile_root,
                None,
                None,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    boards.sort_by(|a, b| {
        a.board_key
            .cmp(&b.board_key)
            .then(a.board_id.cmp(&b.board_id))
    });
    Ok(boards)
}

pub fn update_board(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: BoardUpdateRequest<'_>,
) -> Result<BoardSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let mut board = profile
        .board(request.board_id)?
        .ok_or_else(|| LoomError::not_found("ticket board not found"))?;
    if let Some(board_key) = request.board_key {
        board.board_key = board_key.to_string();
    }
    if let Some(name) = request.name {
        board.name = name.to_string();
    }
    if let Some(description) = request.description {
        board.description = description.to_string();
    }
    if let Some(scope) = request.scope {
        board.scope = scope;
    }
    if let Some(owner) = request.owner_principal {
        board.owner_principal = owner.map(str::to_string);
    }
    if let Some(coordinator) = request.coordinator_principal {
        board.coordinator_principal = coordinator.map(str::to_string);
    }
    if let Some(fields) = request.card_display_fields {
        board.card_display_fields = fields.to_vec();
    }
    if let Some(status) = request.board_status {
        board.board_status = status;
    }
    board.updated_at = now_ms();
    board.updated_by = request.updated_by.to_string();
    board.validate()?;
    persist_board_update(
        &mut profile,
        workspace,
        request.workspace_id,
        board,
        "board.updated",
    )
}

pub fn configure_board_columns(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: BoardColumnConfigureRequest<'_>,
) -> Result<BoardSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let mut board = profile
        .board(request.board_id)?
        .ok_or_else(|| LoomError::not_found("ticket board not found"))?;
    if let Some(mode) = request.mode {
        board.mode = mode;
    }
    board.columns = request.columns.to_vec();
    board.swimlanes = request.swimlanes.to_vec();
    board.updated_at = now_ms();
    board.updated_by = request.updated_by.to_string();
    board.validate()?;
    persist_board_update(
        &mut profile,
        workspace,
        request.workspace_id,
        board,
        "board.columns_configured",
    )
}

pub fn move_board_card(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: BoardCardMoveRequest<'_>,
) -> Result<BoardSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let board = profile
        .board(request.board_id)?
        .ok_or_else(|| LoomError::not_found("ticket board not found"))?;
    let column = board
        .columns
        .iter()
        .find(|column| column.column_id == request.column_id)
        .ok_or_else(|| LoomError::invalid("board card target column not found"))?;
    let ticket = profile
        .ticket(request.ticket_id)?
        .ok_or_else(|| LoomError::not_found("board card ticket not found"))?;
    if board.mode == BoardMode::StatusMapped {
        let ticket_status = ticket_named_field_text(&ticket, "status");
        let matches_column = ticket_status
            .as_deref()
            .is_some_and(|status| column.mapped_statuses.contains(status));
        if !matches_column {
            return Err(LoomError::invalid(
                "status-mapped board card moves must use ticket lifecycle transition first",
            ));
        }
    }
    let placement = BoardCardPlacement::new(
        request.board_id,
        request.ticket_id,
        request.column_id,
        request.rank_token,
        request.swimlane_id.map(str::to_string),
        now_ms(),
        request.updated_by,
    )?;
    let payload = placement.encode()?;
    let base_root = profile.profile_root()?;
    profile.put_board_card(&placement)?;
    let root_after = profile.next_profile_root()?;
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.board_id,
            operation_kind: "board.card_moved",
            target_entity_id: Some(request.ticket_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &[],
            validation: None,
        },
    )?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    let cards = profile.board_cards(request.board_id)?;
    Ok(board_summary(
        request.workspace_id,
        &board,
        cards,
        root_after,
        Some(record.operation_id),
        Some(record.sequence),
    ))
}

pub fn create_ticket(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketCreateRequest<'_>,
) -> Result<TicketSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    // resolve any assignee alias/handle set on create (including importer-supplied values)
    // to the canonical principal id before opening the profile, which borrows `loom` mutably.
    let mut fields = fields_from_json(request.fields)?;
    canonicalize_assignee_field(loom, &mut fields);
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let external_identity = ticket_external_identity(request.external_source, request.external_id)?;
    if let Some(identity) = &external_identity
        && profile.ticket_by_external_identity(identity)?.is_some()
    {
        return Err(LoomError::new(
            Code::AlreadyExists,
            "ticket external identity already exists",
        ));
    }
    let mut project = profile
        .project(request.project_id)?
        .ok_or_else(|| LoomError::not_found("ticket project not found"))?;
    let ticket_number = project.allocate_ticket_number()?;
    let key = project.ticket_key(ticket_number)?.canonical();
    let ticket_type = parse_ticket_type(request.ticket_type)?;
    validate_ticket_fields_against_project(&project, ticket_type.type_id(), &fields, true)?;
    let labels = request
        .policy_labels
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let ticket_id = allocate_ticket_id(&profile)?;
    let ticket = Ticket::new(TicketInput {
        ticket_id: &ticket_id,
        project_id: request.project_id,
        ticket_number,
        ticket_type,
        external_identity,
        fields,
        policy_labels: &labels,
    })?;
    let payload = ticket.encode()?;
    let base_root = profile.profile_root()?;
    profile.put_project(&project)?;
    profile.put_ticket(&ticket)?;
    let root_after = profile.next_profile_root()?;
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.project_id,
            operation_kind: "ticket.created",
            target_entity_id: Some(&ticket.ticket_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &labels,
            validation: None,
        },
    )?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    update_ticket_revision_index(
        loom,
        workspace,
        request.workspace_id,
        &ticket.ticket_id,
        &record,
        &payload,
    )?;
    let mut summary = ticket_summary(
        request.workspace_id,
        &ticket,
        key,
        Some(record.operation_id),
        Some(record.sequence),
        root_after,
        None,
    );
    // surface the additive display alias for the (already canonicalized) assignee on the
    // create response too, consistent with read projections.
    attach_assignee_display(loom.identity_store(), &ticket, &mut summary);
    Ok(summary)
}

pub fn rekey_project(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    project_id: &str,
    key_prefix: &str,
    expected_root: Option<&str>,
) -> Result<TicketProjectSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, workspace_id)?;
    profile.enforce_expected_root(expected_root)?;
    let mut project = profile
        .project(project_id)?
        .ok_or_else(|| LoomError::not_found("ticket project not found"))?;
    project.rekey(key_prefix)?;
    if profile.prefix_exists(&project.key_prefix)? {
        return Err(LoomError::new(
            Code::AlreadyExists,
            "ticket project key prefix exists",
        ));
    }
    let payload = project.encode()?;
    let base_root = profile.profile_root()?;
    profile.put_project(&project)?;
    let root_after = profile.next_profile_root()?;
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id,
            scope_id: project_id,
            operation_kind: "project.rekeyed",
            target_entity_id: Some(project_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &[],
            validation: None,
        },
    )?;
    let principal = profile.effective_principal()?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation_with_audit(
        principal,
        Some("tickets.project.rekeyed"),
        Some(project_id),
    )?;
    debug_assert_eq!(persisted_root, root_after);
    Ok(project_summary(
        workspace_id,
        &project,
        root_after,
        record.operation_id,
        record.sequence,
        false,
    ))
}

pub fn set_project_lifecycle_policy(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketProjectLifecyclePolicyRequest<'_>,
) -> Result<TicketProjectSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Admin)?;
    if let Some(project_owner_principal) = request.project_owner_principal {
        validate_lifecycle_principal(loom, project_owner_principal, "project owner")?;
    }
    for principal in request.acceptance_authorities {
        validate_lifecycle_principal(loom, principal, "acceptance authority")?;
    }
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let mut project = profile
        .project(request.project_id)?
        .ok_or_else(|| LoomError::not_found("ticket project not found"))?;
    project.lifecycle_authorization_policy = request.policy;
    if let Some(project_owner_principal) = request.project_owner_principal {
        project.project_owner_principal = Some(project_owner_principal.to_string());
    }
    if matches!(
        request.policy,
        TicketLifecycleAuthorizationPolicy::OwnershipGoverned
            | TicketLifecycleAuthorizationPolicy::ReviewAuthority
    ) && project.project_owner_principal.is_none()
        && request.acceptance_authorities.is_empty()
    {
        return Err(LoomError::invalid(
            "review-gated lifecycle policy requires project owner or acceptance authority",
        ));
    }
    project.acceptance_authorities = request.acceptance_authorities.iter().cloned().collect();
    project.validate()?;
    let payload = project.encode()?;
    let base_root = profile.profile_root()?;
    profile.put_project(&project)?;
    let root_after = profile.next_profile_root()?;
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.project_id,
            operation_kind: "project.lifecycle_policy_set",
            target_entity_id: Some(request.project_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &[],
            validation: None,
        },
    )?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    Ok(project_summary(
        request.workspace_id,
        &project,
        root_after,
        record.operation_id,
        record.sequence,
        false,
    ))
}

pub fn set_project_settings(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketProjectSettingsRequest<'_>,
) -> Result<TicketProjectSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Admin)?;
    if let Some(project_owner_principal) = request.project_owner_principal {
        validate_lifecycle_principal(loom, project_owner_principal, "project owner")?;
    }
    if let Some(acceptance_authorities) = request.acceptance_authorities {
        for principal in acceptance_authorities {
            validate_lifecycle_principal(loom, principal, "acceptance authority")?;
        }
    }
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let mut project = profile
        .project(request.project_id)?
        .ok_or_else(|| LoomError::not_found("ticket project not found"))?;

    if let Some(policy) = request.actor_enforcement {
        project.lifecycle_authorization_policy = policy;
    }
    if request.clear_project_owner_principal {
        project.project_owner_principal = None;
    }
    if let Some(project_owner_principal) = request.project_owner_principal {
        project.project_owner_principal = Some(project_owner_principal.to_string());
    }
    if let Some(acceptance_authorities) = request.acceptance_authorities {
        project.acceptance_authorities = acceptance_authorities.iter().cloned().collect();
    }
    if let Some(enforcement) = request.acceptance_evidence_enforcement {
        project.acceptance_evidence_policy.enforcement_enabled = enforcement;
    }
    if let Some(required_keys) = request.required_acceptance_evidence_keys {
        project.acceptance_evidence_policy.required_keys = required_keys.iter().copied().collect();
    }
    if let Some(summary) = request.owner_contract_summary {
        project.contracts.owner.summary = summary.to_string();
    }
    if let Some(details) = request.owner_contract_details {
        project.contracts.owner.details = details.to_string();
    }
    if let Some(summary) = request.worker_contract_summary {
        project.contracts.worker.summary = summary.to_string();
    }
    if let Some(details) = request.worker_contract_details {
        project.contracts.worker.details = details.to_string();
    }

    let mut enabled_projections = project.projection_config.enabled_projections.clone();
    for profile in request.enable_projections {
        enabled_projections.insert(*profile);
    }
    for profile in request.disable_projections {
        if profile == &TicketProjectionProfile::Native {
            return Err(LoomError::invalid(
                "native ticket projection must remain enabled",
            ));
        }
        enabled_projections.remove(profile);
    }
    let default_display_projection = request
        .default_projection
        .unwrap_or(project.projection_config.default_display_projection);
    if !enabled_projections.contains(&default_display_projection) {
        enabled_projections.insert(default_display_projection);
    }
    let profile_configs = project.projection_config.profile_configs.clone();
    let retained_config_profiles = enabled_projections.clone();
    project.projection_config = crate::TicketProjectionProjectConfig::new(
        default_display_projection,
        enabled_projections,
        profile_configs
            .into_iter()
            .filter(|(profile, _)| retained_config_profiles.contains(profile))
            .collect(),
    )?;

    if matches!(
        project.lifecycle_authorization_policy,
        TicketLifecycleAuthorizationPolicy::OwnershipGoverned
            | TicketLifecycleAuthorizationPolicy::ReviewAuthority
    ) && project.project_owner_principal.is_none()
        && project.acceptance_authorities.is_empty()
    {
        return Err(LoomError::invalid(
            "review-gated lifecycle policy requires project owner or acceptance authority",
        ));
    }
    project.validate()?;
    let payload = project.encode()?;
    let base_root = profile.profile_root()?;
    profile.put_project(&project)?;
    let root_after = profile.next_profile_root()?;
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.project_id,
            operation_kind: "project.settings_set",
            target_entity_id: Some(request.project_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &[],
            validation: None,
        },
    )?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    Ok(project_summary(
        request.workspace_id,
        &project,
        root_after,
        record.operation_id,
        record.sequence,
        false,
    ))
}

pub fn set_project_workflow(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketProjectWorkflowRequest<'_>,
) -> Result<TicketProjectSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Admin)?;
    request.workflow.validate()?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let mut project = profile
        .project(request.project_id)?
        .ok_or_else(|| LoomError::not_found("ticket project not found"))?;
    project.active_workflow = Some(request.workflow.clone());
    project.validate()?;
    let payload = project.encode()?;
    let base_root = profile.profile_root()?;
    profile.put_project(&project)?;
    let root_after = profile.next_profile_root()?;
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: request.project_id,
            operation_kind: "workflow.updated",
            target_entity_id: Some(request.project_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &[],
            validation: None,
        },
    )?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    Ok(project_summary(
        request.workspace_id,
        &project,
        root_after,
        record.operation_id,
        record.sequence,
        false,
    ))
}

pub fn get_project(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    project_id: &str,
) -> Result<Option<TicketProjectSummary>> {
    get_project_with_contract_details(loom, workspace, workspace_id, project_id, false)
}

pub fn get_project_with_contract_details(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    project_id: &str,
    include_contract_details: bool,
) -> Result<Option<TicketProjectSummary>> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
        return Ok(None);
    };
    profile
        .project(project_id)?
        .map(|project| {
            Ok(project_summary(
                workspace_id,
                &project,
                profile.profile_root()?,
                String::new(),
                0,
                include_contract_details,
            ))
        })
        .transpose()
}

fn validate_lifecycle_principal(
    loom: &Loom<FileStore>,
    principal: &str,
    label: &str,
) -> Result<()> {
    let principal_id = WorkspaceId::parse(principal)
        .map_err(|_| LoomError::invalid(format!("{label} must be a PrincipalId")))?;
    let identity = loom
        .identity_store()
        .ok_or_else(|| LoomError::invalid("identity store is required"))?;
    identity
        .principal(principal_id)
        .map(|_| ())
        .map_err(|_| LoomError::invalid(format!("{label} principal is not registered")))
}

fn legacy_action_target_status(action: TicketLifecycleAction) -> Option<&'static str> {
    match action {
        TicketLifecycleAction::Release => Some("ready"),
        _ => action.target_status(),
    }
}

#[cfg(test)]
fn default_ticket_workflow() -> Result<WorkflowDefinition> {
    let states = [
        "backlog",
        "planned",
        "ready",
        "in_progress",
        "blocked",
        "waiting_for_review",
        "accepted",
        "rejected",
        "closed",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();
    let mut edges = Vec::new();
    for (from, to) in [
        ("backlog", "planned"),
        ("backlog", "ready"),
        ("backlog", "in_progress"),
        ("planned", "ready"),
        ("planned", "in_progress"),
        ("planned", "blocked"),
        ("planned", "backlog"),
        ("ready", "in_progress"),
        ("ready", "blocked"),
        ("ready", "backlog"),
        ("in_progress", "ready"),
        ("blocked", "ready"),
        ("waiting_for_review", "ready"),
        ("blocked", "in_progress"),
        ("rejected", "in_progress"),
        ("accepted", "in_progress"),
        ("in_progress", "waiting_for_review"),
        ("blocked", "waiting_for_review"),
        ("waiting_for_review", "accepted"),
        ("waiting_for_review", "rejected"),
        ("in_progress", "blocked"),
        ("waiting_for_review", "blocked"),
        ("accepted", "closed"),
        ("rejected", "closed"),
    ] {
        edges.push(crate::WorkflowEdge::new(
            format!("{from}-to-{to}"),
            from,
            to,
            Vec::new(),
        )?);
    }
    WorkflowDefinition::new("loom.default.ticket", "v1", states, edges)
}

pub fn update_ticket_fields(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketUpdateFieldsRequest<'_>,
) -> Result<TicketSummary> {
    update_ticket(
        loom,
        workspace,
        TicketUpdateRequest {
            workspace_id: request.workspace_id,
            ticket_id: request.ticket_id,
            set_fields: Some(request.fields),
            delete_fields: &[],
            action: None,
            target_status: None,
            observed_source_status: None,
            observed_workflow_version: None,
            assignee: None,
            expected_root: request.expected_root,
            comment: None,
            comments: &[],
            relation_sets: &[],
            relation_removes: &[],
        },
    )
}

pub fn list_projects(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<TicketProject>> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
        return Ok(Vec::new());
    };
    profile.projects()
}

/// List a ticket's canonical relations in both directions without scanning the ticket set: outgoing
/// relations come from the ticket's own `relations`, and incoming relations are read from the derived
/// `ticket-relations` graph's reverse adjacency. Each view carries direction, relation kind, the
/// other ticket's id, and its title (best-effort). Shared contract for CLI and MCP.
pub fn list_ticket_relations(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
) -> Result<Vec<TicketRelationView>> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
        return Ok(Vec::new());
    };
    let resolved_ticket_id = resolve_ticket_id(&profile, ticket_id)?;
    let Some(ticket) = profile.ticket(&resolved_ticket_id)? else {
        return Err(LoomError::not_found(format!("ticket {ticket_id:?}")));
    };
    let mut out = Vec::new();
    for relation in ticket.relations.values() {
        let target_title = if relation.target_type == TicketRelationTargetType::Ticket {
            profile
                .ticket(&relation.target_id)?
                .and_then(|target| relation_ticket_title(&target))
                .unwrap_or_default()
        } else {
            String::new()
        };
        out.push(TicketRelationView {
            direction: "outgoing".to_string(),
            kind: relation.kind.as_str().to_string(),
            target_ticket_id: relation.target_id.clone(),
            target_title,
        });
    }
    let node = format!("ticket:{resolved_ticket_id}");
    for (_edge_id, edge) in graph_in_edges(loom, workspace, "ticket-relations", &node)? {
        if let Some(source_id) = edge.src.strip_prefix("ticket:") {
            let target_title = profile
                .ticket(source_id)?
                .and_then(|source| relation_ticket_title(&source))
                .unwrap_or_default();
            out.push(TicketRelationView {
                direction: "incoming".to_string(),
                kind: edge.label.clone(),
                target_ticket_id: source_id.to_string(),
                target_title,
            });
        }
    }
    out.sort_by(|a, b| {
        a.direction
            .cmp(&b.direction)
            .then(a.kind.cmp(&b.kind))
            .then(a.target_ticket_id.cmp(&b.target_ticket_id))
    });
    Ok(out)
}

fn relation_ticket_title(ticket: &Ticket) -> Option<String> {
    ["title", "subject", "name"]
        .iter()
        .find_map(|field| crate::model::text_like_field(&ticket.fields, field))
}

pub fn update_ticket(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketUpdateRequest<'_>,
) -> Result<TicketSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let has_comments = request.comment.is_some() || !request.comments.is_empty();
    let has_relation_mutations =
        !request.relation_sets.is_empty() || !request.relation_removes.is_empty();
    let actor = if has_comments {
        Some(ticket_actor(loom, workspace)?)
    } else {
        None
    };
    // resolve an assignee alias/handle to the canonical principal id before opening the
    // profile (which borrows `loom` mutably and hides the identity store). Persisted value is the
    // canonical id when the handle resolves; unknown values fall back to the given string.
    let canonical_assignee = request
        .assignee
        .map(|assignee| canonicalize_assignee(loom, assignee));
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let mut set_fields = match request.set_fields {
        Some(fields) => fields_from_json(fields)?,
        None => BTreeMap::new(),
    };
    if set_fields.contains_key("assignee") {
        return Err(LoomError::invalid(
            "ticket assignee requires a lifecycle action",
        ));
    }
    if request
        .delete_fields
        .iter()
        .any(|field| field == "status" || field == "assignee")
    {
        return Err(LoomError::invalid(
            "ticket status and assignee require a lifecycle action",
        ));
    }
    for field in request.delete_fields {
        if set_fields.contains_key(field) {
            return Err(LoomError::invalid(
                "ticket update cannot set and delete the same field",
            ));
        }
    }
    let field_target_status = extract_status_target_from_fields(&mut set_fields)?;
    let target_status = match (
        request.target_status.map(str::to_string),
        field_target_status,
    ) {
        (Some(explicit), Some(field)) if explicit != field => {
            return Err(LoomError::invalid(
                "ticket update target_status conflicts with status field",
            ));
        }
        (Some(explicit), _) => Some(explicit),
        (None, Some(field)) => Some(field),
        (None, None) => request
            .action
            .and_then(legacy_action_target_status)
            .map(str::to_string),
    };
    if request.set_fields.is_none()
        && request.delete_fields.is_empty()
        && target_status.is_none()
        && canonical_assignee.is_none()
        && !has_comments
        && !has_relation_mutations
    {
        return Err(LoomError::invalid(
            "ticket update requires set_fields, delete_fields, action, target_status, comment, or relation mutation",
        ));
    }
    if let Some(comment) = request.comment.as_ref() {
        let comment_type = comment
            .comment_type
            .unwrap_or(crate::TICKET_DEFAULT_COMMENT_TYPE);
        validate_ticket_comment_type(comment_type)?;
    }
    for comment in request.comments {
        let comment_type = comment
            .comment_type
            .unwrap_or(crate::TICKET_DEFAULT_COMMENT_TYPE);
        validate_ticket_comment_type(comment_type)?;
    }
    for field in request.delete_fields {
        validate_ticket_field_name(field)?;
    }
    let mut update_comments = Vec::new();
    if let Some(comment) = request.comment {
        update_comments.push(comment);
    }
    update_comments.extend_from_slice(request.comments);
    let ticket_id = resolve_ticket_id(&profile, request.ticket_id)?;
    let mut ticket = profile
        .ticket(&ticket_id)?
        .ok_or_else(|| LoomError::corrupt("ticket resolved to a missing record"))?;
    let project = profile
        .project(&ticket.project_id)?
        .ok_or_else(|| LoomError::corrupt("ticket project is missing"))?;
    validate_ticket_update_fields_against_project(
        &project,
        &ticket,
        &set_fields,
        request.delete_fields,
    )?;
    let mut operation_kind = "ticket.updated";
    let mut transition_validation = None;
    if let Some(target_status) = target_status.as_deref() {
        let actor = profile
            .effective_principal()?
            .unwrap_or(workspace)
            .to_string();
        let current_status = ticket_status(&ticket);
        let transition = TransitionOperation {
            operation_id: format!("{}:{}", request.workspace_id, profile.next_sequence()),
            actor_principal: actor.clone(),
            target_status: target_status.to_string(),
            observed_source_status: request
                .observed_source_status
                .unwrap_or(&current_status)
                .to_string(),
            observed_workflow_version: request
                .observed_workflow_version
                .or(project
                    .active_workflow
                    .as_ref()
                    .map(|workflow| workflow.version.as_str()))
                .unwrap_or("permissive")
                .to_string(),
            attached_fields: set_fields.clone(),
        };
        if let Some(workflow) = project.active_workflow.as_ref() {
            let validation = validate_transition(
                Some(workflow),
                &current_status,
                &ticket.fields,
                &transition,
                &WorkflowValidationContext::default(),
            )?;
            if validation.validation_state != WorkflowValidationState::Applied {
                return Err(LoomError::new(
                    Code::Conflict,
                    "ticket workflow transition rejected",
                ));
            }
            transition_validation = Some(validation);
        }
        authorize_transition(&project, &ticket, &actor, target_status)?;
        apply_transition_fields(
            &mut ticket,
            &actor,
            target_status,
            canonical_assignee.as_deref(),
        )?;
        operation_kind = "ticket.transitioned";
    } else if let Some(assignee) = canonical_assignee.as_deref() {
        loom_substrate::validate_text("ticket assignee", assignee)?;
        ticket.fields.insert(
            "assignee".to_string(),
            TicketFieldValue::Principal(assignee.to_string()),
        );
        operation_kind = "ticket.assigned";
    }
    for field in request.delete_fields {
        ticket.fields.remove(field);
    }
    ticket.fields.extend(set_fields);
    if let Some(target_status) = target_status.as_deref() {
        let existing_comments = if target_status == "accepted"
            && project.acceptance_evidence_policy.enforcement_enabled
        {
            profile.comments(&ticket.ticket_id)?
        } else {
            Vec::new()
        };
        enforce_acceptance_evidence(
            &project,
            target_status,
            &existing_comments,
            &update_comments,
        )?;
    }
    let mut touched_relation_ids = BTreeSet::new();
    let mut relation_projection_removals = Vec::new();
    let mut relation_projection_sets = Vec::new();
    for remove in request.relation_removes {
        loom_substrate::validate_text("ticket relation_id", remove.relation_id)?;
        if !touched_relation_ids.insert(remove.relation_id.to_string()) {
            return Err(LoomError::invalid(
                "ticket update cannot mutate the same relation twice",
            ));
        }
        let relation = ticket
            .relations
            .remove(remove.relation_id)
            .ok_or_else(|| LoomError::not_found("ticket relation not found"))?;
        relation_projection_removals.push(relation);
    }
    for set in request.relation_sets {
        let target_type = set.kind.target_type();
        let target_id = normalize_relation_target(&profile, target_type, set.target_id)?;
        let relation_id = set
            .relation_id
            .map(str::to_string)
            .unwrap_or_else(|| default_relation_id(set.kind, target_type, &target_id));
        if !touched_relation_ids.insert(relation_id.clone()) {
            return Err(LoomError::invalid(
                "ticket update cannot mutate the same relation twice",
            ));
        }
        let relation = TicketRelation::new(relation_id, set.kind, target_type, target_id)?;
        if let Some(replaced) = ticket
            .relations
            .insert(relation.relation_id.clone(), relation.clone())
        {
            relation_projection_removals.push(replaced);
        }
        relation_projection_sets.push(relation);
    }
    ticket.validate()?;
    let base_root = profile.profile_root()?;
    profile.put_ticket(&ticket)?;
    for comment in update_comments {
        let comment_type = comment
            .comment_type
            .unwrap_or(crate::TICKET_DEFAULT_COMMENT_TYPE);
        let sequence = profile.next_sequence();
        let comment_id = comment
            .comment_id
            .map(str::to_string)
            .unwrap_or_else(|| format!("comment:{sequence}"));
        if profile.comment_exists(&ticket.ticket_id, &comment_id)? {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "ticket comment already exists",
            ));
        }
        let mut comment_record = TicketComment::new(
            comment_id,
            actor
                .as_ref()
                .ok_or_else(|| LoomError::corrupt("ticket update comment actor missing"))?,
            comment.body,
            now_ms(),
        )?;
        comment_record.comment_type = comment_type.to_string();
        comment_record.evidence = comment.evidence;
        comment_record.validate()?;
        profile.put_comment(&ticket.ticket_id, &comment_record)?;
        if operation_kind == "ticket.updated" {
            operation_kind = "ticket.comment_added";
        }
    }
    for relation in &relation_projection_removals {
        profile.remove_relation_projection(&ticket.ticket_id, relation)?;
    }
    for relation in &relation_projection_sets {
        profile.upsert_relation_projection(&ticket.ticket_id, relation)?;
    }
    if operation_kind == "ticket.updated" && has_relation_mutations {
        operation_kind = "ticket.relations_updated";
    }
    let payload = ticket.encode()?;
    let root_after = profile.next_profile_root()?;
    let labels = ticket
        .policy_labels
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: &ticket.project_id,
            operation_kind,
            target_entity_id: Some(&ticket.ticket_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &labels,
            validation: transition_validation.as_ref(),
        },
    )?;
    let primary_key = ticket_key(&profile, &ticket)?;
    let mut ticket_summary = ticket_summary(
        request.workspace_id,
        &ticket,
        primary_key,
        Some(record.operation_id.clone()),
        Some(record.sequence),
        root_after,
        None,
    );
    ticket_summary.comments = compact_ticket_comments(&profile.comments(&ticket.ticket_id)?);
    let ticket_id = ticket.ticket_id.clone();
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    update_ticket_revision_index(
        loom,
        workspace,
        request.workspace_id,
        &ticket_id,
        &record,
        &payload,
    )?;
    emit_ticket_change_notification(loom, workspace, request.workspace_id, &ticket_id, &record)?;
    // the profile borrow of `loom` has ended, so resolve the additive display alias for the
    // final assignee on the response summary too (keeps write responses consistent with reads).
    attach_assignee_display(loom.identity_store(), &ticket, &mut ticket_summary);
    Ok(ticket_summary)
}

fn extract_status_target_from_fields(
    set_fields: &mut BTreeMap<String, TicketFieldValue>,
) -> Result<Option<String>> {
    let Some(value) = set_fields.remove("status") else {
        return Ok(None);
    };
    match value {
        TicketFieldValue::String(status) | TicketFieldValue::EnumOption(status) => {
            loom_substrate::validate_text("ticket target status", &status)?;
            Ok(Some(status))
        }
        _ => Err(LoomError::invalid(
            "ticket status field must be a string or enum value",
        )),
    }
}

pub fn apply_ticket_lifecycle(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketLifecycleRequest<'_>,
) -> Result<TicketSummary> {
    update_ticket(
        loom,
        workspace,
        TicketUpdateRequest {
            workspace_id: request.workspace_id,
            ticket_id: request.ticket_id,
            set_fields: None,
            delete_fields: &[],
            action: Some(request.action),
            target_status: request.target_status,
            observed_source_status: None,
            observed_workflow_version: None,
            assignee: request.assignee,
            expected_root: request.expected_root,
            comment: None,
            comments: &[],
            relation_sets: &[],
            relation_removes: &[],
        },
    )
}

pub fn delete_ticket(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketDeleteRequest<'_>,
) -> Result<TicketSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let ticket_id = resolve_ticket_id(&profile, request.ticket_id)?;
    let mut ticket = profile
        .ticket(&ticket_id)?
        .ok_or_else(|| LoomError::corrupt("ticket resolved to a missing record"))?;
    if matches!(
        ticket.fields.get("resolution"),
        Some(TicketFieldValue::EnumOption(value) | TicketFieldValue::String(value))
            if value == "deleted"
    ) {
        return Err(LoomError::new(Code::Conflict, "ticket is already deleted"));
    }
    let actor = profile
        .effective_principal()?
        .unwrap_or(workspace)
        .to_string();
    ticket.fields.insert(
        "status".to_string(),
        TicketFieldValue::EnumOption("closed".to_string()),
    );
    ticket.fields.insert(
        "status_category".to_string(),
        TicketFieldValue::EnumOption("done".to_string()),
    );
    ticket.fields.insert(
        "resolution".to_string(),
        TicketFieldValue::EnumOption("deleted".to_string()),
    );
    ticket.fields.insert(
        "deleted_at".to_string(),
        TicketFieldValue::DateTime(DateTimeValue::from_unix_millis_utc(now_ms() as i64)),
    );
    ticket
        .fields
        .insert("deleted_by".to_string(), TicketFieldValue::Principal(actor));
    ticket.validate()?;
    let payload = ticket.encode()?;
    let base_root = profile.profile_root()?;
    profile.put_ticket(&ticket)?;
    let root_after = profile.next_profile_root()?;
    let labels = ticket
        .policy_labels
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: &ticket.project_id,
            operation_kind: "ticket.deleted",
            target_entity_id: Some(&ticket.ticket_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &labels,
            validation: None,
        },
    )?;
    let primary_key = ticket_key(&profile, &ticket)?;
    let mut ticket_summary = ticket_summary(
        request.workspace_id,
        &ticket,
        primary_key,
        Some(record.operation_id.clone()),
        Some(record.sequence),
        root_after,
        None,
    );
    ticket_summary.comments = compact_ticket_comments(&profile.comments(&ticket.ticket_id)?);
    let ticket_id = ticket.ticket_id.clone();
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    update_ticket_revision_index(
        loom,
        workspace,
        request.workspace_id,
        &ticket_id,
        &record,
        &payload,
    )?;
    emit_ticket_change_notification(loom, workspace, request.workspace_id, &ticket_id, &record)?;
    Ok(ticket_summary)
}

pub fn add_ticket_relation(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketRelationRequest<'_>,
) -> Result<TicketRelationSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let source_ticket_id = resolve_ticket_id(&profile, request.ticket_id)?;
    let mut ticket = profile
        .ticket(&source_ticket_id)?
        .ok_or_else(|| LoomError::corrupt("ticket resolved to a missing record"))?;
    let target_type = request.kind.target_type();
    let target_id = normalize_relation_target(&profile, target_type, request.target_id)?;
    let relation_id = request
        .relation_id
        .map(str::to_string)
        .unwrap_or_else(|| default_relation_id(request.kind, target_type, &target_id));
    let relation = TicketRelation::new(relation_id, request.kind, target_type, target_id)?;
    let replaced = ticket
        .relations
        .insert(relation.relation_id.clone(), relation.clone());
    ticket.validate()?;
    let payload = ticket.encode()?;
    let base_root = profile.profile_root()?;
    profile.put_ticket(&ticket)?;
    if let Some(replaced) = replaced {
        profile.remove_relation_projection(&ticket.ticket_id, &replaced)?;
    }
    let edge_id = profile.upsert_relation_projection(&ticket.ticket_id, &relation)?;
    let root_after = profile.next_profile_root()?;
    let labels = ticket
        .policy_labels
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: &ticket.project_id,
            operation_kind: "ticket.relation_set",
            target_entity_id: Some(&ticket.ticket_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &labels,
            validation: None,
        },
    )?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    update_ticket_revision_index(
        loom,
        workspace,
        request.workspace_id,
        &ticket.ticket_id,
        &record,
        &payload,
    )?;
    Ok(relation_summary(
        request.workspace_id,
        &ticket.ticket_id,
        &relation,
        edge_id,
        root_after,
        record.operation_id,
        record.sequence,
    ))
}

pub fn remove_ticket_relation(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketRelationRemoveRequest<'_>,
) -> Result<TicketRelationSummary> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, request.workspace_id)?;
    profile.enforce_expected_root(request.expected_root)?;
    let source_ticket_id = resolve_ticket_id(&profile, request.ticket_id)?;
    let mut ticket = profile
        .ticket(&source_ticket_id)?
        .ok_or_else(|| LoomError::corrupt("ticket resolved to a missing record"))?;
    let relation = ticket
        .relations
        .remove(request.relation_id)
        .ok_or_else(|| LoomError::not_found("ticket relation not found"))?;
    let edge_id = crate::ticket_relation_edge_id(&ticket.ticket_id, &relation);
    let payload = ticket.encode()?;
    let base_root = profile.profile_root()?;
    profile.put_ticket(&ticket)?;
    profile.remove_relation_projection(&ticket.ticket_id, &relation)?;
    let root_after = profile.next_profile_root()?;
    let labels = ticket
        .policy_labels
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id: request.workspace_id,
            scope_id: &ticket.project_id,
            operation_kind: "ticket.relation_removed",
            target_entity_id: Some(&ticket.ticket_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &labels,
            validation: None,
        },
    )?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    update_ticket_revision_index(
        loom,
        workspace,
        request.workspace_id,
        &ticket.ticket_id,
        &record,
        &payload,
    )?;
    Ok(relation_summary(
        request.workspace_id,
        &ticket.ticket_id,
        &relation,
        edge_id,
        root_after,
        record.operation_id,
        record.sequence,
    ))
}

pub fn add_ticket_comment(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketCommentRequest<'_>,
) -> Result<TicketSummary> {
    let actor = ticket_actor(loom, workspace)?;
    let comment_type = request
        .comment_type
        .unwrap_or(crate::TICKET_DEFAULT_COMMENT_TYPE);
    validate_ticket_comment_type(comment_type)?;
    mutate_ticket_collaboration(
        loom,
        workspace,
        request.workspace_id,
        request.ticket_id,
        request.expected_root,
        "ticket.comment_added",
        |profile, ticket, sequence| {
            let comment_id = request
                .comment_id
                .map(str::to_string)
                .unwrap_or_else(|| format!("comment:{sequence}"));
            if profile.comment_exists(&ticket.ticket_id, &comment_id)? {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "ticket comment already exists",
                ));
            }
            let mut comment =
                TicketComment::new(comment_id.clone(), actor, request.body, now_ms())?;
            comment.comment_type = comment_type.to_string();
            comment.evidence = request.evidence.clone();
            comment.validate()?;
            profile.put_comment(&ticket.ticket_id, &comment)?;
            Ok(())
        },
    )
}

pub fn list_ticket_comments(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
) -> Result<Vec<TicketComment>> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
        return Ok(Vec::new());
    };
    let ticket_id = match resolve_ticket_id(&profile, ticket_id) {
        Ok(ticket_id) => ticket_id,
        Err(error) if error.code == Code::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    profile.comments(&ticket_id)
}

pub fn update_ticket_comment(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketCommentUpdateRequest<'_>,
) -> Result<TicketSummary> {
    if request.comment_type.is_none() && request.body.is_none() && request.evidence.is_none() {
        return Err(LoomError::invalid(
            "ticket comment update requires comment_type, body, or evidence",
        ));
    }
    if let Some(comment_type) = request.comment_type {
        validate_ticket_comment_type(comment_type)?;
    }
    mutate_ticket_collaboration(
        loom,
        workspace,
        request.workspace_id,
        request.ticket_id,
        request.expected_root,
        "ticket.comment_updated",
        |profile, ticket, _sequence| {
            let mut comment = profile
                .comment(&ticket.ticket_id, request.comment_id)?
                .ok_or_else(|| LoomError::not_found("ticket comment not found"))?;
            if let Some(comment_type) = request.comment_type {
                comment.comment_type = comment_type.to_string();
            }
            if let Some(body) = request.body {
                comment.body = body.to_string();
                comment.redacted = false;
            }
            if let Some(evidence) = request.evidence.clone() {
                comment.evidence = evidence;
            }
            comment.updated_at_ms = Some(now_ms());
            comment.validate()?;
            profile.delete_comment(&ticket.ticket_id, request.comment_id)?;
            profile.put_comment(&ticket.ticket_id, &comment)?;
            Ok(())
        },
    )
}

pub fn delete_ticket_comment(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketCommentDeleteRequest<'_>,
) -> Result<TicketSummary> {
    mutate_ticket_collaboration(
        loom,
        workspace,
        request.workspace_id,
        request.ticket_id,
        request.expected_root,
        "ticket.comment_deleted",
        |profile, ticket, _sequence| {
            let mut comment = profile
                .comment(&ticket.ticket_id, request.comment_id)?
                .ok_or_else(|| LoomError::not_found("ticket comment not found"))?;
            comment.body.clear();
            comment.redacted = true;
            comment.updated_at_ms = Some(now_ms());
            comment.validate()?;
            profile.delete_comment(&ticket.ticket_id, request.comment_id)?;
            profile.put_comment(&ticket.ticket_id, &comment)?;
            Ok(())
        },
    )
}

pub fn add_ticket_attachment(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketAttachmentRequest<'_>,
) -> Result<TicketSummary> {
    let actor = ticket_actor(loom, workspace)?;
    mutate_ticket_collaboration(
        loom,
        workspace,
        request.workspace_id,
        request.ticket_id,
        request.expected_root,
        "ticket.attachment_added",
        |profile, ticket, sequence| {
            let attachment_id = request
                .attachment_id
                .map(str::to_string)
                .unwrap_or_else(|| format!("attachment:{sequence}"));
            if profile.attachment_exists(&ticket.ticket_id, &attachment_id)? {
                return Err(LoomError::new(
                    Code::AlreadyExists,
                    "ticket attachment already exists",
                ));
            }
            let attachment = TicketAttachment::new(
                attachment_id.clone(),
                request.digest,
                request.name,
                request.media_type,
                request.size,
                actor,
                now_ms(),
                request.shared,
            )?;
            profile.put_attachment(&ticket.ticket_id, &attachment)?;
            Ok(())
        },
    )
}

pub fn set_ticket_watch(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketWatchRequest<'_>,
) -> Result<TicketSummary> {
    let actor = ticket_actor(loom, workspace)?;
    mutate_ticket_collaboration(
        loom,
        workspace,
        request.workspace_id,
        request.ticket_id,
        request.expected_root,
        if request.watch {
            "ticket.watcher_added"
        } else {
            "ticket.watcher_removed"
        },
        |profile, ticket, _| {
            let principal = request.principal.unwrap_or(&actor);
            loom_substrate::validate_text("ticket watcher", principal)?;
            profile.set_watcher(&ticket.ticket_id, principal, request.watch)?;
            Ok(())
        },
    )
}

pub fn set_ticket_rank(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketRankRequest<'_>,
) -> Result<TicketSummary> {
    mutate_ticket_collaboration(
        loom,
        workspace,
        request.workspace_id,
        request.ticket_id,
        request.expected_root,
        "ticket.ranked",
        |profile, ticket, _| {
            loom_substrate::validate_text("ticket rank token", request.rank_token)?;
            profile.set_rank_token(&ticket.ticket_id, request.rank_token)?;
            Ok(())
        },
    )
}

pub fn reconcile_ticket_relation_projection(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    max: usize,
) -> Result<usize> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
        return Ok(0);
    };
    let tickets = profile.tickets()?;
    drop(profile);
    let mut expected = BTreeSet::new();
    let mut writes = 0;
    for ticket in tickets.iter().take(max) {
        for relation in ticket.relations.values() {
            expected.insert(crate::ticket_relation_edge_id(&ticket.ticket_id, relation));
            graph_upsert_node(
                loom,
                workspace,
                "ticket-relations",
                &format!("ticket:{}", ticket.ticket_id),
                relation_node_props("ticket", &ticket.ticket_id),
            )?;
            graph_upsert_node(
                loom,
                workspace,
                "ticket-relations",
                &format!("{}:{}", relation.target_type.as_str(), relation.target_id),
                relation_node_props(relation.target_type.as_str(), &relation.target_id),
            )?;
            graph_upsert_edge(
                loom,
                workspace,
                "ticket-relations",
                &crate::ticket_relation_edge_id(&ticket.ticket_id, relation),
                &format!("ticket:{}", ticket.ticket_id),
                &format!("{}:{}", relation.target_type.as_str(), relation.target_id),
                relation.kind.as_str(),
                relation_edge_props(&ticket.ticket_id, relation),
            )?;
            writes += 1;
        }
    }
    for (edge_id, edge) in graph_edges(loom, workspace, "ticket-relations")? {
        if edge
            .props
            .get("derived_from")
            .is_some_and(|value| value == &GraphValue::Text("tickets".to_string()))
            && !expected.contains(&edge_id)
        {
            graph_remove_edge(loom, workspace, "ticket-relations", &edge_id)?;
            writes += 1;
        }
    }
    Ok(writes)
}

pub fn get_ticket(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
) -> Result<Option<TicketSummary>> {
    get_ticket_with_projection(loom, workspace, workspace_id, ticket_id, None)
}

pub fn get_ticket_with_projection(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
    requested_projection: Option<TicketProjectionProfile>,
) -> Result<Option<TicketSummary>> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
        return Ok(None);
    };
    let resolved = resolve_ticket_id(&profile, ticket_id);
    let ticket = match resolved {
        Ok(ticket_id) => profile.ticket(&ticket_id)?,
        Err(error) if error.code == Code::NotFound => None,
        Err(error) => return Err(error),
    };
    ticket
        .map(|ticket| {
            let selection = ticket_projection_selection(&profile, &ticket, requested_projection)?;
            let mut summary = ticket_summary(
                workspace_id,
                &ticket,
                ticket_key(&profile, &ticket)?,
                None,
                None,
                profile.profile_root()?,
                Some(&selection),
            );
            summary.comments = compact_ticket_comments(&profile.comments(&ticket.ticket_id)?);
            enrich_ticket_relation_projection(&profile, &mut summary)?;
            attach_assignee_display(loom.identity_store(), &ticket, &mut summary);
            Ok(summary)
        })
        .transpose()
}

pub fn list_tickets(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<TicketSummary>> {
    list_tickets_with_projection(loom, workspace, workspace_id, None)
}

pub fn list_tickets_with_projection(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    requested_projection: Option<TicketProjectionProfile>,
) -> Result<Vec<TicketSummary>> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
        return Ok(Vec::new());
    };
    let profile_root = profile.profile_root()?;
    profile
        .tickets()?
        .iter()
        .map(|ticket| {
            let selection = ticket_projection_selection(&profile, ticket, requested_projection)?;
            let mut summary = ticket_summary(
                workspace_id,
                ticket,
                ticket_key(&profile, ticket)?,
                None,
                None,
                profile_root,
                Some(&selection),
            );
            summary.comments = compact_ticket_comments(&profile.comments(&ticket.ticket_id)?);
            enrich_ticket_relation_projection(&profile, &mut summary)?;
            attach_assignee_display(loom.identity_store(), ticket, &mut summary);
            Ok(summary)
        })
        .collect()
}

pub fn query_tickets(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketQueryRequest<'_>,
) -> Result<Vec<TicketSummary>> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, request.workspace_id)? else {
        return Ok(Vec::new());
    };
    let profile_root = profile.profile_root()?;
    let mut out = Vec::new();
    let mut skipped = 0usize;
    for ticket in profile.tickets()? {
        if !ticket_matches_query(&ticket, &request) {
            continue;
        }
        if skipped < request.offset {
            skipped += 1;
            continue;
        }
        if request.limit.is_some_and(|limit| out.len() >= limit) {
            break;
        }
        let selection = ticket_projection_selection(&profile, &ticket, request.projection)?;
        let mut summary = ticket_summary(
            request.workspace_id,
            &ticket,
            ticket_key(&profile, &ticket)?,
            None,
            None,
            profile_root,
            Some(&selection),
        );
        summary.comments = compact_ticket_comments(&profile.comments(&ticket.ticket_id)?);
        enrich_ticket_relation_projection(&profile, &mut summary)?;
        attach_assignee_display(loom.identity_store(), &ticket, &mut summary);
        out.push(summary);
    }
    Ok(out)
}

/// Bounded, filtered, most-recently-updated-first page of compact ticket summaries. This is the
/// single listing path: never unbounded (default [`TICKET_LIST_DEFAULT_LIMIT`], hard
/// [`TICKET_LIST_MAX_LIMIT`]), ordered by the per-ticket operation `sequence` descending with the
/// ticket id as the deterministic tie-breaker, resumed via an opaque continuation cursor.
pub fn list_tickets_page(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    query: &TicketListQuery,
) -> Result<TicketListPage> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
        return Ok(TicketListPage {
            items: Vec::new(),
            total: 0,
            next_cursor: None,
        });
    };
    let profile_root = profile.profile_root()?;
    let all: Vec<Ticket> = profile.tickets()?;
    // Dependency readiness is computed from canonical `depends_on` relations against the current
    // status of each dependency ticket, never from free-form fields.
    let status_by_id: BTreeMap<String, String> = all
        .iter()
        .filter_map(|ticket| {
            ticket_status_value(ticket).map(|status| (ticket.ticket_id.clone(), status))
        })
        .collect();
    let sequence_by_ticket: BTreeMap<String, u64> = profile
        .operations()?
        .into_iter()
        .filter_map(|record| {
            record
                .target_entity_id
                .map(|ticket_id| (ticket_id, record.sequence))
        })
        .fold(BTreeMap::new(), |mut sequences, (ticket_id, sequence)| {
            sequences
                .entry(ticket_id)
                .and_modify(|current| *current = (*current).max(sequence))
                .or_insert(sequence);
            sequences
        });
    // Lane order is preserved from the caller-supplied member list (the Lane's stored order);
    // membership is the same ids as a set. Board order comes from stored card placements. If both
    // are supplied, the result is their membership intersection ordered by Lane order.
    let lane_order = resolved_lane_member_order(&profile, query.lane_member_ids.as_deref())?;
    let lane_member_ids: Option<BTreeSet<String>> =
        lane_order.as_ref().map(|ids| ids.iter().cloned().collect());
    let board_order = resolved_board_member_order(&profile, query.board_id.as_deref())?;
    let board_member_ids: Option<BTreeSet<String>> = board_order
        .as_ref()
        .map(|ids| ids.iter().cloned().collect());
    let mut matched: Vec<&Ticket> = all
        .iter()
        .filter(|&ticket| {
            ticket_list_matches(
                ticket,
                query,
                &status_by_id,
                lane_member_ids.as_ref(),
                board_member_ids.as_ref(),
            )
        })
        .collect();
    match (&lane_order, &board_order) {
        // Lane priority: when both Lane and Board filters are present, membership is intersected
        // above and ordering follows the Lane's stored order.
        (Some(order), _) => {
            let position: BTreeMap<&str, usize> = order
                .iter()
                .enumerate()
                .map(|(index, id)| (id.as_str(), index))
                .collect();
            matched.sort_by(|a, b| {
                position
                    .get(a.ticket_id.as_str())
                    .cmp(&position.get(b.ticket_id.as_str()))
                    .then_with(|| a.ticket_id.cmp(&b.ticket_id))
            });
        }
        (None, Some(order)) => {
            let position: BTreeMap<&str, usize> = order
                .iter()
                .enumerate()
                .map(|(index, id)| (id.as_str(), index))
                .collect();
            matched.sort_by(|a, b| {
                position
                    .get(a.ticket_id.as_str())
                    .cmp(&position.get(b.ticket_id.as_str()))
                    .then_with(|| a.ticket_id.cmp(&b.ticket_id))
            });
        }
        // Non-lane: most-recently-updated first (per-ticket operation sequence; tickets carry no
        // wall-clock timestamp), ties broken on ticket id for a stable, deterministic order.
        (None, None) => {
            matched.sort_by(|a, b| {
                sequence_by_ticket
                    .get(&b.ticket_id)
                    .cmp(&sequence_by_ticket.get(&a.ticket_id))
                    .then_with(|| a.ticket_id.cmp(&b.ticket_id))
            });
        }
    }
    let total = matched.len();
    let start = match query.cursor.as_deref() {
        Some(cursor) => decode_ticket_list_cursor(cursor)?.min(total),
        None => 0,
    };
    let limit = query
        .limit
        .unwrap_or(TICKET_LIST_DEFAULT_LIMIT)
        .clamp(1, TICKET_LIST_MAX_LIMIT);
    let end = start.saturating_add(limit).min(total);
    let mut items = Vec::with_capacity(end.saturating_sub(start));
    for &ticket in &matched[start..end] {
        let selection = ticket_projection_selection(&profile, ticket, query.projection)?;
        let mut summary = ticket_summary(
            workspace_id,
            ticket,
            ticket_key(&profile, ticket)?,
            None,
            sequence_by_ticket.get(&ticket.ticket_id).copied(),
            profile_root,
            Some(&selection),
        );
        summary.comments = compact_ticket_comments(&profile.comments(&ticket.ticket_id)?);
        enrich_ticket_relation_projection(&profile, &mut summary)?;
        attach_assignee_display(loom.identity_store(), ticket, &mut summary);
        items.push(summary);
    }
    let next_cursor = if end < total {
        Some(encode_ticket_list_cursor(end))
    } else {
        None
    };
    Ok(TicketListPage {
        items,
        total,
        next_cursor,
    })
}

fn ticket_status_value(ticket: &Ticket) -> Option<String> {
    ticket_named_field_text(ticket, "status")
}

/// A dependency counts as satisfied only when its ticket is in a completed terminal state.
fn ticket_status_is_completed(status: &str) -> bool {
    matches!(status, "accepted" | "closed" | "done")
}

/// Terminal statuses excluded from lane-scoped listings by default: completed states plus rejected.
fn ticket_status_is_terminal(status: &str) -> bool {
    ticket_status_is_completed(status) || status == "rejected"
}

/// Derived actionable readiness: the ticket is in an actionable pre-work status (backlog, planned,
/// or ready) and every canonical `depends_on` relation targets a completed dependency ticket.
fn ticket_is_ready(ticket: &Ticket, status_by_id: &BTreeMap<String, String>) -> bool {
    let actionable = ticket_status_value(ticket)
        .is_some_and(|status| matches!(status.as_str(), "backlog" | "planned" | "ready"));
    if !actionable {
        return false;
    }
    ticket
        .relations
        .values()
        .filter(|relation| matches!(relation.kind, TicketRelationKind::DependsOn))
        .all(|relation| {
            status_by_id
                .get(&relation.target_id)
                .is_some_and(|status| ticket_status_is_completed(status))
        })
}

fn ticket_list_matches(
    ticket: &Ticket,
    query: &TicketListQuery,
    status_by_id: &BTreeMap<String, String>,
    lane_member_ids: Option<&BTreeSet<String>>,
    board_member_ids: Option<&BTreeSet<String>>,
) -> bool {
    all_empty_or_contains(&query.statuses, ticket_status_value(ticket).as_deref())
        && all_empty_or_contains(
            &query.assignees,
            ticket_named_field_text(ticket, "assignee").as_deref(),
        )
        && all_empty_or_contains(
            &query.priorities,
            ticket_named_field_text(ticket, "priority").as_deref(),
        )
        && (query.ticket_types.is_empty()
            || query
                .ticket_types
                .iter()
                .any(|kind| kind == ticket_type_name(ticket.ticket_type)))
        && labels_match(
            &query.labels,
            ticket_named_field_text(ticket, "labels").as_deref(),
        )
        && (query.policy_labels.is_empty()
            || query
                .policy_labels
                .iter()
                .any(|label| ticket.policy_labels.contains(label)))
        && lane_member_ids.is_none_or(|ids| ids.contains(&ticket.ticket_id))
        && board_member_ids.is_none_or(|ids| ids.contains(&ticket.ticket_id))
        && (!query.ready_only || ticket_is_ready(ticket, status_by_id))
        // Lane- and Board-scoped listings default to incomplete work: hide terminal tickets only
        // when a scoped ordering surface is in use, no explicit status filter was supplied, and
        // completed work was not requested.
        // Explicit `statuses` are always honored; `include_completed` only broadens terminal
        // visibility and never overrides another filter.
        && (query.include_completed
            || !query.statuses.is_empty()
            || (lane_member_ids.is_none() && board_member_ids.is_none())
            || ticket_status_value(ticket)
                .is_none_or(|status| !ticket_status_is_terminal(&status)))
}

/// Resolve caller-supplied Lane member ids to canonical ticket ids, preserving the caller's order
/// (the Lane's stored order) and de-duplicating on first occurrence. Order is what lets a
/// lane-scoped listing sort by stored Lane order; membership is the same ids as a set.
fn resolved_lane_member_order(
    profile: &impl TicketProfileLookup,
    lane_member_ids: Option<&[String]>,
) -> Result<Option<Vec<String>>> {
    let Some(lane_member_ids) = lane_member_ids else {
        return Ok(None);
    };
    let mut resolved = Vec::new();
    let mut seen = BTreeSet::new();
    for id in lane_member_ids {
        let canonical = if profile.ticket(id)?.is_some() {
            Some(id.clone())
        } else {
            profile
                .resolve_ticket_key(id)?
                .map(|ticket| ticket.ticket_id)
        };
        if let Some(canonical) = canonical
            && seen.insert(canonical.clone())
        {
            resolved.push(canonical);
        }
    }
    Ok(Some(resolved))
}

fn resolved_board_member_order(
    profile: &TicketProfileReader<'_>,
    board_id: Option<&str>,
) -> Result<Option<Vec<String>>> {
    let Some(board_id) = board_id else {
        return Ok(None);
    };
    let boards = profile.boards()?;
    let board = boards
        .iter()
        .find(|board| {
            board.board_id == board_id || board.board_key == board_id || board.name == board_id
        })
        .ok_or_else(|| LoomError::new(Code::NotFound, format!("board {board_id:?} not found")))?;
    let column_rank: BTreeMap<&str, u64> = board
        .columns
        .iter()
        .map(|column| (column.column_id.as_str(), column.rank))
        .collect();
    let mut cards = profile.board_cards(&board.board_id)?;
    cards.sort_by(|a, b| {
        column_rank
            .get(a.column_id.as_str())
            .copied()
            .unwrap_or(u64::MAX)
            .cmp(
                &column_rank
                    .get(b.column_id.as_str())
                    .copied()
                    .unwrap_or(u64::MAX),
            )
            .then_with(|| a.column_id.cmp(&b.column_id))
            .then_with(|| a.rank_token.cmp(&b.rank_token))
            .then_with(|| a.ticket_id.cmp(&b.ticket_id))
    });
    let mut resolved = Vec::new();
    let mut seen = BTreeSet::new();
    for card in cards {
        let canonical = if profile.ticket(&card.ticket_id)?.is_some() {
            Some(card.ticket_id)
        } else {
            profile
                .resolve_ticket_key(&card.ticket_id)?
                .map(|ticket| ticket.ticket_id)
        };
        if let Some(canonical) = canonical
            && seen.insert(canonical.clone())
        {
            resolved.push(canonical);
        }
    }
    Ok(Some(resolved))
}

fn labels_match(wanted: &[String], value: Option<&str>) -> bool {
    wanted.is_empty()
        || value.is_some_and(|value| {
            wanted
                .iter()
                .any(|label| value_contains_token(value, label))
        })
}

/// Opaque continuation token. Internally it carries the resume offset, but callers must treat it as
/// an opaque string (the public contract does not expose offset mechanics).
fn encode_ticket_list_cursor(offset: usize) -> String {
    format!("tkl1_{offset}")
}

fn decode_ticket_list_cursor(cursor: &str) -> Result<usize> {
    cursor
        .strip_prefix("tkl1_")
        .and_then(|rest| rest.parse::<usize>().ok())
        .ok_or_else(|| LoomError::invalid("invalid ticket list continuation cursor"))
}

fn ticket_matches_query(ticket: &Ticket, request: &TicketQueryRequest<'_>) -> bool {
    all_empty_or_contains(
        request.statuses,
        ticket_named_field_text(ticket, "status").as_deref(),
    ) && all_empty_or_contains(
        request.buckets,
        ticket_named_field_text(ticket, "bucket").as_deref(),
    ) && all_empty_or_contains(
        request.assignees,
        ticket_named_field_text(ticket, "assignee").as_deref(),
    ) && all_empty_or_contains(
        request.lane_owners,
        ticket_named_field_text(ticket, "lane_owner").as_deref(),
    ) && all_empty_or_contains(
        request.parent_tickets,
        ticket_named_field_text(ticket, "parent_ticket").as_deref(),
    ) && all_empty_or_contains(
        request.queue_lanes,
        ticket_named_field_text(ticket, "queue_lane").as_deref(),
    ) && dependency_matches(ticket, request.dependency_tickets)
        && contains_filter(
            ticket_named_field_text(ticket, "title").as_deref(),
            request.title_contains,
        )
        && text_query_matches(ticket, request.text_contains)
        && request
            .field_equals
            .iter()
            .all(|(field, expected)| ticket_field_matches(ticket, field, expected))
}

fn all_empty_or_contains(allowed: &[String], value: Option<&str>) -> bool {
    allowed.is_empty() || value.is_some_and(|value| allowed.iter().any(|item| item == value))
}

fn dependency_matches(ticket: &Ticket, dependency_tickets: &[String]) -> bool {
    dependency_tickets.is_empty()
        || [
            "blocks",
            "blockers",
            "blocked_ticket",
            "blocked_by",
            "depends_on",
        ]
        .iter()
        .any(|field| {
            ticket_named_field_text(ticket, field).is_some_and(|value| {
                dependency_tickets
                    .iter()
                    .any(|expected| value_contains_token(&value, expected))
            })
        })
}

fn contains_filter(value: Option<&str>, needle: Option<&str>) -> bool {
    let Some(needle) = needle.filter(|needle| !needle.is_empty()) else {
        return true;
    };
    value.is_some_and(|value| lower_contains(value, needle))
}

fn text_query_matches(ticket: &Ticket, text_contains: Option<&str>) -> bool {
    let Some(needle) = text_contains.filter(|needle| !needle.is_empty()) else {
        return true;
    };
    ticket
        .fields
        .values()
        .any(|value| lower_contains(&field_value_search_text(value), needle))
}

fn ticket_field_matches(ticket: &Ticket, field: &str, expected: &str) -> bool {
    ticket_named_field_text(ticket, field).is_some_and(|value| value == expected)
}

fn ticket_named_field_text(ticket: &Ticket, field: &str) -> Option<String> {
    ticket.fields.get(field).map(field_value_search_text)
}

fn field_value_search_text(value: &TicketFieldValue) -> String {
    match value.to_json() {
        Value::String(value) => value,
        Value::Array(values) => values
            .iter()
            .map(json_value_search_text)
            .collect::<Vec<_>>()
            .join(" "),
        value => json_value_search_text(&value),
    }
}

fn json_value_search_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(values) => values
            .iter()
            .map(json_value_search_text)
            .collect::<Vec<_>>()
            .join(" "),
        Value::Object(value) => value
            .values()
            .map(json_value_search_text)
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn lower_contains(value: &str, needle: &str) -> bool {
    value.to_lowercase().contains(&needle.to_lowercase())
}

fn value_contains_token(value: &str, expected: &str) -> bool {
    value
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | '[' | ']'))
        .any(|token| token.trim_matches('"') == expected)
        || value == expected
}

pub fn history(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: Option<&str>,
) -> Result<Vec<TicketHistoryRecord>> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
        return Ok(Vec::new());
    };
    let resolved_ticket_id = match ticket_id {
        Some(ticket_id) => match resolve_ticket_id(&profile, ticket_id) {
            Ok(ticket_id) => Some(ticket_id),
            Err(error) if error.code == Code::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        },
        None => None,
    };
    let mut out = Vec::new();
    for record in profile.operations()? {
        if resolved_ticket_id
            .as_deref()
            .is_some_and(|id| record.target_entity_id.as_deref() != Some(id))
        {
            continue;
        }
        let envelope = OperationEnvelope::decode(&record.envelope)?;
        let comments = match record.target_entity_id.as_deref() {
            Some(ticket_id) => compact_ticket_comments(&profile.comments(ticket_id)?),
            None => Vec::new(),
        };
        out.push(TicketHistoryRecord {
            sequence: record.sequence,
            operation_id: record.operation_id,
            operation_kind: record.operation_kind,
            target_entity_id: record.target_entity_id,
            comments,
            envelope: envelope.debug_json(),
        });
    }
    Ok(out)
}

pub fn operation_changes(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    cursor: &OperationChangeCursor,
    max: usize,
) -> Result<OperationChangeBatch> {
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Read)?;
    let workspace_id = cursor
        .scope_id
        .strip_prefix("tickets:")
        .ok_or_else(|| LoomError::invalid("unsupported profile operation cursor"))?;
    let Some(profile) = TicketProfileReader::open(loom, workspace, workspace_id)? else {
        return Ok(OperationChangeBatch {
            events: Vec::new(),
            next: cursor.clone(),
        });
    };
    crate::TicketOperationLog::new(workspace_id, profile.operations()?)?.changes(cursor, max)
}

fn mutate_ticket_collaboration<F>(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
    expected_root: Option<&str>,
    operation_kind: &str,
    mutate: F,
) -> Result<TicketSummary>
where
    F: FnOnce(&mut IndexedTicketProfile<'_>, &Ticket, u64) -> Result<()>,
{
    loom.authorize_domain(workspace, AclDomain::Tickets, AclRight::Write)?;
    let mut profile = IndexedTicketProfile::open(loom, workspace, workspace_id)?;
    profile.enforce_expected_root(expected_root)?;
    let ticket_id = resolve_ticket_id(&profile, ticket_id)?;
    let ticket = profile
        .ticket(&ticket_id)?
        .ok_or_else(|| LoomError::corrupt("ticket resolved to a missing record"))?;
    let sequence = profile.next_sequence();
    mutate(&mut profile, &ticket, sequence)?;
    let payload = ticket.encode()?;
    let base_root = profile.profile_root()?;
    let root_after = profile.next_profile_root()?;
    let labels = ticket
        .policy_labels
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let record = operation_record(
        &profile,
        workspace,
        OperationRecordRequest {
            workspace_id,
            scope_id: &ticket.project_id,
            operation_kind,
            target_entity_id: Some(&ticket.ticket_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &labels,
            validation: None,
        },
    )?;
    let primary_key = ticket_key(&profile, &ticket)?;
    let mut summary = ticket_summary(
        workspace_id,
        &ticket,
        primary_key,
        Some(record.operation_id.clone()),
        Some(record.sequence),
        root_after,
        None,
    );
    summary.comments = compact_ticket_comments(&profile.comments(&ticket.ticket_id)?);
    let ticket_id = ticket.ticket_id.clone();
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    drop(profile);
    update_ticket_revision_index(loom, workspace, workspace_id, &ticket_id, &record, &payload)?;
    emit_ticket_change_notification(loom, workspace, workspace_id, &ticket_id, &record)?;
    Ok(summary)
}

fn ticket_actor(loom: &Loom<FileStore>, workspace: WorkspaceId) -> Result<String> {
    Ok(loom.effective_principal()?.unwrap_or(workspace).to_string())
}

fn emit_ticket_change_notification(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
    record: &TicketOperationRecord,
) -> Result<DeliveryEnvelope> {
    let payload = serde_json::json!({
        "workspace_id": workspace_id,
        "ticket_id": ticket_id,
        "operation_id": record.operation_id,
        "operation_kind": record.operation_kind,
        "sequence": record.sequence,
        "root_after": record.root_after.to_string(),
    });
    let payload = serde_json::to_vec(&payload)
        .map_err(|error| LoomError::invalid(format!("ticket notification payload: {error}")))?;
    let cursor = record.sequence.to_string();
    delivery_produce(
        loom,
        workspace,
        DeliveryProduceRequest {
            stream_id: &ticket_change_stream(workspace_id),
            producer: APP_ID,
            subject: &format!("ticket:{ticket_id}"),
            payload: &payload,
            created_at_ms: now_ms(),
            expires_at_ms: None,
            source_cursor: Some(cursor.as_bytes()),
        },
    )
}

fn ticket_change_stream(workspace_id: &str) -> String {
    format!("tickets:{workspace_id}:changes")
}

fn update_ticket_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
    record: &TicketOperationRecord,
    payload: &[u8],
) -> Result<()> {
    let index_path = revision_index_path(workspace_id)?;
    let index = match loom.read_file_reserved(workspace, &index_path) {
        Ok(bytes) => RevisionIndex::decode(&bytes)?,
        Err(e) if e.code == Code::NotFound => RevisionIndex::new(),
        Err(e) => return Err(e),
    };
    let envelope = OperationEnvelope::decode(&record.envelope)?;
    let entity_id = format!("ticket:{ticket_id}");
    let expected_latest_revision = index
        .latest(&entity_id)
        .map(|entry| entry.revision)
        .unwrap_or(0);
    let mut state = ProfileTransactionState::new(record.root_after, index);
    let update = ProfileRevisionUpdate::new(
        entity_id,
        record.operation_id.clone(),
        BodyRef::new(
            Digest::hash(loom.store().digest_algo(), payload),
            payload.len() as u64,
            "application/vnd.uldren.loom.ticket.ticket+cbor",
        )?,
        envelope.timestamp_ms,
        format!("{ticket_id}:{}", record.sequence),
        Some(expected_latest_revision),
    )?;
    state.apply(ProfileTransaction::new(
        workspace_id,
        None,
        record.root_after,
        vec![update],
    )?)?;
    let index = state.into_revision_index();
    loom.create_directory_reserved(workspace, REVISION_INDEX_DIR, true)?;
    loom.write_file_reserved(workspace, &index_path, &index.encode()?, 0o100644)
}

fn operation_record(
    profile: &IndexedTicketProfile<'_>,
    workspace: WorkspaceId,
    request: OperationRecordRequest<'_>,
) -> Result<TicketOperationRecord> {
    let sequence = profile.next_sequence();
    let operation_id = format!("{}:{sequence}", request.workspace_id);
    let actor_principal = profile.effective_principal()?.unwrap_or(workspace);
    let envelope = OperationEnvelope::new(
        profile.digest_algo(),
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
            base_root: request.base_root,
            base_entity_version: None,
            target_entity_id: request.target_entity_id,
            payload: request.payload,
            policy_labels: request.policy_labels,
            signature: None,
            agent: None,
        },
    )?;
    TicketOperationRecord::new(
        sequence,
        operation_id,
        request.operation_kind,
        request.target_entity_id.map(str::to_string),
        request.root_after,
        envelope.encode()?,
        request.validation.cloned(),
    )
}

fn persist_board_update(
    profile: &mut IndexedTicketProfile<'_>,
    workspace: WorkspaceId,
    workspace_id: &str,
    board: TicketBoard,
    operation_kind: &str,
) -> Result<BoardSummary> {
    let payload = board.encode()?;
    let base_root = profile.profile_root()?;
    profile.put_board(&board)?;
    let root_after = profile.next_profile_root()?;
    let record = operation_record(
        profile,
        workspace,
        OperationRecordRequest {
            workspace_id,
            scope_id: &board.board_id,
            operation_kind,
            target_entity_id: Some(&board.board_id),
            base_root,
            root_after,
            payload: &payload,
            policy_labels: &[],
            validation: None,
        },
    )?;
    profile.append_operation(&record)?;
    let persisted_root = profile.finish_operation()?;
    debug_assert_eq!(persisted_root, root_after);
    let cards = profile.board_cards(&board.board_id)?;
    Ok(board_summary(
        workspace_id,
        &board,
        cards,
        root_after,
        Some(record.operation_id),
        Some(record.sequence),
    ))
}

fn board_summary(
    workspace_id: &str,
    board: &TicketBoard,
    cards: Vec<BoardCardPlacement>,
    profile_root: Digest,
    operation_id: Option<String>,
    sequence: Option<u64>,
) -> BoardSummary {
    let mut cards = cards
        .into_iter()
        .map(|placement| BoardCardPlacementSummary {
            board_id: placement.board_id,
            ticket_id: placement.ticket_id,
            column_id: placement.column_id,
            rank_token: placement.rank_token,
            swimlane_id: placement.swimlane_id,
            updated_at: placement.updated_at,
            updated_by: placement.updated_by,
        })
        .collect::<Vec<_>>();
    cards.sort_by(|a, b| {
        a.column_id
            .cmp(&b.column_id)
            .then(a.rank_token.cmp(&b.rank_token))
            .then(a.ticket_id.cmp(&b.ticket_id))
    });
    BoardSummary {
        workspace_id: workspace_id.to_string(),
        board_id: board.board_id.clone(),
        board_key: board.board_key.clone(),
        name: board.name.clone(),
        description: board.description.clone(),
        project_id: board.project_id.clone(),
        scope: board_scope_json(&board.scope),
        mode: board.mode.as_str().to_string(),
        columns: board
            .columns
            .iter()
            .map(|column| BoardColumnSummary {
                column_id: column.column_id.clone(),
                name: column.name.clone(),
                mapped_statuses: column.mapped_statuses.iter().cloned().collect(),
                wip_limit: column.wip_limit,
                hidden: column.hidden,
                rank: column.rank,
            })
            .collect(),
        swimlanes: board
            .swimlanes
            .iter()
            .map(|swimlane| BoardSwimlaneSummary {
                swimlane_id: swimlane.swimlane_id.clone(),
                name: swimlane.name.clone(),
                predicate: swimlane.predicate.clone(),
                rank: swimlane.rank,
            })
            .collect(),
        card_display_fields: board.card_display_fields.clone(),
        owner_principal: board.owner_principal.clone(),
        coordinator_principal: board.coordinator_principal.clone(),
        board_status: board.board_status.as_str().to_string(),
        cards,
        profile_root: profile_root.to_string(),
        operation_id,
        sequence,
    }
}

fn board_scope_json(scope: &BoardScope) -> Value {
    match scope {
        BoardScope::Project { project_id } => serde_json::json!({
            "kind": "project",
            "project_id": project_id,
        }),
        BoardScope::Filter { filter_id } => serde_json::json!({
            "kind": "filter",
            "filter_id": filter_id,
        }),
        BoardScope::ManualSet => serde_json::json!({
            "kind": "manual_set",
        }),
    }
}

fn ticket_summary(
    workspace_id: &str,
    ticket: &Ticket,
    primary_key: String,
    operation_id: Option<String>,
    sequence: Option<u64>,
    profile_root: Digest,
    projection_selection: Option<&TicketProjectionSelection>,
) -> TicketSummary {
    let relations = compact_ticket_relations(ticket);
    let depends_on = relation_targets(ticket, TicketRelationKind::DependsOn);
    let blocks = relation_targets(ticket, TicketRelationKind::Blocks);
    let native_selection;
    let selection = match projection_selection {
        Some(selection) => selection,
        None => {
            native_selection = crate::ticket_projection_contract(TicketProjectionProfile::Native);
            return TicketSummary {
                workspace_id: workspace_id.to_string(),
                ticket_id: ticket.ticket_id.clone(),
                project_id: ticket.project_id.clone(),
                primary_key,
                ticket_type: ticket_type_name(ticket.ticket_type).to_string(),
                projection_profile: TicketProjectionProfile::Native.profile_id().to_string(),
                projection_kind: native_selection.tagged_response_kind.to_string(),
                projection_source: native_selection.source.to_string(),
                projection_selection_source: projection_selection_source_name(
                    TicketProjectionSelectionSource::MachineDefaultNative,
                )
                .to_string(),
                external_source: ticket
                    .external_identity
                    .as_ref()
                    .map(|identity| identity.source.clone()),
                external_id: ticket
                    .external_identity
                    .as_ref()
                    .map(|identity| identity.id.clone()),
                fields: ticket
                    .fields
                    .iter()
                    .map(|(key, value)| (projected_ticket_field_key(None, key), value.to_json()))
                    .collect(),
                policy_labels: ticket.policy_labels.iter().cloned().collect(),
                relations,
                relation_rollup: TicketRelationRollup::default(),
                depends_on,
                blocks,
                comments: Vec::new(),
                profile_root: profile_root.to_string(),
                operation_id,
                sequence,
            };
        }
    };
    TicketSummary {
        workspace_id: workspace_id.to_string(),
        ticket_id: ticket.ticket_id.clone(),
        project_id: ticket.project_id.clone(),
        primary_key,
        ticket_type: ticket_type_name(ticket.ticket_type).to_string(),
        projection_profile: selection.profile.profile_id().to_string(),
        projection_kind: selection.contract.tagged_response_kind.to_string(),
        projection_source: selection.contract.source.to_string(),
        projection_selection_source: projection_selection_source_name(selection.selection_source)
            .to_string(),
        external_source: ticket
            .external_identity
            .as_ref()
            .map(|identity| identity.source.clone()),
        external_id: ticket
            .external_identity
            .as_ref()
            .map(|identity| identity.id.clone()),
        fields: ticket
            .fields
            .iter()
            .map(|(key, value)| {
                (
                    projected_ticket_field_key(selection.profile_config.as_ref(), key),
                    value.to_json(),
                )
            })
            .collect(),
        policy_labels: ticket.policy_labels.iter().cloned().collect(),
        relations,
        relation_rollup: TicketRelationRollup::default(),
        depends_on,
        blocks,
        comments: Vec::new(),
        profile_root: profile_root.to_string(),
        operation_id,
        sequence,
    }
}

fn compact_ticket_comments(comments: &[TicketComment]) -> Vec<TicketCommentCompact> {
    comments
        .iter()
        .map(|comment| TicketCommentCompact {
            comment_id: comment.comment_id.clone(),
            comment_type: comment.comment_type.clone(),
            author_principal: comment.author_principal.clone(),
            created_at_ms: comment.created_at_ms,
            updated_at_ms: comment.updated_at_ms,
            redacted: comment.redacted,
        })
        .collect()
}

fn compact_ticket_relations(ticket: &Ticket) -> Vec<TicketRelationCompact> {
    ticket
        .relations
        .values()
        .map(|relation| TicketRelationCompact {
            relation_id: relation.relation_id.clone(),
            kind: relation.kind.as_str().to_string(),
            target_type: relation.target_type.as_str().to_string(),
            target_id: relation.target_id.clone(),
            target: None,
        })
        .collect()
}

fn relation_targets(ticket: &Ticket, kind: TicketRelationKind) -> Vec<String> {
    ticket
        .relations
        .values()
        .filter(|relation| relation.kind == kind)
        .map(|relation| relation.target_id.clone())
        .collect()
}

fn enrich_ticket_relation_projection(
    profile: &impl TicketProfileLookup,
    summary: &mut TicketSummary,
) -> Result<()> {
    let mut rollup = TicketRelationRollup::default();
    for relation in &mut summary.relations {
        if relation.target_type != TicketRelationTargetType::Ticket.as_str() {
            continue;
        }
        let Some(target) = profile.ticket(&relation.target_id)? else {
            continue;
        };
        let status = ticket_status_value(&target);
        let blocked = status.as_deref() == Some("blocked");
        relation.target = Some(TicketRelationTargetState {
            primary_key: ticket_key(profile, &target)?,
            title: crate::text_like_field(&target.fields, "title"),
            status: status.clone(),
            blocked,
        });
        if relation.kind == TicketRelationKind::ParentOf.as_str() {
            rollup.total_children += 1;
            match status.as_deref() {
                Some("accepted") => rollup.accepted_children += 1,
                Some("blocked") => rollup.blocked_children += 1,
                Some("waiting_for_review") => rollup.waiting_for_review_children += 1,
                Some("feedback_available") => rollup.feedback_available_children += 1,
                Some("in_progress") => rollup.in_progress_children += 1,
                _ => {}
            }
        }
    }
    summary.relation_rollup = rollup;
    Ok(())
}

fn ticket_summary_public_json(summary: &TicketSummary) -> Value {
    match summary.projection_profile.as_str() {
        "jira" => {
            let mut object = Map::new();
            object.insert(
                "projection".to_string(),
                Value::String(summary.projection_profile.clone()),
            );
            object.insert("id".to_string(), Value::String(summary.ticket_id.clone()));
            object.insert(
                "key".to_string(),
                Value::String(summary.primary_key.clone()),
            );
            object.insert(
                "fields".to_string(),
                prefixed_fields(&summary.fields, "fields."),
            );
            Value::Object(object)
        }
        "asana" => {
            let mut object = Map::new();
            object.insert(
                "projection".to_string(),
                Value::String(summary.projection_profile.clone()),
            );
            object.insert(
                "gid".to_string(),
                Value::String(
                    summary
                        .external_id
                        .clone()
                        .unwrap_or_else(|| summary.ticket_id.clone()),
                ),
            );
            object.insert(
                "data".to_string(),
                prefixed_fields(&summary.fields, "data."),
            );
            Value::Object(object)
        }
        "notion" => {
            let mut object = Map::new();
            object.insert(
                "projection".to_string(),
                Value::String(summary.projection_profile.clone()),
            );
            object.insert("id".to_string(), Value::String(summary.ticket_id.clone()));
            object.insert(
                "properties".to_string(),
                nested_prefixed_fields(&summary.fields, "properties."),
            );
            Value::Object(object)
        }
        "redmine" => {
            let mut object = Map::new();
            object.insert(
                "projection".to_string(),
                Value::String(summary.projection_profile.clone()),
            );
            object.insert(
                "issue".to_string(),
                prefixed_fields(&summary.fields, "issue."),
            );
            Value::Object(object)
        }
        _ => {
            let mut object = Map::new();
            object.insert(
                "workspace_id".to_string(),
                Value::String(summary.workspace_id.clone()),
            );
            object.insert(
                "ticket_id".to_string(),
                Value::String(summary.ticket_id.clone()),
            );
            object.insert(
                "project_id".to_string(),
                Value::String(summary.project_id.clone()),
            );
            object.insert(
                "primary_key".to_string(),
                Value::String(summary.primary_key.clone()),
            );
            object.insert(
                "ticket_type".to_string(),
                Value::String(summary.ticket_type.clone()),
            );
            object.insert(
                "projection".to_string(),
                Value::String(summary.projection_profile.clone()),
            );
            object.insert(
                "external_source".to_string(),
                summary
                    .external_source
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            object.insert(
                "external_id".to_string(),
                summary
                    .external_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            object.insert(
                "fields".to_string(),
                Value::Object(summary.fields.clone().into_iter().collect()),
            );
            object.insert(
                "policy_labels".to_string(),
                Value::Array(
                    summary
                        .policy_labels
                        .iter()
                        .cloned()
                        .map(Value::String)
                        .collect(),
                ),
            );
            object.insert(
                "relations".to_string(),
                serde_json::to_value(&summary.relations).unwrap_or(Value::Null),
            );
            object.insert(
                "relation_rollup".to_string(),
                serde_json::to_value(&summary.relation_rollup).unwrap_or(Value::Null),
            );
            object.insert(
                "depends_on".to_string(),
                Value::Array(
                    summary
                        .depends_on
                        .iter()
                        .cloned()
                        .map(Value::String)
                        .collect(),
                ),
            );
            object.insert(
                "blocks".to_string(),
                Value::Array(summary.blocks.iter().cloned().map(Value::String).collect()),
            );
            object.insert(
                "comments".to_string(),
                serde_json::to_value(&summary.comments).unwrap_or(Value::Null),
            );
            object.insert(
                "profile_root".to_string(),
                Value::String(summary.profile_root.clone()),
            );
            object.insert(
                "operation_id".to_string(),
                summary
                    .operation_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            object.insert(
                "sequence".to_string(),
                summary
                    .sequence
                    .map(|value| Value::Number(value.into()))
                    .unwrap_or(Value::Null),
            );
            Value::Object(object)
        }
    }
}

fn prefixed_fields(fields: &BTreeMap<String, Value>, prefix: &str) -> Value {
    let mut object = Map::new();
    for (field, value) in fields {
        object.insert(
            field
                .strip_prefix(prefix)
                .unwrap_or(field.as_str())
                .to_string(),
            value.clone(),
        );
    }
    Value::Object(object)
}

fn nested_prefixed_fields(fields: &BTreeMap<String, Value>, prefix: &str) -> Value {
    let mut object = Map::new();
    for (field, value) in fields {
        insert_nested_value(
            &mut object,
            field.strip_prefix(prefix).unwrap_or(field.as_str()),
            value.clone(),
        );
    }
    Value::Object(object)
}

fn insert_nested_value(object: &mut Map<String, Value>, path: &str, value: Value) {
    let mut segments = path.split('.').peekable();
    let mut current = object;
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            current.insert(segment.to_string(), value);
            break;
        }
        current = current
            .entry(segment.to_string())
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("inserted object remains an object");
    }
}

fn ticket_projection_selection(
    profile: &TicketProfileReader<'_>,
    ticket: &Ticket,
    requested_projection: Option<TicketProjectionProfile>,
) -> Result<TicketProjectionSelection> {
    let project = profile
        .project(&ticket.project_id)?
        .ok_or_else(|| LoomError::corrupt("ticket references missing project"))?;
    project.projection_config.select(
        TicketProjectionRequestContext::MachineApi,
        requested_projection,
    )
}

fn projected_ticket_field_key(
    profile_config: Option<&crate::TicketProjectionProfileConfig>,
    field_id: &str,
) -> String {
    profile_config
        .and_then(|config| config.field_aliases.get(field_id))
        .cloned()
        .unwrap_or_else(|| field_id.to_string())
}

fn require_ticket_field_object(fields: &Value) -> Result<&serde_json::Map<String, Value>> {
    fields
        .as_object()
        .ok_or_else(|| LoomError::invalid("ticket fields must be a JSON object"))
}

fn normalize_jira_fields(fields: &Value) -> Result<Value> {
    let object = require_ticket_field_object(fields)?;
    let mut normalized = serde_json::Map::new();
    if let Some(fields_object) = object.get("fields").and_then(Value::as_object) {
        insert_projected_fields(
            &mut normalized,
            fields_object,
            Some(TicketProjectionProfile::Jira),
        );
    } else {
        insert_projected_fields(&mut normalized, object, Some(TicketProjectionProfile::Jira));
    }
    if let Some(update_object) = object.get("update").and_then(Value::as_object) {
        for (field, operation) in update_object {
            if let Some(value) = jira_update_set_value(operation) {
                normalized.insert(
                    projected_input_field_key(field, Some(TicketProjectionProfile::Jira))
                        .to_string(),
                    value,
                );
            }
        }
    }
    if let Some(status) = object
        .get("transition")
        .and_then(projected_status_from_object)
    {
        normalized.insert("status".to_string(), Value::String(status));
    }
    Ok(Value::Object(normalized))
}

fn normalize_asana_fields(fields: &Value) -> Result<Value> {
    let object = require_ticket_field_object(fields)?;
    let data = object
        .get("data")
        .and_then(Value::as_object)
        .unwrap_or(object);
    let mut normalized = serde_json::Map::new();
    insert_projected_fields(&mut normalized, data, Some(TicketProjectionProfile::Asana));
    if let Some(completed) = data.get("completed").and_then(Value::as_bool)
        && completed
    {
        normalized.insert("status".to_string(), Value::String("accepted".to_string()));
    }
    if let Some(status) = data.get("status").and_then(projected_status_from_value) {
        normalized.insert("status".to_string(), Value::String(status));
    }
    Ok(Value::Object(normalized))
}

fn normalize_notion_fields(fields: &Value) -> Result<Value> {
    let object = require_ticket_field_object(fields)?;
    let properties = object
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(object);
    let mut normalized = serde_json::Map::new();
    for (field, value) in properties {
        let native_field = projected_input_field_key(field, Some(TicketProjectionProfile::Notion));
        let normalized_value = if native_field == "status" {
            projected_status_from_value(value)
                .map(Value::String)
                .unwrap_or_else(|| notion_property_value(value).unwrap_or_else(|| value.clone()))
        } else {
            notion_property_value(value).unwrap_or_else(|| value.clone())
        };
        normalized.insert(native_field.to_string(), normalized_value);
    }
    Ok(Value::Object(normalized))
}

fn normalize_redmine_fields(fields: &Value) -> Result<Value> {
    let object = require_ticket_field_object(fields)?;
    let issue = object
        .get("issue")
        .and_then(Value::as_object)
        .unwrap_or(object);
    let mut normalized = serde_json::Map::new();
    insert_projected_fields(
        &mut normalized,
        issue,
        Some(TicketProjectionProfile::Redmine),
    );
    if let Some(status) = issue
        .get("status_id")
        .or_else(|| issue.get("status"))
        .and_then(projected_status_from_value)
    {
        normalized.insert("status".to_string(), Value::String(status));
    }
    Ok(Value::Object(normalized))
}

fn projected_status_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(status) => Some(status.clone()),
        Value::Object(_) => projected_status_from_object(value),
        _ => None,
    }
}

fn projected_status_from_object(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    object
        .get("name")
        .or_else(|| object.get("id"))
        .or_else(|| object.get("value"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| object.get("status").and_then(projected_status_from_value))
        .or_else(|| object.get("to").and_then(projected_status_from_value))
}

fn insert_projected_fields(
    target: &mut serde_json::Map<String, Value>,
    source: &serde_json::Map<String, Value>,
    projection: Option<TicketProjectionProfile>,
) {
    for (field, value) in source {
        target.insert(
            projected_input_field_key(field, projection).to_string(),
            value.clone(),
        );
    }
}

fn projected_input_field_key(field: &str, projection: Option<TicketProjectionProfile>) -> &str {
    match projection {
        Some(TicketProjectionProfile::Jira) => match field {
            "fields.summary" | "summary" => "title",
            "fields.description" => "description",
            "fields.status" | "transition.to.name" => "status",
            "fields.assignee" => "assignee",
            "fields.reporter" => "reporter",
            "fields.priority" => "priority",
            "fields.resolution" => "resolution",
            value => value.strip_prefix("fields.").unwrap_or(value),
        },
        Some(TicketProjectionProfile::Asana) => match field {
            "name" => "title",
            "notes" | "html_notes" => "description",
            "assignee" => "assignee",
            "due_on" | "due_at" => "due_date",
            value => value,
        },
        Some(TicketProjectionProfile::Notion) => match field {
            "Name" | "title" | "properties.Name.title" => "title",
            "Description" | "description" => "description",
            "Status" | "status" => "status",
            "Assignee" | "assignee" => "assignee",
            "Priority" | "priority" => "priority",
            "Due" | "Due date" | "due_date" => "due_date",
            value => value,
        },
        Some(TicketProjectionProfile::Redmine) => match field {
            "subject" => "title",
            "description" => "description",
            "status_id" | "status" => "status",
            "assigned_to_id" | "assigned_to" => "assignee",
            "priority_id" | "priority" => "priority",
            "due_date" => "due_date",
            value => value,
        },
        Some(TicketProjectionProfile::Native) | None => field,
    }
}

fn jira_update_set_value(operation: &Value) -> Option<Value> {
    let operations = operation.as_array()?;
    if let Some(entry) = operations.iter().next() {
        let set_value = entry.as_object()?.get("set")?;
        return Some(set_value.clone());
    }
    None
}

fn notion_property_value(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    if let Some(text) = object.get("title").and_then(notion_rich_text) {
        return Some(Value::String(text));
    }
    if let Some(text) = object.get("rich_text").and_then(notion_rich_text) {
        return Some(Value::String(text));
    }
    if let Some(select) = object
        .get("select")
        .and_then(Value::as_object)
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
    {
        return Some(Value::String(select.to_string()));
    }
    if let Some(status) = object
        .get("status")
        .and_then(Value::as_object)
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
    {
        return Some(Value::String(status.to_string()));
    }
    if let Some(date) = object.get("date").and_then(Value::as_object)
        && let Some(start) = date.get("start").and_then(Value::as_str)
    {
        return Some(Value::String(start.to_string()));
    }
    object
        .get("number")
        .or_else(|| object.get("checkbox"))
        .or_else(|| object.get("url"))
        .cloned()
}

fn notion_rich_text(value: &Value) -> Option<String> {
    let parts = value.as_array()?;
    let mut text = String::new();
    for part in parts {
        if let Some(value) = part.get("plain_text").and_then(Value::as_str) {
            text.push_str(value);
        } else if let Some(value) = part
            .get("text")
            .and_then(Value::as_object)
            .and_then(|text| text.get("content"))
            .and_then(Value::as_str)
        {
            text.push_str(value);
        }
    }
    Some(text)
}

fn projection_selection_source_name(source: TicketProjectionSelectionSource) -> &'static str {
    match source {
        TicketProjectionSelectionSource::ExplicitRequest => "explicit_request",
        TicketProjectionSelectionSource::ProjectDefaultDisplay => "project_default_display",
        TicketProjectionSelectionSource::MachineDefaultNative => "machine_default_native",
    }
}

fn project_contracts_summary(
    project: &TicketProject,
    include_details: bool,
) -> TicketProjectContractsSummary {
    TicketProjectContractsSummary {
        note: project.contracts.note.clone(),
        owner: TicketProjectContractSummary {
            summary: project.contracts.owner.summary.clone(),
            details: include_details.then(|| project.contracts.owner.details.clone()),
        },
        worker: TicketProjectContractSummary {
            summary: project.contracts.worker.summary.clone(),
            details: include_details.then(|| project.contracts.worker.details.clone()),
        },
    }
}

fn project_summary(
    workspace_id: &str,
    project: &TicketProject,
    profile_root: Digest,
    operation_id: String,
    sequence: u64,
    include_contract_details: bool,
) -> TicketProjectSummary {
    TicketProjectSummary {
        workspace_id: workspace_id.to_string(),
        project_id: project.project_id.clone(),
        key_prefix: project.key_prefix.clone(),
        name: project.name.clone(),
        next_ticket_number: project.next_ticket_number,
        default_projection: project
            .projection_config
            .default_display_projection
            .profile_id()
            .to_string(),
        enabled_projections: project
            .projection_config
            .enabled_projections
            .iter()
            .map(|profile| profile.profile_id().to_string())
            .collect(),
        lifecycle_authorization_policy: project.lifecycle_authorization_policy.as_str().to_string(),
        project_owner_principal: project.project_owner_principal.clone(),
        acceptance_authorities: project.acceptance_authorities.iter().cloned().collect(),
        acceptance_evidence_enforcement: project.acceptance_evidence_policy.enforcement_enabled,
        required_acceptance_evidence_keys: project
            .acceptance_evidence_policy
            .required_keys
            .iter()
            .map(|key| key.as_str().to_string())
            .collect(),
        contracts: project_contracts_summary(project, include_contract_details),
        active_workflow_version: project
            .active_workflow
            .as_ref()
            .map(|workflow| workflow.version.clone()),
        profile_root: profile_root.to_string(),
        operation_id,
        sequence,
    }
}

#[allow(clippy::too_many_arguments)]
fn relation_summary(
    workspace_id: &str,
    ticket_id: &str,
    relation: &TicketRelation,
    graph_edge_id: String,
    profile_root: Digest,
    operation_id: String,
    sequence: u64,
) -> TicketRelationSummary {
    TicketRelationSummary {
        workspace_id: workspace_id.to_string(),
        ticket_id: ticket_id.to_string(),
        relation_id: relation.relation_id.clone(),
        kind: relation.kind.as_str().to_string(),
        target_type: relation.target_type.as_str().to_string(),
        target_id: relation.target_id.clone(),
        graph_edge_id,
        profile_root: profile_root.to_string(),
        operation_id,
        sequence,
    }
}

fn resolve_ticket_id(profile: &impl TicketProfileLookup, value: &str) -> Result<String> {
    if profile.ticket(value)?.is_some() {
        return Ok(value.to_string());
    }
    profile
        .resolve_ticket_key(value)?
        .map(|resolution| resolution.ticket_id)
        .ok_or_else(|| LoomError::not_found("ticket not found"))
}

fn normalize_relation_target(
    profile: &impl TicketProfileLookup,
    target_type: TicketRelationTargetType,
    target_id: &str,
) -> Result<String> {
    match target_type {
        TicketRelationTargetType::Ticket => resolve_ticket_id(profile, target_id),
        TicketRelationTargetType::Principal => {
            loom_substrate::validate_text("ticket relation principal target", target_id)?;
            Ok(target_id.to_string())
        }
        TicketRelationTargetType::Page
        | TicketRelationTargetType::Document
        | TicketRelationTargetType::Prompt
        | TicketRelationTargetType::Result
        | TicketRelationTargetType::Decision => {
            loom_substrate::validate_text("ticket relation target", target_id)?;
            Ok(target_id.to_string())
        }
    }
}

fn default_relation_id(
    kind: TicketRelationKind,
    target_type: TicketRelationTargetType,
    target_id: &str,
) -> String {
    format!("{}:{}:{}", kind.as_str(), target_type.as_str(), target_id)
}

fn relation_node_props(target_type: &str, target_id: &str) -> loom_core::graph::Props {
    BTreeMap::from([
        (
            "target_type".to_string(),
            GraphValue::Text(target_type.to_string()),
        ),
        (
            "target_id".to_string(),
            GraphValue::Text(target_id.to_string()),
        ),
    ])
}

fn relation_edge_props(
    source_ticket_id: &str,
    relation: &TicketRelation,
) -> loom_core::graph::Props {
    BTreeMap::from([
        (
            "derived_from".to_string(),
            GraphValue::Text("tickets".to_string()),
        ),
        (
            "source_ticket_id".to_string(),
            GraphValue::Text(source_ticket_id.to_string()),
        ),
        (
            "relation_id".to_string(),
            GraphValue::Text(relation.relation_id.clone()),
        ),
        (
            "target_type".to_string(),
            GraphValue::Text(relation.target_type.as_str().to_string()),
        ),
        (
            "target_id".to_string(),
            GraphValue::Text(relation.target_id.clone()),
        ),
    ])
}

fn ticket_external_identity(
    source: Option<&str>,
    id: Option<&str>,
) -> Result<Option<ExternalTicketIdentity>> {
    match (source, id) {
        (None, None) => Ok(None),
        (Some(source), Some(id)) => ExternalTicketIdentity::new(source, id).map(Some),
        _ => Err(LoomError::invalid(
            "external_source and external_id must be provided together",
        )),
    }
}

fn authorize_transition(
    project: &TicketProject,
    ticket: &Ticket,
    actor: &str,
    target_status: &str,
) -> Result<()> {
    match project.lifecycle_authorization_policy {
        TicketLifecycleAuthorizationPolicy::WriteAccess => Ok(()),
        TicketLifecycleAuthorizationPolicy::Assignee => {
            authorize_assignee_transition(ticket, actor)
        }
        TicketLifecycleAuthorizationPolicy::ReviewAuthority => {
            if is_review_authority_transition(target_status) {
                authorize_review_authority(project, actor)
            } else {
                Ok(())
            }
        }
        TicketLifecycleAuthorizationPolicy::OwnershipGoverned => {
            if target_status == "in_progress" && ticket_assignee(ticket).is_none() {
                return Ok(());
            }
            if is_review_authority_transition(target_status) {
                return authorize_review_authority(project, actor);
            }
            authorize_assignee_transition(ticket, actor)
        }
    }
}

fn is_review_authority_transition(target_status: &str) -> bool {
    matches!(target_status, "accepted" | "rejected")
}

fn authorize_review_authority(project: &TicketProject, actor: &str) -> Result<()> {
    if project.project_owner_principal.as_deref() == Some(actor)
        || project.acceptance_authorities.contains(actor)
    {
        return Ok(());
    }
    Err(LoomError::new(
        Code::PermissionDenied,
        "ticket lifecycle action requires project owner or acceptance authority",
    ))
}

fn enforce_acceptance_evidence(
    project: &TicketProject,
    target_status: &str,
    existing_comments: &[TicketComment],
    update_comments: &[TicketUpdateCommentRequest<'_>],
) -> Result<()> {
    if target_status != "accepted" || !project.acceptance_evidence_policy.enforcement_enabled {
        return Ok(());
    }
    for key in &project.acceptance_evidence_policy.required_keys {
        let field_name = key.as_str();
        if comment_evidence_value_present(*key, existing_comments, update_comments) {
            continue;
        }
        return Err(LoomError::new(
            Code::InvalidArgument,
            format!("ticket acceptance requires structured evidence key `{field_name}`"),
        ));
    }
    Ok(())
}

fn comment_evidence_value_present(
    key: TicketAcceptanceEvidenceKey,
    existing_comments: &[TicketComment],
    update_comments: &[TicketUpdateCommentRequest<'_>],
) -> bool {
    existing_comments
        .iter()
        .filter_map(|comment| comment.evidence.as_ref())
        .any(|evidence| evidence.has_key_value(key))
        || update_comments
            .iter()
            .filter_map(|comment| comment.evidence.as_ref())
            .any(|evidence| evidence.has_key_value(key))
}

fn authorize_assignee_transition(ticket: &Ticket, actor: &str) -> Result<()> {
    if ticket_assignee(ticket).as_deref() == Some(actor) {
        Ok(())
    } else {
        Err(LoomError::new(
            Code::PermissionDenied,
            "ticket lifecycle action requires ticket assignee",
        ))
    }
}

#[cfg(test)]
fn authorize_lifecycle_action(
    project: &TicketProject,
    ticket: &Ticket,
    actor: &str,
    action: TicketLifecycleAction,
) -> Result<()> {
    let target_status = legacy_action_target_status(action)
        .ok_or_else(|| LoomError::invalid("ticket lifecycle action has no target status"))?;
    authorize_transition(project, ticket, actor, target_status)
}

#[cfg(test)]
fn enforce_lifecycle_transition(ticket: &Ticket, action: TicketLifecycleAction) -> Result<()> {
    let status = ticket_status(ticket);
    let Some(target_status) = legacy_action_target_status(action) else {
        return Err(LoomError::new(
            Code::Conflict,
            "ticket lifecycle action has no target status",
        ));
    };
    let workflow = default_ticket_workflow()?;
    let transition = TransitionOperation {
        operation_id: "validation".to_string(),
        actor_principal: "validation".to_string(),
        target_status: target_status.to_string(),
        observed_source_status: status.clone(),
        observed_workflow_version: workflow.version.clone(),
        attached_fields: BTreeMap::new(),
    };
    let record = validate_transition(
        Some(&workflow),
        &status,
        &ticket.fields,
        &transition,
        &WorkflowValidationContext::default(),
    )?;
    if record.validation_state == WorkflowValidationState::Applied {
        Ok(())
    } else {
        Err(LoomError::new(
            Code::Conflict,
            "ticket lifecycle action is not valid from current status",
        ))
    }
}

fn apply_transition_fields(
    ticket: &mut Ticket,
    actor: &str,
    target_status: &str,
    requested_assignee: Option<&str>,
) -> Result<()> {
    ticket.fields.insert(
        "status".to_string(),
        TicketFieldValue::EnumOption(target_status.to_string()),
    );
    match target_status {
        "in_progress" => {
            let assignee = requested_assignee.unwrap_or(actor);
            loom_substrate::validate_text("ticket assignee", assignee)?;
            ticket.fields.insert(
                "assignee".to_string(),
                TicketFieldValue::Principal(assignee.to_string()),
            );
        }
        "ready" => {
            ticket
                .fields
                .insert("assignee".to_string(), TicketFieldValue::Null);
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
fn apply_lifecycle_fields(
    ticket: &mut Ticket,
    actor: &str,
    action: TicketLifecycleAction,
    requested_assignee: Option<&str>,
) -> Result<()> {
    let target_status = legacy_action_target_status(action)
        .ok_or_else(|| LoomError::invalid("ticket lifecycle action has no target status"))?;
    apply_transition_fields(ticket, actor, target_status, requested_assignee)
}

fn ticket_status(ticket: &Ticket) -> String {
    match ticket.fields.get("status") {
        Some(TicketFieldValue::Principal(value))
        | Some(TicketFieldValue::String(value))
        | Some(TicketFieldValue::EnumOption(value)) => value.clone(),
        _ => "planned".to_string(),
    }
}

fn ticket_assignee(ticket: &Ticket) -> Option<String> {
    match ticket.fields.get("assignee") {
        Some(TicketFieldValue::Principal(value))
        | Some(TicketFieldValue::String(value))
        | Some(TicketFieldValue::EnumOption(value)) => Some(value.clone()),
        _ => None,
    }
}

/// shared resolver. Map a stored canonical principal id to its user-facing display alias
/// (the registered handle) for read projections. When `id` parses as a canonical principal id and
/// is registered in the identity store, its handle is returned; otherwise the input is returned
/// unchanged. This never errors: an unknown, unparseable, or unregistered id falls back to itself.
/// This is the single display resolver reused across every user-facing surface (CLI, MCP, lanes,
/// projection profiles); do not duplicate the lookup per surface.
pub fn resolve_principal_display(identity: Option<&IdentityStore>, id: &str) -> String {
    if let Some(identity) = identity
        && let Ok(principal_id) = WorkspaceId::parse(id)
        && let Ok(principal) = identity.principal(principal_id)
    {
        return principal.handle.clone();
    }
    id.to_string()
}

/// write-path normalization. Resolve a user-supplied assignee alias/handle to the canonical
/// principal id for persistence. When the identity store resolves the handle, the canonical id
/// string is persisted (source of truth); otherwise the input is stored verbatim. Unknown values
/// are never rejected.
fn canonicalize_assignee(loom: &Loom<FileStore>, assignee: &str) -> String {
    if let Some(identity) = loom.identity_store()
        && let Ok(Some(principal_id)) = identity.resolve_handle(assignee)
    {
        return principal_id.to_string();
    }
    assignee.to_string()
}

/// Canonicalize the `assignee` field of a create/import field map in place, preserving the field as
/// a `Principal` value. No-op when there is no text-like assignee value.
fn canonicalize_assignee_field(
    loom: &Loom<FileStore>,
    fields: &mut BTreeMap<String, TicketFieldValue>,
) {
    let current = match fields.get("assignee") {
        Some(TicketFieldValue::Principal(value))
        | Some(TicketFieldValue::String(value))
        | Some(TicketFieldValue::EnumOption(value)) => Some(value.clone()),
        _ => None,
    };
    if let Some(value) = current {
        let canonical = canonicalize_assignee(loom, &value);
        fields.insert(
            "assignee".to_string(),
            TicketFieldValue::Principal(canonical),
        );
    }
}

/// Inject the additive `assignee_display` alias into an already-projected ticket summary.
/// The canonical `assignee` field is left untouched (it remains the source of truth); the display
/// alias is added alongside it and flows through every projection profile at serialize time.
fn attach_assignee_display(
    identity: Option<&IdentityStore>,
    ticket: &Ticket,
    summary: &mut TicketSummary,
) {
    if let Some(canonical) = ticket_assignee(ticket) {
        let display = resolve_principal_display(identity, &canonical);
        summary
            .fields
            .insert("assignee_display".to_string(), Value::String(display));
    }
}

fn allocate_ticket_id(profile: &IndexedTicketProfile<'_>) -> Result<String> {
    for _ in 0..16 {
        let ticket_id = uuid::Uuid::new_v4().to_string();
        if profile.ticket(&ticket_id)?.is_none() {
            return Ok(ticket_id);
        }
    }
    Err(LoomError::new(
        Code::Conflict,
        "could not allocate a unique ticket UUID",
    ))
}

pub fn update_ticket_field_references(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    ticket_id: &str,
    fields: &BTreeMap<String, Value>,
) -> Result<()> {
    let mut index =
        loom_reference::load_index(loom, workspace)?.unwrap_or_else(ReferenceIndex::new);
    let prefix = ReferenceSource::new("tickets", workspace_id, ticket_id, "fields")?;
    index.remove_sources_matching(|source| {
        source == &prefix
            || (source.facet == "tickets"
                && source.collection == workspace_id
                && source.entity_id == ticket_id)
    });
    for (field, value) in fields {
        let Some(text) = json_field_text(value) else {
            continue;
        };
        let source = ReferenceSource::new("tickets", workspace_id, ticket_id, field)?;
        index.add_text_refs(source, "refers_to", &text)?;
    }
    loom_reference::save_index(loom, workspace, &index)?;
    loom_reference::project_reference_index_edges(loom, workspace, &index)
}

pub fn enqueue_ticket_reference_candidates(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: TicketReferenceCandidateRequest<'_>,
) -> Result<bool> {
    let mut queued = false;
    for (field, value) in request.fields {
        let Some(text) = json_field_text(value) else {
            continue;
        };
        let source =
            ReferenceSource::new("tickets", request.workspace_id, request.ticket_id, field)?;
        for (span_start, span_end, key, evidence) in ticket_key_occurrences(&text) {
            if let Some(profile) = TicketProfileReader::open(loom, workspace, request.workspace_id)?
                && let Some(resolution) = profile.resolve_ticket_key(&key)?
            {
                let mut index = loom_reference::load_index(loom, workspace)?
                    .unwrap_or_else(ReferenceIndex::new);
                index.add(ReferenceEdge::new(
                    source.clone(),
                    EntityRef::parse(&format!("ticket:{}", resolution.ticket_id))?,
                    "refers_to",
                    span_start,
                    span_end,
                    evidence.clone(),
                )?);
                loom_reference::save_index(loom, workspace, &index)?;
                loom_reference::project_reference_index_edges(loom, workspace, &index)?;
                continue;
            }
            let candidate_id = format!("{}:{field}:{span_start}", request.operation_id);
            let candidate =
                UnresolvedReference::new(loom_substrate::refs::UnresolvedReferenceInput {
                    candidate_id,
                    source: source.clone(),
                    source_operation_id: request.operation_id.to_string(),
                    source_root: request.source_root,
                    alias_text: evidence,
                    relation: "refers_to".to_string(),
                    span_start: span_start as u64,
                    span_end: span_end as u64,
                    evidence: text.clone(),
                    next_attempt_ms: request.now_ms,
                })?;
            loom_reference::enqueue(loom, workspace, &candidate)?;
            queued = true;
        }
    }
    Ok(queued)
}

trait TicketProfileLookup {
    fn project(&self, project_id: &str) -> Result<Option<TicketProject>>;
    fn ticket(&self, ticket_id: &str) -> Result<Option<Ticket>>;
    fn resolve_ticket_key(&self, value: &str) -> Result<Option<crate::TicketKeyResolution>>;
}

impl TicketProfileLookup for IndexedTicketProfile<'_> {
    fn project(&self, project_id: &str) -> Result<Option<TicketProject>> {
        self.project(project_id)
    }

    fn ticket(&self, ticket_id: &str) -> Result<Option<Ticket>> {
        self.ticket(ticket_id)
    }

    fn resolve_ticket_key(&self, value: &str) -> Result<Option<crate::TicketKeyResolution>> {
        self.resolve_ticket_key(value)
    }
}

impl TicketProfileLookup for TicketProfileReader<'_> {
    fn project(&self, project_id: &str) -> Result<Option<TicketProject>> {
        self.project(project_id)
    }

    fn ticket(&self, ticket_id: &str) -> Result<Option<Ticket>> {
        self.ticket(ticket_id)
    }

    fn resolve_ticket_key(&self, value: &str) -> Result<Option<crate::TicketKeyResolution>> {
        self.resolve_ticket_key(value)
    }
}

fn ticket_key(profile: &impl TicketProfileLookup, ticket: &Ticket) -> Result<String> {
    let project = profile
        .project(&ticket.project_id)?
        .ok_or_else(|| LoomError::corrupt("ticket project is missing"))?;
    Ok(project.ticket_key(ticket.ticket_number)?.canonical())
}

fn parse_ticket_type(value: &str) -> Result<TicketType> {
    TicketType::from_type_id(value)
}

fn ticket_type_name(ticket_type: TicketType) -> &'static str {
    ticket_type.type_id()
}

fn fields_from_json(value: &Value) -> Result<BTreeMap<String, TicketFieldValue>> {
    let Value::Object(map) = value else {
        return Err(LoomError::invalid("ticket fields must be a JSON object"));
    };
    map.iter()
        .map(|(key, value)| Ok((key.clone(), TicketFieldValue::from_json(value)?)))
        .collect()
}

fn validate_ticket_fields_against_project(
    project: &TicketProject,
    ticket_type: &str,
    fields: &BTreeMap<String, TicketFieldValue>,
    require_custom_fields: bool,
) -> Result<()> {
    for field in fields.keys() {
        validate_ticket_field_name(field)?;
        if is_core_ticket_field(field) {
            continue;
        }
        let Some(definition) = project.custom_field_definitions.get(field) else {
            return Err(LoomError::invalid(format!(
                "unknown ticket field `{field}`; inspect tickets fields and create a project custom-field definition before writing it"
            )));
        };
        if !definition.is_applicable(&project.project_id, ticket_type) {
            return Err(LoomError::invalid(format!(
                "ticket field `{field}` is not applicable to this ticket type"
            )));
        }
        let Some(value) = fields.get(field) else {
            continue;
        };
        definition.validate_ticket_value(value)?;
    }
    if require_custom_fields {
        for definition in project.custom_field_definitions.values() {
            if definition.retired
                || !definition.definition.required
                || !definition.is_applicable(&project.project_id, ticket_type)
            {
                continue;
            }
            if !fields.contains_key(&definition.definition.field_id) {
                return Err(LoomError::invalid(format!(
                    "required ticket field `{}` is missing",
                    definition.definition.field_id
                )));
            }
        }
    }
    Ok(())
}

fn validate_ticket_update_fields_against_project(
    project: &TicketProject,
    ticket: &Ticket,
    set_fields: &BTreeMap<String, TicketFieldValue>,
    delete_fields: &[String],
) -> Result<()> {
    validate_ticket_fields_against_project(
        project,
        ticket.ticket_type.type_id(),
        set_fields,
        false,
    )?;
    for field in delete_fields {
        validate_ticket_field_name(field)?;
        if is_core_ticket_field(field) || ticket.fields.contains_key(field) {
            continue;
        }
        let Some(definition) = project.custom_field_definitions.get(field) else {
            return Err(LoomError::invalid(format!(
                "unknown ticket field `{field}`; inspect tickets fields before deleting it"
            )));
        };
        if !definition.is_applicable(&project.project_id, ticket.ticket_type.type_id()) {
            return Err(LoomError::invalid(format!(
                "ticket field `{field}` is not applicable to this ticket type"
            )));
        }
    }
    Ok(())
}

fn is_core_ticket_field(field: &str) -> bool {
    ticket_core_field_specs()
        .iter()
        .any(|spec| spec.native_field == field)
}

fn validate_ticket_field_name(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid("ticket field key must not be empty"));
    }
    if value
        .chars()
        .any(|ch| !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'))
    {
        return Err(LoomError::invalid(
            "ticket field key must contain only a-z, 0-9, or underscore",
        ));
    }
    Ok(())
}

fn json_field_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Object(map) => map
            .get("String")
            .or_else(|| map.get("Text"))
            .or_else(|| map.get("EnumOption"))
            .or_else(|| map.get("Principal"))
            .and_then(Value::as_str)
            .map(str::to_string),
        Value::Array(items) => {
            let values = items.iter().filter_map(json_field_text).collect::<Vec<_>>();
            if values.is_empty() {
                None
            } else {
                Some(values.join(" "))
            }
        }
        _ => None,
    }
}

fn ticket_key_occurrences(text: &str) -> Vec<(usize, usize, String, String)> {
    extract_markdown_reference_candidates(text)
        .into_iter()
        .filter_map(|candidate| {
            if candidate.kind != MarkdownReferenceKind::Typed {
                return None;
            }
            let key = candidate.text.strip_prefix("!ticket:")?;
            crate::TicketKey::parse(key).ok().map(|key| {
                (
                    candidate.span_start,
                    candidate.span_end,
                    key.canonical(),
                    candidate.text,
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use loom_core::{AclEffect, AclGrant, AclScope, AclSubject, IdentityStore, PrincipalKind};
    use loom_store::{FileStore, MemoryBacking};

    #[test]
    fn ticket_list_cursor_round_trips_and_rejects_garbage() {
        for offset in [0usize, 1, 25, 100, 4096] {
            let cursor = encode_ticket_list_cursor(offset);
            assert_eq!(decode_ticket_list_cursor(&cursor).unwrap(), offset);
        }
        assert!(decode_ticket_list_cursor("not-a-cursor").is_err());
        assert!(decode_ticket_list_cursor("").is_err());
    }

    #[test]
    fn ticket_completed_status_gates_dependency_readiness() {
        for done in ["accepted", "closed", "done"] {
            assert!(ticket_status_is_completed(done), "{done}");
        }
        for open in [
            "backlog",
            "planned",
            "ready",
            "in_progress",
            "blocked",
            "waiting_for_review",
            "rejected",
        ] {
            assert!(!ticket_status_is_completed(open), "{open}");
        }
    }

    #[test]
    fn board_create_list_update_and_manual_card_move_are_profile_rooted() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let columns = vec![
            BoardColumn::with_display("todo", "To Do", BTreeSet::new(), None, false, 10).unwrap(),
            BoardColumn::with_display("doing", "Doing", BTreeSet::new(), Some(3), false, 20)
                .unwrap(),
        ];
        let created = create_board(
            &mut loom,
            namespace,
            BoardCreateRequest {
                workspace_id: &workspace_id,
                board_id: "board-1",
                board_key: "ENG",
                name: "Engineering",
                description: "Planning board",
                project_id: "matrix",
                scope: BoardScope::ManualSet,
                mode: BoardMode::Manual,
                columns: &columns,
                swimlanes: &[],
                card_display_fields: &["title".to_string(), "priority".to_string()],
                owner_principal: Some("agent:lead"),
                coordinator_principal: None,
                updated_by: "agent:lead",
                expected_root: None,
            },
        )
        .unwrap();
        assert_eq!(created.mode, "manual");
        assert_ne!(created.profile_root, "");

        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "ready", "title": "Build board"}),
                policy_labels: &[],
                expected_root: Some(&created.profile_root),
            },
        )
        .unwrap();
        let moved = move_board_card(
            &mut loom,
            namespace,
            BoardCardMoveRequest {
                workspace_id: &workspace_id,
                board_id: "board-1",
                ticket_id: &ticket.ticket_id,
                column_id: "doing",
                rank_token: "m",
                swimlane_id: None,
                updated_by: "agent:lead",
                expected_root: Some(&ticket.profile_root),
            },
        )
        .unwrap();
        assert_eq!(moved.cards.len(), 1);
        assert_eq!(moved.cards[0].column_id, "doing");

        let listed = list_boards(&loom, namespace, &workspace_id, false).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].cards[0].ticket_id, ticket.ticket_id);

        let archived = update_board(
            &mut loom,
            namespace,
            BoardUpdateRequest {
                workspace_id: &workspace_id,
                board_id: "board-1",
                board_key: None,
                name: Some("Engineering Archive"),
                description: None,
                scope: None,
                owner_principal: None,
                coordinator_principal: None,
                card_display_fields: None,
                board_status: Some(BoardStatus::Archived),
                updated_by: "agent:lead",
                expected_root: Some(&moved.profile_root),
            },
        )
        .unwrap();
        assert_eq!(archived.name, "Engineering Archive");
        assert_eq!(archived.board_status, "archived");
    }

    #[test]
    fn status_mapped_board_card_move_requires_ticket_status_match() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let columns = vec![
            BoardColumn::with_display(
                "done",
                "Done",
                BTreeSet::from(["accepted".to_string()]),
                None,
                false,
                10,
            )
            .unwrap(),
        ];
        let created = create_board(
            &mut loom,
            namespace,
            BoardCreateRequest {
                workspace_id: &workspace_id,
                board_id: "status-board",
                board_key: "STATUS",
                name: "Status",
                description: "",
                project_id: "matrix",
                scope: BoardScope::project("matrix"),
                mode: BoardMode::StatusMapped,
                columns: &columns,
                swimlanes: &[],
                card_display_fields: &[],
                owner_principal: None,
                coordinator_principal: None,
                updated_by: "agent:lead",
                expected_root: None,
            },
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "ready"}),
                policy_labels: &[],
                expected_root: Some(&created.profile_root),
            },
        )
        .unwrap();
        let err = move_board_card(
            &mut loom,
            namespace,
            BoardCardMoveRequest {
                workspace_id: &workspace_id,
                board_id: "status-board",
                ticket_id: &ticket.ticket_id,
                column_id: "done",
                rank_token: "a",
                swimlane_id: None,
                updated_by: "agent:lead",
                expected_root: Some(&ticket.profile_root),
            },
        )
        .unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
    }

    fn ticket_with_assignee(assignee: &str) -> Ticket {
        let mut fields = BTreeMap::new();
        fields.insert(
            "assignee".to_string(),
            TicketFieldValue::Principal(assignee.to_string()),
        );
        fields.insert(
            "status".to_string(),
            TicketFieldValue::EnumOption("ready".to_string()),
        );
        Ticket::new(TicketInput {
            ticket_id: "11111111-1111-4111-8111-111111111111",
            project_id: "matrix",
            ticket_number: 1,
            ticket_type: TicketType::Task,
            external_identity: None,
            fields,
            policy_labels: &[],
        })
        .unwrap()
    }

    fn ticket_without_assignee() -> Ticket {
        let fields = BTreeMap::from([(
            "status".to_string(),
            TicketFieldValue::EnumOption("planned".to_string()),
        )]);
        Ticket::new(TicketInput {
            ticket_id: "11111111-1111-4111-8111-111111111111",
            project_id: "matrix",
            ticket_number: 1,
            ticket_type: TicketType::Task,
            external_identity: None,
            fields,
            policy_labels: &[],
        })
        .unwrap()
    }

    fn define_optional_string_task_fields(
        loom: &mut Loom<FileStore>,
        namespace: WorkspaceId,
        workspace_id: &str,
        project_id: &str,
        field_ids: &[&str],
    ) {
        let task_types = vec!["task".to_string()];
        for field_id in field_ids {
            put_ticket_field_definition(
                loom,
                namespace,
                TicketFieldDefinitionWriteRequest {
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
                    cardinality: TicketFieldCardinality::Optional,
                    applicable_type_ids: &task_types,
                    expected_root: None,
                },
            )
            .unwrap();
        }
    }

    #[test]
    fn write_access_policy_allows_any_ticket_writer_lifecycle_action() {
        let project = TicketProject::new("matrix", "MX", "Matrix").unwrap();
        let ticket = ticket_with_assignee("agent:3");
        authorize_lifecycle_action(
            &project,
            &ticket,
            "agent:4",
            TicketLifecycleAction::RequestReview,
        )
        .unwrap();
    }

    #[test]
    fn default_project_allows_workspace_writer_to_transition_valid_edges() {
        let (mut loom, namespace, _admin, writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let writer_session = loom
            .identity_store_mut()
            .unwrap()
            .authenticate_passphrase(writer, "writer", "writer-session")
            .unwrap();
        loom.set_session(writer_session.id);
        let transitioned = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("in_progress"),
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
        .unwrap();
        assert_eq!(transitioned.fields["status"], "in_progress");
    }

    #[test]
    fn resolve_principal_display_maps_canonical_id_to_handle_and_falls_back() {
        // the single shared resolver reused by every user-facing surface (tickets, lanes,
        // CLI, MCP). A lane owner (or ticket assignee) canonical id resolves to its handle; an
        // unregistered id falls back to the id string.
        let admin = pid(2);
        let owner = pid(7);
        let mut identity = IdentityStore::new(admin);
        identity
            .add_principal_with_handle(owner, "lane-owner", "Lane Owner", PrincipalKind::User)
            .unwrap();
        let canonical = owner.to_string();
        assert_eq!(
            resolve_principal_display(Some(&identity), &canonical),
            "lane-owner".to_string()
        );
        assert_eq!(
            resolve_principal_display(Some(&identity), "unregistered-id"),
            "unregistered-id".to_string()
        );
        assert_eq!(resolve_principal_display(None, &canonical), canonical);
    }

    #[test]
    fn assignee_alias_persists_canonical_id_and_exposes_display() {
        // assigning by a registered handle persists the canonical principal id (source of
        // truth) while read projections expose the resolved display alias additively.
        let (mut loom, namespace, _admin, writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let transitioned = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("in_progress"),
                observed_source_status: None,
                observed_workflow_version: None,
                assignee: Some("writer"),
                expected_root: Some(&ticket.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        let canonical = writer.to_string();
        // The persisted assignee is the canonical id, never the alias.
        assert_eq!(
            transitioned.fields["assignee"],
            serde_json::json!(canonical)
        );
        assert_eq!(
            transitioned.fields["assignee_display"],
            serde_json::json!("writer")
        );

        // Read-back projection: canonical id plus resolved display alias.
        let fetched = get_ticket(&loom, namespace, &workspace_id, &ticket.ticket_id)
            .unwrap()
            .unwrap();
        assert_eq!(fetched.fields["assignee"], serde_json::json!(canonical));
        assert_eq!(
            fetched.fields["assignee_display"],
            serde_json::json!("writer")
        );

        // TicketCoreFields exposes canonical assignee and resolved display.
        let profile = TicketProfileReader::open(&loom, namespace, &workspace_id)
            .unwrap()
            .unwrap();
        let stored = profile.ticket(&ticket.ticket_id).unwrap().unwrap();
        let core =
            crate::TicketCoreFields::from_ticket_with_identity(&stored, loom.identity_store());
        assert_eq!(core.assignee.as_deref(), Some(canonical.as_str()));
        assert_eq!(core.assignee_display.as_deref(), Some("writer"));

        // Unknown/unregistered id falls back to the id string on display.
        assert_eq!(
            resolve_principal_display(loom.identity_store(), "not-a-registered-id"),
            "not-a-registered-id".to_string()
        );
    }

    #[test]
    fn ticket_identity_key_alias_and_external_identity_stay_separate() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: Some("jira-cloud"),
                external_id: Some("10042"),
                fields: &serde_json::json!({"status": "planned", "title": "Preserve identity"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        assert_eq!(ticket.primary_key, "MX-1");
        assert_eq!(ticket.external_source.as_deref(), Some("jira-cloud"));
        assert_eq!(ticket.external_id.as_deref(), Some("10042"));

        let by_id = get_ticket(&loom, namespace, &workspace_id, &ticket.ticket_id)
            .unwrap()
            .unwrap();
        let by_key = get_ticket(&loom, namespace, &workspace_id, "mx-1")
            .unwrap()
            .unwrap();
        assert_eq!(by_id.ticket_id, ticket.ticket_id);
        assert_eq!(by_key.ticket_id, ticket.ticket_id);

        let profile = TicketProfileReader::open(&loom, namespace, &workspace_id)
            .unwrap()
            .unwrap();
        let imported = profile
            .ticket_by_external_identity(
                &ExternalTicketIdentity::new("jira-cloud", "10042").unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(imported.ticket_id, ticket.ticket_id);
        drop(profile);

        let rekeyed = rekey_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "CORE",
            Some(&ticket.profile_root),
        )
        .unwrap();
        let by_retired_key = get_ticket(&loom, namespace, &workspace_id, "MX-1")
            .unwrap()
            .unwrap();
        let by_active_key = get_ticket(&loom, namespace, &workspace_id, "CORE-1")
            .unwrap()
            .unwrap();
        assert_eq!(by_retired_key.ticket_id, ticket.ticket_id);
        assert_eq!(by_active_key.ticket_id, ticket.ticket_id);
        assert_eq!(by_active_key.primary_key, "CORE-1");

        let duplicate = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "bug",
                external_source: Some("jira-cloud"),
                external_id: Some("10042"),
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: Some(&rekeyed.profile_root),
            },
        )
        .unwrap_err();
        assert_eq!(duplicate.code, Code::AlreadyExists);
    }

    #[test]
    fn ticket_read_projection_is_explicit_and_tagged() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"title": "Projection parity"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let default_read = get_ticket(&loom, namespace, &workspace_id, &ticket.ticket_id)
            .unwrap()
            .unwrap();
        assert_eq!(default_read.projection_profile, "native");
        assert_eq!(default_read.projection_kind, "ticket.native");
        assert_eq!(
            default_read.projection_selection_source,
            "machine_default_native"
        );
        assert!(default_read.fields.contains_key("title"));

        let projected_read = get_ticket_with_projection(
            &loom,
            namespace,
            &workspace_id,
            &ticket.ticket_id,
            parse_ticket_projection(Some("jira")).unwrap(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(projected_read.projection_profile, "jira");
        assert_eq!(projected_read.projection_kind, "ticket.projected.jira");
        assert_eq!(
            projected_read.projection_selection_source,
            "explicit_request"
        );
        assert!(projected_read.fields.contains_key("fields.summary"));
        assert!(!projected_read.fields.contains_key("title"));
        let public_json = serde_json::to_value(&projected_read).unwrap();
        assert_eq!(public_json["projection"], "jira");
        assert_eq!(public_json["key"], "MX-1");
        assert_eq!(
            public_json.pointer("/fields/summary"),
            Some(&serde_json::json!("Projection parity"))
        );
        assert!(public_json.get("projection_profile").is_none());
        assert!(public_json.get("projection_kind").is_none());
        assert!(public_json.get("projection_source").is_none());
        assert!(public_json.get("projection_selection_source").is_none());
    }

    #[test]
    fn projected_ticket_input_fields_normalize_to_native_fields() {
        let jira = normalize_ticket_fields_for_projection(
            &serde_json::json!({
                "fields": {
                    "summary": "Jira title",
                    "description": "Jira body"
                },
                "transition": {
                    "to": { "name": "accepted" }
                },
                "update": {
                    "priority": [{"set": "High"}]
                }
            }),
            parse_ticket_projection(Some("jira")).unwrap(),
        )
        .unwrap();
        assert_eq!(jira["title"], "Jira title");
        assert_eq!(jira["description"], "Jira body");
        assert_eq!(jira["priority"], "High");
        assert_eq!(jira["status"], "accepted");

        let asana = normalize_ticket_fields_for_projection(
            &serde_json::json!({
                "data": {
                    "name": "Asana title",
                    "notes": "Asana body",
                    "completed": true
                }
            }),
            parse_ticket_projection(Some("asana")).unwrap(),
        )
        .unwrap();
        assert_eq!(asana["title"], "Asana title");
        assert_eq!(asana["description"], "Asana body");
        assert_eq!(asana["status"], "accepted");

        let notion = normalize_ticket_fields_for_projection(
            &serde_json::json!({
                "properties": {
                    "Name": { "title": [{ "plain_text": "Notion title" }] },
                    "Description": { "rich_text": [{ "text": { "content": "Notion body" } }] },
                    "Status": { "status": { "name": "waiting_for_review" } }
                }
            }),
            parse_ticket_projection(Some("notion")).unwrap(),
        )
        .unwrap();
        assert_eq!(notion["title"], "Notion title");
        assert_eq!(notion["description"], "Notion body");
        assert_eq!(notion["status"], "waiting_for_review");

        let redmine = normalize_ticket_fields_for_projection(
            &serde_json::json!({
                "issue": {
                    "subject": "Redmine title",
                    "description": "Redmine body",
                    "status_id": "accepted"
                }
            }),
            parse_ticket_projection(Some("redmine")).unwrap(),
        )
        .unwrap();
        assert_eq!(redmine["title"], "Redmine title");
        assert_eq!(redmine["description"], "Redmine body");
        assert_eq!(redmine["status"], "accepted");

        assert_eq!(
            normalize_ticket_delete_fields_for_projection(
                &["fields.summary".to_string(), "subject".to_string()],
                parse_ticket_projection(Some("jira")).unwrap()
            ),
            vec!["title".to_string(), "subject".to_string()]
        );
    }

    #[test]
    fn ticket_field_catalog_exposes_projection_write_paths() {
        let jira = ticket_field_catalog(
            parse_ticket_projection(Some("jira")).unwrap(),
            Some("create"),
        )
        .unwrap();
        let title = jira
            .fields
            .iter()
            .find(|field| field.native_field == "title")
            .unwrap();
        assert_eq!(jira.projection_profile, "jira");
        assert_eq!(title.write_path, "fields.summary");
        assert!(title.required_on_create);
        let status = jira
            .fields
            .iter()
            .find(|field| field.native_field == "status")
            .unwrap();
        assert_eq!(status.field_type, "enum");
        assert!(
            status
                .enum_values
                .iter()
                .any(|value| value == "waiting_for_review")
        );

        let asana = ticket_field_catalog(
            parse_ticket_projection(Some("asana")).unwrap(),
            Some("update"),
        )
        .unwrap();
        let description = asana
            .fields
            .iter()
            .find(|field| field.native_field == "description")
            .unwrap();
        assert_eq!(description.write_path, "data.notes");
        assert!(!description.required_on_create);
    }

    #[test]
    fn ticket_field_definition_write_updates_project_catalog() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let catalog = put_ticket_field_definition(
            &mut loom,
            namespace,
            TicketFieldDefinitionWriteRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                field_id: "severity",
                key: "severity",
                name: "Severity",
                description: Some("Incident severity."),
                field_type: "enum",
                option_set: Some("severity"),
                max_length: Some(64),
                required: false,
                searchable: true,
                orderable: true,
                cardinality: TicketFieldCardinality::Optional,
                applicable_type_ids: &["bug".to_string()],
                expected_root: None,
            },
        )
        .unwrap();
        let severity = catalog
            .fields
            .iter()
            .find(|field| field.native_field == "severity")
            .unwrap();
        assert_eq!(severity.field_type, "enum:severity");
        assert_eq!(severity.max_length, Some(64));

        let read_catalog =
            ticket_field_catalog_for_project(&loom, namespace, &workspace_id, "matrix", None, None)
                .unwrap();
        assert!(
            read_catalog
                .fields
                .iter()
                .any(|field| field.native_field == "severity")
        );

        let retired = retire_ticket_field_definition(
            &mut loom,
            namespace,
            TicketFieldDefinitionRetireRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                field_id: "severity",
                expected_root: None,
            },
        )
        .unwrap();
        assert!(
            retired
                .fields
                .iter()
                .all(|field| field.native_field != "severity")
        );
    }

    #[test]
    fn ticket_update_rejects_unknown_custom_field_without_definition() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let create_rejected = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"title": "Before", "unknown_field": "value"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap_err();
        assert_eq!(create_rejected.code, Code::InvalidArgument);

        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"title": "Before"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let update_rejected = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: Some(&serde_json::json!({"unknown_field": "value"})),
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
        assert_eq!(update_rejected.code, Code::InvalidArgument);

        define_optional_string_task_fields(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            &["unknown_field"],
        );
        let updated = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: Some(&serde_json::json!({"unknown_field": "value"})),
                delete_fields: &[],
                action: None,
                target_status: None,
                observed_source_status: None,
                observed_workflow_version: None,
                assignee: None,
                expected_root: None,
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        assert_eq!(updated.fields["unknown_field"], "value");
    }

    #[test]
    fn ticket_delete_is_audited_tombstone_with_expected_root() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: Some("jira"),
                external_id: Some("10001"),
                fields: &serde_json::json!({"title": "Before"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let updated = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: Some(&serde_json::json!({"title": "After"})),
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
        .unwrap();
        let stale = delete_ticket(
            &mut loom,
            namespace,
            TicketDeleteRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.primary_key,
                expected_root: Some(&ticket.profile_root),
            },
        )
        .unwrap_err();
        assert_eq!(stale.code, Code::Conflict);

        let deleted = delete_ticket(
            &mut loom,
            namespace,
            TicketDeleteRequest {
                workspace_id: &workspace_id,
                ticket_id: &updated.primary_key,
                expected_root: Some(&updated.profile_root),
            },
        )
        .unwrap();
        assert_eq!(deleted.fields["status"], "closed");
        assert_eq!(deleted.fields["status_category"], "done");
        assert_eq!(deleted.fields["resolution"], "deleted");
        assert!(deleted.fields.contains_key("deleted_at"));
        assert!(deleted.fields.contains_key("deleted_by"));
        assert_eq!(deleted.external_source.as_deref(), Some("jira"));
        assert_eq!(deleted.external_id.as_deref(), Some("10001"));
        let history = history(&loom, namespace, &workspace_id, Some(&ticket.ticket_id)).unwrap();
        assert!(
            history
                .iter()
                .any(|record| record.operation_kind == "ticket.deleted")
        );

        let duplicate = delete_ticket(
            &mut loom,
            namespace,
            TicketDeleteRequest {
                workspace_id: &workspace_id,
                ticket_id: &deleted.primary_key,
                expected_root: Some(&deleted.profile_root),
            },
        )
        .unwrap_err();
        assert_eq!(duplicate.code, Code::Conflict);
    }

    #[test]
    fn ticket_summary_applies_explicit_projection_aliases() {
        let ticket = Ticket::new(TicketInput {
            ticket_id: "11111111-1111-4111-8111-111111111111",
            project_id: "matrix",
            ticket_number: 1,
            ticket_type: TicketType::Task,
            external_identity: None,
            fields: BTreeMap::from([(
                "title".to_string(),
                TicketFieldValue::String("Projection parity".to_string()),
            )]),
            policy_labels: &[],
        })
        .unwrap();
        let selection = crate::TicketProjectionProjectConfig::new(
            TicketProjectionProfile::Jira,
            BTreeSet::from([
                TicketProjectionProfile::Native,
                TicketProjectionProfile::Jira,
            ]),
            BTreeMap::from([(
                TicketProjectionProfile::Jira,
                crate::TicketProjectionProfileConfig::new(
                    TicketProjectionProfile::Jira,
                    BTreeMap::from([("title".to_string(), "summary".to_string())]),
                    BTreeMap::new(),
                )
                .unwrap(),
            )]),
        )
        .unwrap()
        .select(
            TicketProjectionRequestContext::MachineApi,
            Some(TicketProjectionProfile::Jira),
        )
        .unwrap();
        let profile_root = Digest::parse(
            "blake3:0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();

        let summary = ticket_summary(
            "workspace",
            &ticket,
            "MX-1".to_string(),
            None,
            None,
            profile_root,
            Some(&selection),
        );

        assert_eq!(summary.projection_profile, "jira");
        assert_eq!(summary.projection_kind, "ticket.projected.jira");
        assert_eq!(summary.projection_source, "canonical_ticket");
        assert_eq!(summary.projection_selection_source, "explicit_request");
        assert!(summary.fields.contains_key("summary"));
        assert!(!summary.fields.contains_key("title"));
    }

    #[test]
    fn ownership_governed_policy_checks_assignee_and_acceptance_authority() {
        let mut project = TicketProject::new("matrix", "MX", "Matrix").unwrap();
        project.lifecycle_authorization_policy =
            TicketLifecycleAuthorizationPolicy::OwnershipGoverned;
        project.project_owner_principal = Some("owner:matrix".to_string());
        project.acceptance_authorities = BTreeSet::from(["acceptor:matrix".to_string()]);
        let ticket = ticket_with_assignee("agent:3");

        authorize_lifecycle_action(
            &project,
            &ticket,
            "agent:3",
            TicketLifecycleAction::RequestReview,
        )
        .unwrap();
        let rejected = authorize_lifecycle_action(
            &project,
            &ticket,
            "agent:4",
            TicketLifecycleAction::RequestReview,
        )
        .unwrap_err();
        assert_eq!(rejected.code, Code::PermissionDenied);
        authorize_lifecycle_action(
            &project,
            &ticket,
            "owner:matrix",
            TicketLifecycleAction::Accept,
        )
        .unwrap();
        authorize_lifecycle_action(
            &project,
            &ticket,
            "acceptor:matrix",
            TicketLifecycleAction::Reject,
        )
        .unwrap();
        let rejected =
            authorize_lifecycle_action(&project, &ticket, "agent:3", TicketLifecycleAction::Accept)
                .unwrap_err();
        assert_eq!(rejected.code, Code::PermissionDenied);
    }

    #[test]
    fn split_lifecycle_actor_policies_are_independent_from_workflow_edges() {
        let mut project = TicketProject::new("matrix", "MX", "Matrix").unwrap();
        let ticket = ticket_with_assignee("agent:3");

        project.lifecycle_authorization_policy = TicketLifecycleAuthorizationPolicy::Assignee;
        authorize_lifecycle_action(
            &project,
            &ticket,
            "agent:3",
            TicketLifecycleAction::RequestReview,
        )
        .unwrap();
        let denied = authorize_lifecycle_action(
            &project,
            &ticket,
            "agent:4",
            TicketLifecycleAction::RequestReview,
        )
        .unwrap_err();
        assert_eq!(denied.code, Code::PermissionDenied);

        project.lifecycle_authorization_policy =
            TicketLifecycleAuthorizationPolicy::ReviewAuthority;
        project.project_owner_principal = Some("owner:matrix".to_string());
        authorize_lifecycle_action(
            &project,
            &ticket,
            "agent:4",
            TicketLifecycleAction::RequestReview,
        )
        .unwrap();
        let denied =
            authorize_lifecycle_action(&project, &ticket, "agent:4", TicketLifecycleAction::Accept)
                .unwrap_err();
        assert_eq!(denied.code, Code::PermissionDenied);
        authorize_lifecycle_action(
            &project,
            &ticket,
            "owner:matrix",
            TicketLifecycleAction::Accept,
        )
        .unwrap();
    }

    #[test]
    fn ownership_governed_policy_allows_unassigned_claim_once() {
        let mut project = TicketProject::new("matrix", "MX", "Matrix").unwrap();
        project.lifecycle_authorization_policy =
            TicketLifecycleAuthorizationPolicy::OwnershipGoverned;
        let unassigned = ticket_without_assignee();
        authorize_lifecycle_action(
            &project,
            &unassigned,
            "agent:3",
            TicketLifecycleAction::Claim,
        )
        .unwrap();

        let assigned = ticket_with_assignee("agent:3");
        let rejected = authorize_lifecycle_action(
            &project,
            &assigned,
            "agent:4",
            TicketLifecycleAction::Claim,
        )
        .unwrap_err();
        assert_eq!(rejected.code, Code::PermissionDenied);
    }

    #[test]
    fn lifecycle_transition_graph_rejects_direct_terminal_moves() {
        let ticket = ticket_with_assignee("agent:3");
        assert!(enforce_lifecycle_transition(&ticket, TicketLifecycleAction::Accept).is_err());
        assert!(enforce_lifecycle_transition(&ticket, TicketLifecycleAction::Complete).is_err());
        enforce_lifecycle_transition(&ticket, TicketLifecycleAction::Claim).unwrap();
    }

    #[test]
    fn lifecycle_fields_preserve_assignment_and_status_semantics() {
        let mut ticket = ticket_with_assignee("agent:3");
        apply_lifecycle_fields(
            &mut ticket,
            "agent:3",
            TicketLifecycleAction::RequestReview,
            None,
        )
        .unwrap();
        assert_eq!(
            ticket.fields.get("status"),
            Some(&TicketFieldValue::EnumOption(
                "waiting_for_review".to_string()
            ))
        );
        apply_lifecycle_fields(&mut ticket, "agent:3", TicketLifecycleAction::Release, None)
            .unwrap();
        assert_eq!(ticket.fields.get("assignee"), Some(&TicketFieldValue::Null));
        assert_eq!(
            ticket.fields.get("status"),
            Some(&TicketFieldValue::EnumOption("ready".to_string()))
        );
    }

    fn pid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    fn authenticated_ticket_loom() -> (Loom<FileStore>, WorkspaceId, WorkspaceId, WorkspaceId) {
        let namespace = pid(1);
        let admin = pid(2);
        let writer = pid(3);
        let mut loom =
            Loom::new(FileStore::with_backing(Box::new(MemoryBacking::new()), true).unwrap());
        let mut identity = IdentityStore::new(admin);
        identity
            .set_passphrase(admin, "admin", b"12345678")
            .unwrap();
        identity
            .add_principal(writer, "writer", PrincipalKind::User)
            .unwrap();
        identity
            .set_passphrase(writer, "writer", b"12345678")
            .unwrap();
        let admin_session = identity
            .authenticate_passphrase(admin, "admin", "admin-session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(admin_session.id);
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(admin),
                Some(namespace),
                None,
                [AclRight::Admin],
            )
            .unwrap();
        loom.acl_store_mut()
            .grant(AclGrant {
                subject: AclSubject::Principal(admin),
                workspace: Some(namespace),
                domain: Some(AclDomain::Tickets),
                ref_glob: None,
                scopes: vec![AclScope::All],
                rights: [AclRight::Read, AclRight::Write, AclRight::Admin]
                    .into_iter()
                    .collect(),
                effect: AclEffect::Allow,
                predicate: None,
            })
            .unwrap();
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(admin),
                Some(namespace),
                Some(FacetKind::Vcs),
                [AclRight::Admin],
            )
            .unwrap();
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(admin),
                Some(namespace),
                Some(FacetKind::Graph),
                [AclRight::Admin],
            )
            .unwrap();
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(admin),
                Some(namespace),
                Some(FacetKind::Queue),
                [AclRight::Read, AclRight::Write, AclRight::Advance],
            )
            .unwrap();
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(writer),
                Some(namespace),
                None,
                [AclRight::Read, AclRight::Write],
            )
            .unwrap();
        loom.acl_store_mut()
            .grant(AclGrant {
                subject: AclSubject::Principal(writer),
                workspace: Some(namespace),
                domain: Some(AclDomain::Tickets),
                ref_glob: None,
                scopes: vec![AclScope::All],
                rights: [AclRight::Read, AclRight::Write].into_iter().collect(),
                effect: AclEffect::Allow,
                predicate: None,
            })
            .unwrap();
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(writer),
                Some(namespace),
                Some(FacetKind::Vcs),
                [AclRight::Read, AclRight::Write],
            )
            .unwrap();
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(writer),
                Some(namespace),
                Some(FacetKind::Graph),
                [AclRight::Read, AclRight::Write],
            )
            .unwrap();
        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(writer),
                Some(namespace),
                Some(FacetKind::Queue),
                [AclRight::Read, AclRight::Write, AclRight::Advance],
            )
            .unwrap();
        (loom, namespace, admin, writer)
    }

    #[test]
    fn lifecycle_policy_configuration_requires_admin_authorization() {
        let (mut loom, namespace, _admin, writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let writer_session = loom
            .identity_store_mut()
            .unwrap()
            .authenticate_passphrase(writer, "writer", "writer-session")
            .unwrap();
        loom.set_session(writer_session.id);
        let denied = set_project_lifecycle_policy(
            &mut loom,
            namespace,
            TicketProjectLifecyclePolicyRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                policy: TicketLifecycleAuthorizationPolicy::OwnershipGoverned,
                project_owner_principal: Some("owner:matrix"),
                acceptance_authorities: &["owner:matrix".to_string()],
                expected_root: None,
            },
        )
        .unwrap_err();
        assert_eq!(denied.code, Code::PermissionDenied);
    }

    #[test]
    fn acceptance_evidence_policy_defaults_to_disabled() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned", "title": "Ready"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let accepted = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("accepted"),
                observed_source_status: Some("planned"),
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&ticket.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        assert_eq!(accepted.fields["status"], "accepted");
    }

    #[test]
    fn acceptance_evidence_policy_validates_required_keys_on_accept() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        let project = create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let settings = set_project_settings(
            &mut loom,
            namespace,
            TicketProjectSettingsRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                default_projection: None,
                enable_projections: &[],
                disable_projections: &[],
                actor_enforcement: None,
                project_owner_principal: None,
                clear_project_owner_principal: false,
                acceptance_authorities: None,
                acceptance_evidence_enforcement: Some(true),
                required_acceptance_evidence_keys: Some(&[
                    TicketAcceptanceEvidenceKey::SourceAnchors,
                    TicketAcceptanceEvidenceKey::ChecksRun,
                ]),
                owner_contract_summary: None,
                owner_contract_details: None,
                worker_contract_summary: None,
                worker_contract_details: None,
                expected_root: Some(&project.profile_root),
            },
        )
        .unwrap();
        assert!(settings.acceptance_evidence_enforcement);
        assert_eq!(
            settings.required_acceptance_evidence_keys,
            vec!["source_anchors".to_string(), "checks_run".to_string()]
        );
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned", "title": "Ready"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let missing = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("accepted"),
                observed_source_status: Some("planned"),
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
        assert_eq!(missing.code, Code::InvalidArgument);
        let evidence = TicketCommentEvidence::from_json(&serde_json::json!({
            "source_anchors": ["crates/loom-tickets/src/service.rs:1"],
            "checks_run": ["cargo test -p uldren-loom-tickets acceptance_evidence_policy_validates_required_keys_on_accept --offline"]
        }))
        .unwrap();
        let accepted = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("accepted"),
                observed_source_status: Some("planned"),
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&ticket.profile_root),
                comment: Some(TicketUpdateCommentRequest {
                    comment_id: Some("acceptance-evidence"),
                    comment_type: Some("acceptance_evidence"),
                    body: "Acceptance evidence attached.",
                    evidence: Some(evidence),
                }),
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        assert_eq!(accepted.fields["status"], "accepted");
    }

    #[test]
    fn ownership_governed_policy_requires_owner_or_acceptance_authority() {
        let namespace = pid(9);
        let mut loom =
            Loom::new(FileStore::with_backing(Box::new(MemoryBacking::new()), true).unwrap());
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let invalid = set_project_lifecycle_policy(
            &mut loom,
            namespace,
            TicketProjectLifecyclePolicyRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                policy: TicketLifecycleAuthorizationPolicy::OwnershipGoverned,
                project_owner_principal: None,
                acceptance_authorities: &[],
                expected_root: None,
            },
        )
        .unwrap_err();
        assert_eq!(invalid.code, Code::InvalidArgument);
    }

    #[test]
    fn project_settings_updates_projection_and_lifecycle_policy_together() {
        let (mut loom, namespace, admin, writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let admin_principal = admin.to_string();
        let writer_principal = writer.to_string();
        let updated = set_project_settings(
            &mut loom,
            namespace,
            TicketProjectSettingsRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                default_projection: Some(TicketProjectionProfile::Jira),
                enable_projections: &[TicketProjectionProfile::Jira],
                disable_projections: &[TicketProjectionProfile::Asana],
                actor_enforcement: Some(TicketLifecycleAuthorizationPolicy::ReviewAuthority),
                project_owner_principal: Some(&admin_principal),
                clear_project_owner_principal: false,
                acceptance_authorities: Some(std::slice::from_ref(&writer_principal)),
                acceptance_evidence_enforcement: None,
                required_acceptance_evidence_keys: None,
                owner_contract_summary: None,
                owner_contract_details: None,
                worker_contract_summary: None,
                worker_contract_details: None,
                expected_root: None,
            },
        )
        .unwrap();
        assert_eq!(updated.default_projection, "jira");
        assert!(updated.enabled_projections.contains(&"jira".to_string()));
        assert!(!updated.enabled_projections.contains(&"asana".to_string()));
        assert_eq!(updated.lifecycle_authorization_policy, "review_authority");
        assert_eq!(
            updated.project_owner_principal.as_deref(),
            Some(admin_principal.as_str())
        );
        assert_eq!(updated.acceptance_authorities, vec![writer_principal]);

        let stored = get_project(&loom, namespace, &workspace_id, "matrix")
            .unwrap()
            .unwrap();
        assert_eq!(stored.default_projection, "jira");
        assert!(!stored.enabled_projections.contains(&"asana".to_string()));
    }

    #[test]
    fn project_contracts_default_and_round_trip_through_settings() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        let created = create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        assert_eq!(created.contracts.note, crate::TICKET_CONTRACTS_NOTE);
        assert_eq!(
            created.contracts.owner.summary,
            "Owner verifies completed work before acceptance."
        );
        assert_eq!(created.contracts.owner.details, None);
        let created_with_details =
            get_project_with_contract_details(&loom, namespace, &workspace_id, "matrix", true)
                .unwrap()
                .unwrap();
        assert_eq!(
            created_with_details.contracts.note,
            crate::TICKET_CONTRACTS_NOTE
        );
        assert_eq!(
            created_with_details.contracts.owner.details.as_deref(),
            Some(crate::TICKET_DEFAULT_OWNER_CONTRACT)
        );
        assert_eq!(
            created_with_details.contracts.worker.details.as_deref(),
            Some(crate::TICKET_DEFAULT_WORKER_CONTRACT)
        );
        let owner_default = created_with_details
            .contracts
            .owner
            .details
            .as_deref()
            .unwrap();
        assert!(owner_default.contains("## Go Contract"));
        assert!(owner_default.contains("Accepting a ticket is a correctness and code-review pass"));
        assert!(owner_default.contains("Feedback must name the issue"));
        assert!(owner_default.contains("which lanes can be told go"));
        let worker_default = created_with_details
            .contracts
            .worker
            .details
            .as_deref()
            .unwrap();
        assert!(worker_default.contains("## Ticket Source Of Truth"));
        assert!(worker_default.contains("resolve that feedback first"));
        assert!(worker_default.contains("Record progress, blockers, questions, source anchors"));
        assert!(worker_default.contains("the next ticket the lane expects to work"));

        let owner_summary = "Owner answers recorded project decisions.";
        let owner_details = "# Owner Contract\n\nAnswer recorded project decisions.";
        let worker_summary = "Worker records durable ticket state.";
        let worker_details = "# Worker Contract\n\nRecord durable workflow state on tickets.";
        let updated = set_project_settings(
            &mut loom,
            namespace,
            TicketProjectSettingsRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                default_projection: None,
                enable_projections: &[],
                disable_projections: &[],
                actor_enforcement: None,
                project_owner_principal: None,
                clear_project_owner_principal: false,
                acceptance_authorities: None,
                acceptance_evidence_enforcement: None,
                required_acceptance_evidence_keys: None,
                owner_contract_summary: Some(owner_summary),
                owner_contract_details: Some(owner_details),
                worker_contract_summary: Some(worker_summary),
                worker_contract_details: Some(worker_details),
                expected_root: Some(&created.profile_root),
            },
        )
        .unwrap();
        assert_eq!(updated.contracts.note, crate::TICKET_CONTRACTS_NOTE);
        assert_eq!(updated.contracts.owner.summary, owner_summary);
        assert_eq!(updated.contracts.owner.details, None);
        assert_eq!(updated.contracts.worker.summary, worker_summary);
        assert_eq!(updated.contracts.worker.details, None);

        let stored = get_project(&loom, namespace, &workspace_id, "matrix")
            .unwrap()
            .unwrap();
        assert_eq!(stored.contracts.note, crate::TICKET_CONTRACTS_NOTE);
        assert_eq!(stored.contracts.owner.summary, owner_summary);
        assert_eq!(stored.contracts.owner.details, None);
        assert_eq!(stored.contracts.worker.summary, worker_summary);
        assert_eq!(stored.contracts.worker.details, None);
        let stored_with_details =
            get_project_with_contract_details(&loom, namespace, &workspace_id, "matrix", true)
                .unwrap()
                .unwrap();
        assert_eq!(
            stored_with_details.contracts.owner.details.as_deref(),
            Some(owner_details)
        );
        assert_eq!(
            stored_with_details.contracts.worker.details.as_deref(),
            Some(worker_details)
        );
    }

    #[test]
    fn lifecycle_policy_configuration_validates_principal_ids() {
        let (mut loom, namespace, admin, writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();

        let malformed = set_project_lifecycle_policy(
            &mut loom,
            namespace,
            TicketProjectLifecyclePolicyRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                policy: TicketLifecycleAuthorizationPolicy::OwnershipGoverned,
                project_owner_principal: Some("owner:matrix"),
                acceptance_authorities: &[],
                expected_root: None,
            },
        )
        .unwrap_err();
        assert_eq!(malformed.code, Code::InvalidArgument);

        let unknown = pid(200).to_string();
        let unknown = set_project_lifecycle_policy(
            &mut loom,
            namespace,
            TicketProjectLifecyclePolicyRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                policy: TicketLifecycleAuthorizationPolicy::OwnershipGoverned,
                project_owner_principal: Some(&unknown),
                acceptance_authorities: &[],
                expected_root: None,
            },
        )
        .unwrap_err();
        assert_eq!(unknown.code, Code::InvalidArgument);

        set_project_lifecycle_policy(
            &mut loom,
            namespace,
            TicketProjectLifecyclePolicyRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                policy: TicketLifecycleAuthorizationPolicy::OwnershipGoverned,
                project_owner_principal: Some(&admin.to_string()),
                acceptance_authorities: &[writer.to_string()],
                expected_root: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn project_create_records_authenticated_owner() {
        let (mut loom, namespace, admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let profile = TicketProfileReader::open(&loom, namespace, &workspace_id)
            .unwrap()
            .unwrap();
        let project = profile.project("matrix").unwrap().unwrap();
        let admin = admin.to_string();
        assert_eq!(
            project.project_owner_principal.as_deref(),
            Some(admin.as_str())
        );
    }

    #[test]
    fn lifecycle_mutation_rejects_stale_expected_root() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let stale_root = ticket.profile_root;
        apply_ticket_lifecycle(
            &mut loom,
            namespace,
            TicketLifecycleRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                action: TicketLifecycleAction::Claim,
                target_status: None,
                assignee: None,
                expected_root: Some(&stale_root),
            },
        )
        .unwrap();
        let conflict = apply_ticket_lifecycle(
            &mut loom,
            namespace,
            TicketLifecycleRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                action: TicketLifecycleAction::RequestReview,
                target_status: None,
                assignee: None,
                expected_root: Some(&stale_root),
            },
        )
        .unwrap_err();
        assert_eq!(conflict.code, Code::Conflict);
    }

    #[test]
    fn ticket_relation_mutation_projects_and_removes_graph_edge() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        put_ticket_field_definition(
            &mut loom,
            namespace,
            TicketFieldDefinitionWriteRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                field_id: "title",
                key: "title",
                name: "Title",
                description: None,
                field_type: "string",
                option_set: None,
                max_length: None,
                required: false,
                searchable: true,
                orderable: true,
                cardinality: TicketFieldCardinality::Optional,
                applicable_type_ids: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let first = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned", "title": "Blocked ticket"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let second = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned", "title": "Dependency ticket"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();

        let relation = add_ticket_relation(
            &mut loom,
            namespace,
            TicketRelationRequest {
                workspace_id: &workspace_id,
                ticket_id: &first.ticket_id,
                relation_id: Some("dependency"),
                kind: TicketRelationKind::DependsOn,
                target_id: &second.primary_key,
                expected_root: None,
            },
        )
        .unwrap();

        let edge = loom_core::graph::graph_get_edge(
            &loom,
            namespace,
            "ticket-relations",
            &relation.graph_edge_id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(edge.src, format!("ticket:{}", first.ticket_id));
        assert_eq!(edge.dst, format!("ticket:{}", second.ticket_id));
        assert_eq!(edge.label, "depends_on");
        let profile = TicketProfileReader::open(&loom, namespace, &workspace_id)
            .unwrap()
            .unwrap();
        assert!(
            profile
                .ticket(&first.ticket_id)
                .unwrap()
                .unwrap()
                .relations
                .contains_key("dependency")
        );
        drop(profile);
        let first_summary = get_ticket(&loom, namespace, &workspace_id, &first.primary_key)
            .unwrap()
            .unwrap();
        assert_eq!(first_summary.depends_on, vec![second.ticket_id.clone()]);
        assert!(first_summary.blocks.is_empty());
        assert_eq!(first_summary.relations.len(), 1);
        assert_eq!(first_summary.relations[0].kind, "depends_on");
        assert_eq!(first_summary.relations[0].target_id, second.ticket_id);
        let outgoing =
            list_ticket_relations(&loom, namespace, &workspace_id, &first.primary_key).unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].direction, "outgoing");
        assert_eq!(outgoing[0].kind, "depends_on");
        assert_eq!(outgoing[0].target_ticket_id, second.ticket_id);
        assert_eq!(outgoing[0].target_title, "Dependency ticket");
        let incoming =
            list_ticket_relations(&loom, namespace, &workspace_id, &second.primary_key).unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].direction, "incoming");
        assert_eq!(incoming[0].kind, "depends_on");
        assert_eq!(incoming[0].target_ticket_id, first.ticket_id);
        assert_eq!(incoming[0].target_title, "Blocked ticket");

        remove_ticket_relation(
            &mut loom,
            namespace,
            TicketRelationRemoveRequest {
                workspace_id: &workspace_id,
                ticket_id: &first.primary_key,
                relation_id: "dependency",
                expected_root: None,
            },
        )
        .unwrap();
        assert!(
            loom_core::graph::graph_get_edge(
                &loom,
                namespace,
                "ticket-relations",
                &relation.graph_edge_id
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn ticket_relation_cardinality_rejects_second_singleton_target() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        add_ticket_relation(
            &mut loom,
            namespace,
            TicketRelationRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                relation_id: Some("assignee-a"),
                kind: TicketRelationKind::AssignedTo,
                target_id: "agent:3",
                expected_root: None,
            },
        )
        .unwrap();
        let rejected = add_ticket_relation(
            &mut loom,
            namespace,
            TicketRelationRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                relation_id: Some("assignee-b"),
                kind: TicketRelationKind::AssignedTo,
                target_id: "agent:4",
                expected_root: None,
            },
        )
        .unwrap_err();
        assert_eq!(rejected.code, Code::InvalidArgument);
    }

    #[test]
    fn ticket_relation_reconciler_restores_missing_projection_edge() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let first = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let second = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let relation = add_ticket_relation(
            &mut loom,
            namespace,
            TicketRelationRequest {
                workspace_id: &workspace_id,
                ticket_id: &first.ticket_id,
                relation_id: Some("blocks-target"),
                kind: TicketRelationKind::Blocks,
                target_id: &second.ticket_id,
                expected_root: None,
            },
        )
        .unwrap();
        assert!(
            loom_core::graph::graph_remove_edge(
                &mut loom,
                namespace,
                "ticket-relations",
                &relation.graph_edge_id
            )
            .unwrap()
        );

        let writes =
            reconcile_ticket_relation_projection(&mut loom, namespace, &workspace_id, 16).unwrap();
        assert!(writes > 0);
        assert!(
            loom_core::graph::graph_get_edge(
                &loom,
                namespace,
                "ticket-relations",
                &relation.graph_edge_id
            )
            .unwrap()
            .is_some()
        );
    }

    #[test]
    fn ticket_collaboration_primitives_persist_history_and_notifications() {
        let (mut loom, namespace, admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "open"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();

        add_ticket_comment(
            &mut loom,
            namespace,
            TicketCommentRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                comment_id: Some("c1"),
                comment_type: Some("review_request"),
                body: "Ready for review",
                evidence: None,
                expected_root: Some(&ticket.profile_root),
            },
        )
        .unwrap();
        update_ticket_comment(
            &mut loom,
            namespace,
            TicketCommentUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                comment_id: "c1",
                comment_type: Some("review_feedback"),
                body: Some("Needs evidence"),
                evidence: None,
                expected_root: None,
            },
        )
        .unwrap();
        let comments =
            list_ticket_comments(&loom, namespace, &workspace_id, &ticket.ticket_id).unwrap();
        assert_eq!(comments[0].comment_type, "review_feedback");
        assert_eq!(comments[0].author_principal, admin.to_string());
        assert_eq!(comments[0].body, "Needs evidence");
        assert!(comments[0].updated_at_ms.is_some());
        delete_ticket_comment(
            &mut loom,
            namespace,
            TicketCommentDeleteRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                comment_id: "c1",
                expected_root: None,
            },
        )
        .unwrap();
        let digest = Digest::hash(loom.store().digest_algo(), b"attachment");
        add_ticket_attachment(
            &mut loom,
            namespace,
            TicketAttachmentRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                attachment_id: Some("a1"),
                digest,
                name: "evidence.txt",
                media_type: "text/plain",
                size: 10,
                shared: true,
                expected_root: None,
            },
        )
        .unwrap();
        set_ticket_watch(
            &mut loom,
            namespace,
            TicketWatchRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                principal: Some("agent:3"),
                watch: true,
                expected_root: None,
            },
        )
        .unwrap();
        set_ticket_rank(
            &mut loom,
            namespace,
            TicketRankRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                rank_token: "m",
                expected_root: None,
            },
        )
        .unwrap();

        let profile = TicketProfileReader::open(&loom, namespace, &workspace_id)
            .unwrap()
            .unwrap();
        let comments = profile.comments(&ticket.ticket_id).unwrap();
        let attachments = profile.attachments(&ticket.ticket_id).unwrap();
        let watchers = profile.watchers(&ticket.ticket_id).unwrap();
        assert_eq!(comments[0].comment_type, "review_feedback");
        assert_eq!(comments[0].author_principal, admin.to_string());
        assert!(comments[0].redacted);
        assert!(comments[0].body.is_empty());
        assert_eq!(attachments[0].name, "evidence.txt");
        assert!(attachments[0].shared);
        assert!(watchers.contains(&"agent:3".to_string()));
        assert_eq!(
            profile.rank_token(&ticket.ticket_id).unwrap().as_deref(),
            Some("m")
        );
        drop(profile);

        let history = history(&loom, namespace, &workspace_id, Some(&ticket.ticket_id)).unwrap();
        assert!(
            history
                .iter()
                .any(|record| record.operation_kind == "ticket.comment_added")
        );
        assert!(history.iter().any(|record| {
            record
                .comments
                .iter()
                .any(|comment| comment.comment_type == "review_feedback")
        }));
        assert!(
            history
                .iter()
                .any(|record| record.operation_kind == "ticket.comment_updated")
        );
        assert!(
            history
                .iter()
                .any(|record| record.operation_kind == "ticket.comment_deleted")
        );
        assert!(
            history
                .iter()
                .any(|record| record.operation_kind == "ticket.attachment_added")
        );
        assert!(
            history
                .iter()
                .any(|record| record.operation_kind == "ticket.watcher_added")
        );
        assert!(
            history
                .iter()
                .any(|record| record.operation_kind == "ticket.ranked")
        );
        let replay = loom_core::delivery::delivery_replay(
            &loom,
            namespace,
            &ticket_change_stream(&workspace_id),
            "client",
            None,
            false,
            8,
        )
        .unwrap();
        assert!(replay.messages.len() >= 4);
        assert!(replay.messages.iter().any(|message| {
            message.envelope.subject == format!("ticket:{}", ticket.ticket_id)
                && serde_json::from_slice::<serde_json::Value>(&message.payload)
                    .unwrap()["operation_kind"]
                    == "ticket.comment_added"
        }));
    }

    #[test]
    fn ticket_update_applies_fields_and_lifecycle_action_atomically() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        define_optional_string_task_fields(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            &["due_at"],
        );
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned", "title": "Before"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let updated = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: Some(
                    &serde_json::json!({"title": "After", "due_at": "2026-08-01T00:00:00Z"}),
                ),
                delete_fields: &[],
                action: Some(TicketLifecycleAction::Claim),
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
        .unwrap();
        assert_eq!(updated.fields["status"], "in_progress");
        assert_eq!(updated.fields["title"], "After");
        assert_eq!(updated.fields["due_at"], "2026-08-01T00:00:00Z");
        assert_eq!(
            updated.sequence,
            ticket.sequence.map(|sequence| sequence + 1)
        );
        let history = history(&loom, namespace, &workspace_id, Some(&ticket.ticket_id)).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].operation_kind, "ticket.transitioned");
        let replay = loom_core::delivery::delivery_replay(
            &loom,
            namespace,
            &ticket_change_stream(&workspace_id),
            "client",
            None,
            false,
            8,
        )
        .unwrap();
        assert!(replay.messages.iter().any(|message| {
            message.envelope.subject == format!("ticket:{}", ticket.ticket_id)
                && serde_json::from_slice::<serde_json::Value>(&message.payload)
                    .unwrap()["operation_kind"]
                    == "ticket.transitioned"
        }));
        loom_core::delivery::delivery_ack(
            &mut loom,
            namespace,
            &ticket_change_stream(&workspace_id),
            "client",
            replay.messages[0].envelope.seq,
        )
        .unwrap();
        let read =
            get_ticket_with_projection(&loom, namespace, &workspace_id, &ticket.ticket_id, None)
                .unwrap()
                .unwrap();
        assert_eq!(read.fields["status"], "in_progress");
    }

    #[test]
    fn ticket_update_adds_status_and_comment_atomically() {
        let (mut loom, namespace, admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let updated = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("in_progress"),
                observed_source_status: Some("planned"),
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&ticket.profile_root),
                comment: Some(TicketUpdateCommentRequest {
                    comment_id: Some("progress"),
                    comment_type: Some("progress"),
                    body: "Started implementation",
                    evidence: None,
                }),
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        assert_eq!(updated.fields["status"], "in_progress");
        assert_eq!(updated.comments.len(), 1);
        assert_eq!(updated.comments[0].comment_id, "progress");
        assert_eq!(updated.comments[0].comment_type, "progress");
        assert_eq!(updated.comments[0].author_principal, admin.to_string());

        let comments =
            list_ticket_comments(&loom, namespace, &workspace_id, &ticket.ticket_id).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].body, "Started implementation");
        assert_eq!(comments[0].comment_type, "progress");
        assert_eq!(comments[0].author_principal, admin.to_string());
        let history = history(&loom, namespace, &workspace_id, Some(&ticket.ticket_id)).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].operation_kind, "ticket.transitioned");

        let rejected = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("waiting_for_review"),
                observed_source_status: Some("in_progress"),
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&ticket.profile_root),
                comment: Some(TicketUpdateCommentRequest {
                    comment_id: Some("stale"),
                    comment_type: Some("progress"),
                    body: "This must not be inserted",
                    evidence: None,
                }),
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap_err();
        assert_eq!(rejected.code, Code::Conflict);
        let comments =
            list_ticket_comments(&loom, namespace, &workspace_id, &ticket.ticket_id).unwrap();
        assert!(comments.iter().all(|comment| comment.comment_id != "stale"));
    }

    #[test]
    fn ticket_update_composes_status_comment_and_relations_atomically() {
        let (mut loom, namespace, admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let source = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let target = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: Some(&source.profile_root),
            },
        )
        .unwrap();
        let relation_sets = [TicketUpdateRelationSetRequest {
            relation_id: Some("dependency"),
            kind: TicketRelationKind::DependsOn,
            target_id: &target.ticket_id,
        }];
        let updated = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &source.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("blocked"),
                observed_source_status: Some("planned"),
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&target.profile_root),
                comment: Some(TicketUpdateCommentRequest {
                    comment_id: Some("blocked"),
                    comment_type: Some("blocker"),
                    body: "Blocked on dependency",
                    evidence: None,
                }),
                comments: &[],
                relation_sets: &relation_sets,
                relation_removes: &[],
            },
        )
        .unwrap();
        assert_eq!(updated.fields["status"], "blocked");
        assert_eq!(updated.comments.len(), 1);
        assert_eq!(updated.comments[0].author_principal, admin.to_string());
        assert_eq!(updated.relations.len(), 1);
        assert_eq!(updated.relations[0].relation_id, "dependency");
        assert_eq!(updated.relations[0].kind, "depends_on");
        assert_eq!(updated.relations[0].target_id, target.ticket_id);
        let relations =
            list_ticket_relations(&loom, namespace, &workspace_id, &source.ticket_id).unwrap();
        assert_eq!(relations.len(), 1);
        assert_eq!(relations[0].direction, "outgoing");
        assert_eq!(relations[0].kind, "depends_on");
        assert_eq!(relations[0].target_ticket_id, target.ticket_id);
        let history = history(&loom, namespace, &workspace_id, Some(&source.ticket_id)).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].operation_kind, "ticket.transitioned");
        assert!(
            history[1]
                .comments
                .iter()
                .any(|comment| comment.comment_type == "blocker")
        );

        let failing = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: Some(&updated.profile_root),
            },
        )
        .unwrap();
        let missing_removes = [TicketUpdateRelationRemoveRequest {
            relation_id: "missing",
        }];
        let rejected = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &failing.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("in_progress"),
                observed_source_status: Some("planned"),
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&failing.profile_root),
                comment: Some(TicketUpdateCommentRequest {
                    comment_id: Some("should-not-exist"),
                    comment_type: Some("progress"),
                    body: "This should not be committed",
                    evidence: None,
                }),
                comments: &[],
                relation_sets: &[],
                relation_removes: &missing_removes,
            },
        )
        .unwrap_err();
        assert_eq!(rejected.code, Code::NotFound);
        let unchanged =
            get_ticket_with_projection(&loom, namespace, &workspace_id, &failing.ticket_id, None)
                .unwrap()
                .unwrap();
        assert_eq!(unchanged.fields["status"], "planned");
        let comments =
            list_ticket_comments(&loom, namespace, &workspace_id, &failing.ticket_id).unwrap();
        assert!(comments.is_empty());
    }

    #[test]
    fn ticket_read_projects_linked_ticket_state_and_relation_rollup() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let parent = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned", "title": "Parent"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let child = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "in_progress", "title": "Child"}),
                policy_labels: &[],
                expected_root: Some(&parent.profile_root),
            },
        )
        .unwrap();
        let relation_sets = [TicketUpdateRelationSetRequest {
            relation_id: Some("child"),
            kind: TicketRelationKind::ParentOf,
            target_id: &child.ticket_id,
        }];
        update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &parent.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: None,
                observed_source_status: None,
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&child.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &relation_sets,
                relation_removes: &[],
            },
        )
        .unwrap();

        let read = get_ticket(&loom, namespace, &workspace_id, &parent.ticket_id)
            .unwrap()
            .unwrap();
        assert_eq!(read.relations.len(), 1);
        let target = read.relations[0].target.as_ref().unwrap();
        assert_eq!(target.primary_key, child.primary_key);
        assert_eq!(target.title.as_deref(), Some("Child"));
        assert_eq!(target.status.as_deref(), Some("in_progress"));
        assert!(!target.blocked);
        assert_eq!(read.relation_rollup.total_children, 1);
        assert_eq!(read.relation_rollup.in_progress_children, 1);
    }

    #[test]
    fn ticket_update_routes_status_field_through_transition_path() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let updated = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: Some(&serde_json::json!({"status": "accepted"})),
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
        .unwrap();
        assert_eq!(updated.fields["status"], "accepted");
        let current = get_ticket(&loom, namespace, &workspace_id, &ticket.ticket_id)
            .unwrap()
            .unwrap();
        assert_eq!(current.fields["status"], "accepted");
        let history = history(&loom, namespace, &workspace_id, Some(&ticket.ticket_id)).unwrap();
        assert_eq!(history[1].operation_kind, "ticket.transitioned");
    }

    #[test]
    fn ticket_update_deletes_fields_atomically() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        define_optional_string_task_fields(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            &["source_document", "result_target"],
        );
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({
                    "status": "planned",
                    "title": "Before",
                    "source_document": "tasks/STORE-MAINTENANCE-001A",
                    "result_target": "results/STORE-MAINTENANCE-001A/run-001"
                }),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let deleted = vec!["source_document".to_string(), "result_target".to_string()];
        let updated = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: Some(&serde_json::json!({"title": "After"})),
                delete_fields: &deleted,
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
        .unwrap();

        assert_eq!(updated.fields["status"], "planned");
        assert_eq!(updated.fields["title"], "After");
        assert!(!updated.fields.contains_key("source_document"));
        assert!(!updated.fields.contains_key("result_target"));
        assert_eq!(
            updated.sequence,
            ticket.sequence.map(|sequence| sequence + 1)
        );
    }

    #[test]
    fn ticket_update_rejects_set_and_delete_overlap_without_mutation() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned", "title": "Before"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let deleted = vec!["title".to_string()];
        let rejected = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: Some(&serde_json::json!({"title": "After"})),
                delete_fields: &deleted,
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
        assert_eq!(rejected.code, Code::InvalidArgument);
        let current = get_ticket(&loom, namespace, &workspace_id, &ticket.ticket_id)
            .unwrap()
            .unwrap();
        assert_eq!(current.profile_root, ticket.profile_root);
        assert_eq!(current.fields["title"], "Before");
    }

    #[test]
    fn ticket_update_permissive_default_allows_direct_status_repairs() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let planned = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let accepted = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &planned.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("accepted"),
                observed_source_status: Some("planned"),
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&planned.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        assert_eq!(accepted.fields["status"], "accepted");
        let feedback = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &planned.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("feedback_available"),
                observed_source_status: Some("accepted"),
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&accepted.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        assert_eq!(feedback.fields["status"], "feedback_available");

        let blocked = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "blocked"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let accepted_from_blocked = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &blocked.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("accepted"),
                observed_source_status: Some("blocked"),
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&blocked.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        assert_eq!(accepted_from_blocked.fields["status"], "accepted");
    }

    #[test]
    fn ticket_update_rejects_invalid_transition_when_workflow_is_configured() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let workflow = default_ticket_workflow().unwrap();
        set_project_workflow(
            &mut loom,
            namespace,
            TicketProjectWorkflowRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                workflow: &workflow,
                expected_root: None,
            },
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let rejected = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: Some(TicketLifecycleAction::Complete),
                target_status: None,
                observed_source_status: None,
                observed_workflow_version: None,
                assignee: None,
                expected_root: None,
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap_err();
        assert_eq!(rejected.code, Code::Conflict);
        let current = get_ticket(&loom, namespace, &workspace_id, &ticket.ticket_id)
            .unwrap()
            .unwrap();
        assert_eq!(current.profile_root, ticket.profile_root);
        assert_eq!(current.fields["status"], "planned");
    }

    #[test]
    fn ticket_transition_reopens_accepted_work_through_workflow_edge() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let in_progress = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("in_progress"),
                observed_source_status: Some("planned"),
                observed_workflow_version: Some("v1"),
                assignee: None,
                expected_root: Some(&ticket.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        let review = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("waiting_for_review"),
                observed_source_status: Some("in_progress"),
                observed_workflow_version: Some("v1"),
                assignee: None,
                expected_root: Some(&in_progress.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        let accepted = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("accepted"),
                observed_source_status: Some("waiting_for_review"),
                observed_workflow_version: Some("v1"),
                assignee: None,
                expected_root: Some(&review.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        assert_eq!(accepted.fields["status"], "accepted");

        let reopened = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("in_progress"),
                observed_source_status: Some("accepted"),
                observed_workflow_version: Some("v1"),
                assignee: None,
                expected_root: Some(&accepted.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        assert_eq!(reopened.fields["status"], "in_progress");
        let history = history(&loom, namespace, &workspace_id, Some(&ticket.ticket_id)).unwrap();
        assert_eq!(history.len(), 5);
        assert_eq!(history[3].operation_kind, "ticket.transitioned");
        assert_eq!(history[4].operation_kind, "ticket.transitioned");
        assert_eq!(history[3].sequence + 1, history[4].sequence);
    }

    #[test]
    fn configured_project_workflow_controls_ticket_transitions() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let workflow = WorkflowDefinition::new(
            "custom",
            "custom-v1",
            BTreeSet::from([
                "triage".to_string(),
                "doing".to_string(),
                "verified".to_string(),
            ]),
            vec![
                crate::WorkflowEdge::new("start", "triage", "doing", Vec::new()).unwrap(),
                crate::WorkflowEdge::new("verify", "doing", "verified", Vec::new()).unwrap(),
                crate::WorkflowEdge::new("rework", "verified", "doing", Vec::new()).unwrap(),
            ],
        )
        .unwrap();
        set_project_workflow(
            &mut loom,
            namespace,
            TicketProjectWorkflowRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                workflow: &workflow,
                expected_root: None,
            },
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "triage"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let missing_edge = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("verified"),
                observed_source_status: Some("triage"),
                observed_workflow_version: Some("custom-v1"),
                assignee: None,
                expected_root: Some(&ticket.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap_err();
        assert_eq!(missing_edge.code, Code::Conflict);
        let doing = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("doing"),
                observed_source_status: Some("triage"),
                observed_workflow_version: Some("custom-v1"),
                assignee: None,
                expected_root: Some(&ticket.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        let verified = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("verified"),
                observed_source_status: Some("doing"),
                observed_workflow_version: Some("custom-v1"),
                assignee: None,
                expected_root: Some(&doing.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        let reworked = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: None,
                target_status: Some("doing"),
                observed_source_status: Some("verified"),
                observed_workflow_version: Some("custom-v1"),
                assignee: None,
                expected_root: Some(&verified.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        assert_eq!(reworked.fields["status"], "doing");
        let history = history(&loom, namespace, &workspace_id, Some(&ticket.ticket_id)).unwrap();
        assert_eq!(history.len(), 4);
        assert_eq!(history[1].operation_kind, "ticket.transitioned");
        assert_eq!(history[2].operation_kind, "ticket.transitioned");
        assert_eq!(history[3].operation_kind, "ticket.transitioned");
    }

    #[test]
    fn ticket_update_enforces_governed_lifecycle_authorization_without_mutation() {
        let (mut loom, namespace, admin, writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        set_project_lifecycle_policy(
            &mut loom,
            namespace,
            TicketProjectLifecyclePolicyRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                policy: TicketLifecycleAuthorizationPolicy::OwnershipGoverned,
                project_owner_principal: Some(&admin.to_string()),
                acceptance_authorities: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let ticket = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({"status": "planned"}),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        let in_progress = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: Some(TicketLifecycleAction::Claim),
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
        .unwrap();
        let reviewed = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: None,
                delete_fields: &[],
                action: Some(TicketLifecycleAction::RequestReview),
                target_status: None,
                observed_source_status: None,
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&in_progress.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap();
        let writer_session = loom
            .identity_store_mut()
            .unwrap()
            .authenticate_passphrase(writer, "writer", "writer-session")
            .unwrap();
        loom.set_session(writer_session.id);
        let denied = update_ticket(
            &mut loom,
            namespace,
            TicketUpdateRequest {
                workspace_id: &workspace_id,
                ticket_id: &ticket.ticket_id,
                set_fields: Some(&serde_json::json!({"title": "Unauthorized"})),
                delete_fields: &[],
                action: Some(TicketLifecycleAction::Accept),
                target_status: None,
                observed_source_status: None,
                observed_workflow_version: None,
                assignee: None,
                expected_root: Some(&reviewed.profile_root),
                comment: None,
                comments: &[],
                relation_sets: &[],
                relation_removes: &[],
            },
        )
        .unwrap_err();
        assert_eq!(denied.code, Code::PermissionDenied);
        let current = get_ticket(&loom, namespace, &workspace_id, &ticket.ticket_id)
            .unwrap()
            .unwrap();
        assert_eq!(current.profile_root, reviewed.profile_root);
        assert_eq!(current.fields["status"], "waiting_for_review");
        assert!(!current.fields.contains_key("title"));
    }

    #[test]
    fn ticket_query_filters_status_bucket_lane_parent_dependency_and_text() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        for field in [
            "bucket",
            "lane_owner",
            "queue_lane",
            "parent_ticket",
            "blocks",
        ] {
            put_ticket_field_definition(
                &mut loom,
                namespace,
                TicketFieldDefinitionWriteRequest {
                    workspace_id: &workspace_id,
                    project_id: "matrix",
                    field_id: field,
                    key: field,
                    name: field,
                    description: None,
                    field_type: "string",
                    option_set: None,
                    max_length: None,
                    required: false,
                    searchable: true,
                    orderable: false,
                    cardinality: TicketFieldCardinality::Optional,
                    applicable_type_ids: &[],
                    expected_root: None,
                },
            )
            .unwrap();
        }
        let wanted = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({
                    "title": "Make ticket discovery searchable",
                    "status": "blocked",
                    "bucket": "workgraph-ergonomics",
                    "lane_owner": "agent:3",
                    "queue_lane": "tickets-schema",
                    "parent_ticket": "MX-230",
                    "blocks": "MX-300"
                }),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({
                    "title": "Unrelated",
                    "status": "ready",
                    "bucket": "other",
                    "lane_owner": "agent:4",
                    "queue_lane": "other",
                    "parent_ticket": "MX-999"
                }),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();

        let results = query_tickets(
            &loom,
            namespace,
            TicketQueryRequest {
                workspace_id: &workspace_id,
                projection: None,
                statuses: &["blocked".to_string()],
                buckets: &["workgraph-ergonomics".to_string()],
                assignees: &[],
                lane_owners: &["agent:3".to_string()],
                parent_tickets: &["MX-230".to_string()],
                dependency_tickets: &["MX-300".to_string()],
                queue_lanes: &["tickets-schema".to_string()],
                title_contains: Some("discovery"),
                text_contains: Some("searchable"),
                field_equals: &BTreeMap::new(),
                limit: Some(10),
                offset: 0,
            },
        )
        .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].ticket_id, wanted.ticket_id);
    }

    #[test]
    fn ticket_list_lane_membership_accepts_primary_ticket_keys() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let wanted = create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({
                    "title": "Lane member",
                    "status": "ready"
                }),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
        create_ticket(
            &mut loom,
            namespace,
            TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "matrix",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &serde_json::json!({
                    "title": "Outside lane",
                    "status": "ready"
                }),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();

        let page = list_tickets_page(
            &loom,
            namespace,
            &workspace_id,
            &TicketListQuery {
                lane_member_ids: Some(vec![wanted.primary_key.clone()]),
                limit: Some(10),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].ticket_id, wanted.ticket_id);
        assert_eq!(page.items[0].primary_key, wanted.primary_key);
    }

    #[test]
    fn lane_scoped_list_defaults_to_incomplete_with_include_completed_and_status_override() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let mut mk = |title: &str, status: &str| {
            create_ticket(
                &mut loom,
                namespace,
                TicketCreateRequest {
                    workspace_id: &workspace_id,
                    project_id: "matrix",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({ "title": title, "status": status }),
                    policy_labels: &[],
                    expected_root: None,
                },
            )
            .unwrap()
        };
        let ready = mk("Ready", "ready");
        let working = mk("Working", "in_progress");
        let accepted = mk("Accepted", "accepted");
        let closed = mk("Closed", "closed");
        let rejected = mk("Rejected", "rejected");
        let lane = vec![
            ready.primary_key.clone(),
            working.primary_key.clone(),
            accepted.primary_key.clone(),
            closed.primary_key.clone(),
            rejected.primary_key.clone(),
        ];

        // Lane-scoped default hides terminal tickets (accepted/closed/rejected): only the two
        // incomplete tickets are returned.
        let page = list_tickets_page(
            &loom,
            namespace,
            &workspace_id,
            &TicketListQuery {
                lane_member_ids: Some(lane.clone()),
                limit: Some(50),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.total, 2);
        let shown: BTreeSet<&str> = page.items.iter().map(|t| t.ticket_id.as_str()).collect();
        assert!(shown.contains(ready.ticket_id.as_str()));
        assert!(shown.contains(working.ticket_id.as_str()));

        // include_completed broadens terminal visibility to all five.
        let page = list_tickets_page(
            &loom,
            namespace,
            &workspace_id,
            &TicketListQuery {
                lane_member_ids: Some(lane.clone()),
                include_completed: true,
                limit: Some(50),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.total, 5);

        // An explicit status filter is honored even for a terminal status with include_completed
        // false: the default hiding must not override an explicit filter.
        let page = list_tickets_page(
            &loom,
            namespace,
            &workspace_id,
            &TicketListQuery {
                lane_member_ids: Some(lane),
                statuses: vec!["accepted".to_string()],
                limit: Some(50),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].ticket_id, accepted.ticket_id);
    }

    #[test]
    fn lane_scoped_list_uses_stored_lane_order_and_non_lane_uses_recency() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let mut mk = |title: &str| {
            create_ticket(
                &mut loom,
                namespace,
                TicketCreateRequest {
                    workspace_id: &workspace_id,
                    project_id: "matrix",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({ "title": title, "status": "ready" }),
                    policy_labels: &[],
                    expected_root: None,
                },
            )
            .unwrap()
        };
        let a = mk("A");
        let b = mk("B");
        let c = mk("C");

        // Lane-scoped: sort follows the stored Lane order (c, a, b), not creation/recency.
        let page = list_tickets_page(
            &loom,
            namespace,
            &workspace_id,
            &TicketListQuery {
                lane_member_ids: Some(vec![
                    c.primary_key.clone(),
                    a.primary_key.clone(),
                    b.primary_key.clone(),
                ]),
                limit: Some(50),
                ..Default::default()
            },
        )
        .unwrap();
        let lane_order: Vec<&str> = page.items.iter().map(|t| t.ticket_id.as_str()).collect();
        assert_eq!(
            lane_order,
            vec![
                c.ticket_id.as_str(),
                a.ticket_id.as_str(),
                b.ticket_id.as_str()
            ]
        );

        // Non-lane: most-recently-updated first (c, b, a), unchanged from the default sort.
        let page = list_tickets_page(
            &loom,
            namespace,
            &workspace_id,
            &TicketListQuery {
                limit: Some(50),
                ..Default::default()
            },
        )
        .unwrap();
        let recency_order: Vec<&str> = page.items.iter().map(|t| t.ticket_id.as_str()).collect();
        assert_eq!(
            recency_order,
            vec![
                c.ticket_id.as_str(),
                b.ticket_id.as_str(),
                a.ticket_id.as_str()
            ]
        );
    }

    #[test]
    fn board_scoped_list_uses_board_order_and_lane_priority_when_combined() {
        let (mut loom, namespace, _admin, _writer) = authenticated_ticket_loom();
        let workspace_id = namespace.to_string();
        create_project(
            &mut loom,
            namespace,
            &workspace_id,
            "matrix",
            "MX",
            "Matrix",
            None,
        )
        .unwrap();
        let mut mk = |title: &str, status: &str| {
            create_ticket(
                &mut loom,
                namespace,
                TicketCreateRequest {
                    workspace_id: &workspace_id,
                    project_id: "matrix",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({ "title": title, "status": status }),
                    policy_labels: &[],
                    expected_root: None,
                },
            )
            .unwrap()
        };
        let a = mk("A", "ready");
        let b = mk("B", "in_progress");
        let c = mk("C", "accepted");
        let d = mk("D", "ready");
        let columns = vec![
            BoardColumn::with_display("todo", "To Do", BTreeSet::new(), None, false, 10).unwrap(),
        ];
        create_board(
            &mut loom,
            namespace,
            BoardCreateRequest {
                workspace_id: &workspace_id,
                board_id: "planning",
                board_key: "PLAN",
                name: "Planning",
                description: "",
                project_id: "matrix",
                scope: BoardScope::ManualSet,
                mode: BoardMode::Manual,
                columns: &columns,
                swimlanes: &[],
                card_display_fields: &[],
                owner_principal: None,
                coordinator_principal: None,
                updated_by: "test",
                expected_root: None,
            },
        )
        .unwrap();
        for (ticket, rank) in [
            (&b.ticket_id, "001"),
            (&c.ticket_id, "002"),
            (&a.ticket_id, "003"),
            (&d.ticket_id, "004"),
        ] {
            move_board_card(
                &mut loom,
                namespace,
                BoardCardMoveRequest {
                    workspace_id: &workspace_id,
                    board_id: "planning",
                    ticket_id: ticket,
                    column_id: "todo",
                    rank_token: rank,
                    swimlane_id: None,
                    updated_by: "test",
                    expected_root: None,
                },
            )
            .unwrap();
        }

        let page = list_tickets_page(
            &loom,
            namespace,
            &workspace_id,
            &TicketListQuery {
                board_id: Some("PLAN".to_string()),
                limit: Some(50),
                ..Default::default()
            },
        )
        .unwrap();
        let board_order: Vec<&str> = page.items.iter().map(|t| t.ticket_id.as_str()).collect();
        assert_eq!(
            board_order,
            vec![
                b.ticket_id.as_str(),
                a.ticket_id.as_str(),
                d.ticket_id.as_str()
            ]
        );

        let page = list_tickets_page(
            &loom,
            namespace,
            &workspace_id,
            &TicketListQuery {
                board_id: Some("Planning".to_string()),
                include_completed: true,
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.total, 4);
        assert!(page.next_cursor.is_some());
        let first_page: Vec<&str> = page.items.iter().map(|t| t.ticket_id.as_str()).collect();
        assert_eq!(first_page, vec![b.ticket_id.as_str(), c.ticket_id.as_str()]);

        let page = list_tickets_page(
            &loom,
            namespace,
            &workspace_id,
            &TicketListQuery {
                board_id: Some("planning".to_string()),
                lane_member_ids: Some(vec![a.primary_key.clone(), b.primary_key.clone()]),
                include_completed: true,
                limit: Some(50),
                ..Default::default()
            },
        )
        .unwrap();
        let lane_priority_order: Vec<&str> =
            page.items.iter().map(|t| t.ticket_id.as_str()).collect();
        assert_eq!(
            lane_priority_order,
            vec![a.ticket_id.as_str(), b.ticket_id.as_str()]
        );

        let page = list_tickets_page(
            &loom,
            namespace,
            &workspace_id,
            &TicketListQuery {
                board_id: Some("planning".to_string()),
                statuses: vec!["accepted".to_string()],
                limit: Some(50),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].ticket_id, c.ticket_id);
    }
}
