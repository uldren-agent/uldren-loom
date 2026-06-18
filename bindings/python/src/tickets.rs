//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_tickets::{
    BoardCardMoveRequest, BoardColumn, BoardColumnConfigureRequest, BoardCreateRequest, BoardMode,
    BoardScope, BoardStatus, BoardSwimlane, BoardUpdateRequest, TicketCommentDeleteRequest,
    TicketCommentRequest, TicketCommentUpdateRequest, TicketCreateRequest, TicketDeleteRequest,
    TicketFieldDefinitionRetireRequest, TicketFieldDefinitionWriteRequest, TicketLifecycleAction,
    TicketLifecycleAuthorizationPolicy, TicketRelationKind, TicketRelationRemoveRequest,
    TicketRelationRequest, TicketRelationSummary, TicketSummary, TicketUpdateCommentRequest,
    TicketUpdateRelationRemoveRequest, TicketUpdateRelationSetRequest, TicketUpdateRequest,
};
use loom_types::{MutationChange, MutationEnvelope, MutationReceipt};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value as JsonValue;
use std::collections::BTreeSet;

fn to_json<T: Serialize>(value: loom_core::error::Result<T>) -> PyResult<String> {
    let value = value.map_err(py_err)?;
    serde_json::to_string(&value).map_err(|error| PyRuntimeError::new_err(error.to_string()))
}

fn ticket_field_value_changes(fields: &JsonValue) -> Vec<MutationChange> {
    fields.as_object().map_or_else(Vec::new, |fields| {
        fields
            .iter()
            .map(|(field, value)| MutationChange::field_set(field.clone(), value.to_string()))
            .collect()
    })
}

fn ticket_update_changes(
    set_fields: Option<&JsonValue>,
    delete_fields: &[String],
    action_applied: bool,
    target_status: Option<&str>,
    observed_source_status: Option<&str>,
    assignee: Option<&str>,
    comment_types: impl IntoIterator<Item = Option<String>>,
    relation_sets: impl IntoIterator<Item = (String, String, String)>,
    relation_removes: impl IntoIterator<Item = String>,
) -> Vec<MutationChange> {
    let mut changes = set_fields
        .map(ticket_field_value_changes)
        .unwrap_or_default();
    changes.extend(
        delete_fields
            .iter()
            .map(|field| MutationChange::field_deleted(field.clone(), None::<String>)),
    );
    if let Some(target_status) = target_status {
        changes.push(MutationChange::field_changed(
            "status",
            observed_source_status.map(str::to_string),
            Some(target_status.to_string()),
        ));
    }
    if let Some(assignee) = assignee {
        changes.push(MutationChange::field_changed(
            "assignee",
            None::<String>,
            Some(assignee.to_string()),
        ));
    }
    if action_applied && target_status.is_none() {
        changes.push(MutationChange::field_set("lifecycle_action", "applied"));
    }
    for comment_type in comment_types {
        changes.push(MutationChange::field_set(
            "comment",
            comment_type.unwrap_or_else(|| "comment".to_string()),
        ));
    }
    changes.extend(
        relation_sets
            .into_iter()
            .map(|(relation_id, kind, target_id)| {
                MutationChange::relation_set(relation_id, kind, target_id)
            }),
    );
    changes.extend(relation_removes.into_iter().map(|relation_id| {
        MutationChange::field_deleted(format!("relation:{relation_id}"), None::<String>)
    }));
    changes
}

fn ticket_mutation_json(
    ticket: TicketSummary,
    operation: &str,
    root_before: Option<&str>,
    changes: Vec<MutationChange>,
) -> PyResult<String> {
    let receipt = MutationReceipt::new(operation, "ticket", ticket.primary_key.clone())
        .operation_id(ticket.operation_id.clone())
        .roots(
            root_before.map(str::to_string),
            Some(ticket.profile_root.clone()),
        )
        .changes(changes);
    serde_json::to_string(&MutationEnvelope::new(ticket, receipt))
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))
}

fn relation_mutation_json(
    relation: TicketRelationSummary,
    operation: &str,
    root_before: Option<&str>,
    changes: Vec<MutationChange>,
) -> PyResult<String> {
    let receipt = MutationReceipt::new(operation, "ticket_relation", relation.relation_id.clone())
        .operation_id(Some(relation.operation_id.clone()))
        .roots(
            root_before.map(str::to_string),
            Some(relation.profile_root.clone()),
        )
        .changes(changes);
    serde_json::to_string(&MutationEnvelope::new(relation, receipt))
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))
}

fn parse_json(value: &str, what: &str) -> PyResult<JsonValue> {
    serde_json::from_str(value).map_err(|error| PyRuntimeError::new_err(format!("{what}: {error}")))
}

fn parse_string_list(value: &str, what: &str) -> PyResult<Vec<String>> {
    serde_json::from_str(value).map_err(|error| PyRuntimeError::new_err(format!("{what}: {error}")))
}

fn parse_optional_json_list<T: DeserializeOwned>(
    value: Option<&str>,
    what: &str,
) -> PyResult<Vec<T>> {
    value
        .map(|value| serde_json::from_str(value))
        .transpose()
        .map_err(|error| PyRuntimeError::new_err(format!("{what}: {error}")))
        .map(|value| value.unwrap_or_default())
}

#[derive(Deserialize)]
struct PyTicketUpdateComment {
    #[serde(default)]
    comment_id: Option<String>,
    #[serde(default)]
    comment_type: Option<String>,
    body: String,
}

#[derive(Deserialize)]
struct PyTicketUpdateRelationSet {
    #[serde(default)]
    relation_id: Option<String>,
    kind: String,
    target_id: String,
}

#[derive(Deserialize)]
struct PyTicketUpdateRelationRemove {
    relation_id: String,
}

fn parse_projection_list(
    value: &str,
    what: &str,
) -> PyResult<Vec<loom_tickets::TicketProjectionProfile>> {
    parse_string_list(value, what)?
        .into_iter()
        .map(|profile| loom_tickets::TicketProjectionProfile::parse(&profile).map_err(py_err))
        .collect()
}

fn parse_cardinality(value: &str) -> PyResult<loom_tickets::TicketFieldCardinality> {
    match value {
        "single" => Ok(loom_tickets::TicketFieldCardinality::Single),
        "optional" => Ok(loom_tickets::TicketFieldCardinality::Optional),
        "list" => Ok(loom_tickets::TicketFieldCardinality::List {
            min_items: 0,
            max_items: None,
        }),
        other => Err(PyRuntimeError::new_err(format!(
            "ticket field cardinality must be single, optional, or list: {other}"
        ))),
    }
}

#[derive(Deserialize)]
struct PythonBoardColumn {
    column_id: String,
    name: String,
    #[serde(default)]
    mapped_statuses: Vec<String>,
    #[serde(default)]
    wip_limit: Option<u32>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    rank: u64,
}

#[derive(Deserialize)]
struct PythonBoardSwimlane {
    swimlane_id: String,
    name: String,
    #[serde(default)]
    predicate: Option<String>,
    #[serde(default)]
    rank: u64,
}

#[derive(Deserialize)]
struct PythonBoardCreateRequest {
    board_id: String,
    board_key: String,
    name: String,
    project_id: String,
    #[serde(default)]
    description: String,
    #[serde(default = "default_board_mode")]
    mode: String,
    #[serde(default = "default_board_scope")]
    scope: String,
    #[serde(default)]
    columns: Vec<PythonBoardColumn>,
    #[serde(default)]
    swimlanes: Vec<PythonBoardSwimlane>,
    #[serde(default)]
    card_display_fields: Vec<String>,
    #[serde(default)]
    owner_principal: Option<String>,
    #[serde(default)]
    coordinator_principal: Option<String>,
    #[serde(default = "default_updated_by")]
    updated_by: String,
    #[serde(default)]
    expected_root: Option<String>,
}

#[derive(Deserialize)]
struct PythonBoardUpdateRequest {
    #[serde(default)]
    board_key: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    board_status: Option<String>,
    #[serde(default)]
    card_display_fields: Option<Vec<String>>,
    #[serde(default = "default_updated_by")]
    updated_by: String,
    #[serde(default)]
    expected_root: Option<String>,
}

#[derive(Deserialize)]
struct PythonBoardConfigureColumnsRequest {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    columns: Vec<PythonBoardColumn>,
    #[serde(default)]
    swimlanes: Vec<PythonBoardSwimlane>,
    #[serde(default = "default_updated_by")]
    updated_by: String,
    #[serde(default)]
    expected_root: Option<String>,
}

#[derive(Deserialize)]
struct PythonBoardMoveCardRequest {
    ticket_id: String,
    column_id: String,
    rank_token: String,
    #[serde(default)]
    swimlane_id: Option<String>,
    #[serde(default = "default_updated_by")]
    updated_by: String,
    #[serde(default)]
    expected_root: Option<String>,
}

fn default_board_mode() -> String {
    "status_mapped".to_string()
}

fn default_board_scope() -> String {
    "project".to_string()
}

fn default_updated_by() -> String {
    "python".to_string()
}

fn board_columns(columns: Vec<PythonBoardColumn>) -> PyResult<Vec<BoardColumn>> {
    columns
        .into_iter()
        .map(|column| {
            BoardColumn::with_display(
                column.column_id,
                column.name,
                column.mapped_statuses.into_iter().collect::<BTreeSet<_>>(),
                column.wip_limit,
                column.hidden,
                column.rank,
            )
            .map_err(py_err)
        })
        .collect()
}

fn board_swimlanes(swimlanes: Vec<PythonBoardSwimlane>) -> PyResult<Vec<BoardSwimlane>> {
    swimlanes
        .into_iter()
        .map(|swimlane| {
            BoardSwimlane::new(
                swimlane.swimlane_id,
                swimlane.name,
                swimlane.predicate,
                swimlane.rank,
            )
            .map_err(py_err)
        })
        .collect()
}

fn board_scope(kind: &str, project_id: &str) -> PyResult<BoardScope> {
    match kind {
        "project" => Ok(BoardScope::project(project_id.to_string())),
        "manual_set" => Ok(BoardScope::ManualSet),
        _ => Err(PyRuntimeError::new_err(
            "board scope must be project or manual_set",
        )),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn tickets_read<T>(
    path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> PyResult<T>,
) -> PyResult<T> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    f(&loom, workspace_id)
}

fn tickets_write<T>(
    path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    f: impl FnOnce(&mut Loom<FileStore>, WorkspaceId) -> PyResult<T>,
) -> PyResult<T> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    let out = f(&mut loom, workspace_id)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(out)
}

fn sync_ticket_references(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    ticket: &TicketSummary,
) -> PyResult<()> {
    loom_tickets::update_ticket_field_references(
        loom,
        workspace,
        &ticket.workspace_id,
        &ticket.ticket_id,
        &ticket.fields,
    )
    .map_err(py_err)?;
    if let Some(operation_id) = ticket.operation_id.as_deref() {
        loom_tickets::enqueue_ticket_reference_candidates(
            loom,
            workspace,
            loom_tickets::TicketReferenceCandidateRequest {
                workspace_id: &ticket.workspace_id,
                ticket_id: &ticket.ticket_id,
                operation_id,
                source_root: Digest::parse(&ticket.profile_root).map_err(py_err)?,
                fields: &ticket.fields,
                now_ms: now_ms(),
            },
        )
        .map_err(py_err)?;
    }
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, project_id, key_prefix, name, expected_root=None, passphrase=None))]
pub(crate) fn tickets_project_create_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    project_id: &str,
    key_prefix: &str,
    name: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_tickets::create_project(
            loom,
            ns,
            ticket_workspace_id,
            project_id,
            key_prefix,
            name,
            expected_root,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, project_id, key_prefix, expected_root=None, passphrase=None))]
pub(crate) fn tickets_project_rekey_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    project_id: &str,
    key_prefix: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_tickets::rekey_project(
            loom,
            ns,
            ticket_workspace_id,
            project_id,
            key_prefix,
            expected_root,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, project_id, passphrase=None))]
pub(crate) fn tickets_project_settings_get_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    project_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_tickets::get_project(
            loom,
            ns,
            ticket_workspace_id,
            project_id,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, project_id, default_projection=None, enable_projections_json="[]", disable_projections_json="[]", actor_enforcement=None, project_owner_principal=None, clear_project_owner_principal=false, acceptance_authorities_json=None, expected_root=None, passphrase=None))]
pub(crate) fn tickets_project_settings_set_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    project_id: &str,
    default_projection: Option<&str>,
    enable_projections_json: &str,
    disable_projections_json: &str,
    actor_enforcement: Option<&str>,
    project_owner_principal: Option<&str>,
    clear_project_owner_principal: bool,
    acceptance_authorities_json: Option<&str>,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let default_projection = default_projection
        .map(loom_tickets::TicketProjectionProfile::parse)
        .transpose()
        .map_err(py_err)?;
    let enable_projections =
        parse_projection_list(enable_projections_json, "ticket enable projections json")?;
    let disable_projections =
        parse_projection_list(disable_projections_json, "ticket disable projections json")?;
    let actor_enforcement = actor_enforcement
        .map(TicketLifecycleAuthorizationPolicy::parse)
        .transpose()
        .map_err(py_err)?;
    let acceptance_authorities = acceptance_authorities_json
        .map(|value| parse_string_list(value, "ticket acceptance authorities json"))
        .transpose()?;
    tickets_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_tickets::set_project_settings(
            loom,
            ns,
            loom_tickets::TicketProjectSettingsRequest {
                workspace_id: ticket_workspace_id,
                project_id,
                default_projection,
                enable_projections: &enable_projections,
                disable_projections: &disable_projections,
                actor_enforcement,
                project_owner_principal,
                clear_project_owner_principal,
                acceptance_authorities: acceptance_authorities.as_deref(),
                expected_root,
            },
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, project_id=None, projection=None, operation=None, passphrase=None))]
pub(crate) fn tickets_fields_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    project_id: Option<&str>,
    projection: Option<&str>,
    operation: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_read(path, workspace, passphrase, |loom, ns| {
        let projection = loom_tickets::parse_ticket_projection(projection).map_err(py_err)?;
        match project_id {
            Some(project_id) => to_json(loom_tickets::ticket_field_catalog_for_project(
                loom,
                ns,
                ticket_workspace_id,
                project_id,
                projection,
                operation,
            )),
            None => to_json(loom_tickets::ticket_field_catalog(projection, operation)),
        }
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, project_id, field_id, key, name, description=None, field_type="string", option_set=None, max_length=None, required=false, searchable=true, orderable=false, cardinality="optional", applicable_type_ids_json="[]", expected_root=None, passphrase=None))]
pub(crate) fn tickets_field_put_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    project_id: &str,
    field_id: &str,
    key: &str,
    name: &str,
    description: Option<&str>,
    field_type: &str,
    option_set: Option<&str>,
    max_length: Option<u32>,
    required: bool,
    searchable: bool,
    orderable: bool,
    cardinality: &str,
    applicable_type_ids_json: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let cardinality = parse_cardinality(cardinality)?;
    let applicable_type_ids =
        parse_string_list(applicable_type_ids_json, "ticket applicable type ids json")?;
    tickets_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_tickets::put_ticket_field_definition(
            loom,
            ns,
            TicketFieldDefinitionWriteRequest {
                workspace_id: ticket_workspace_id,
                project_id,
                field_id,
                key,
                name,
                description,
                field_type,
                option_set,
                max_length,
                required,
                searchable,
                orderable,
                cardinality,
                applicable_type_ids: &applicable_type_ids,
                expected_root,
            },
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, project_id, field_id, expected_root=None, passphrase=None))]
pub(crate) fn tickets_field_retire_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    project_id: &str,
    field_id: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_tickets::retire_ticket_field_definition(
            loom,
            ns,
            TicketFieldDefinitionRetireRequest {
                workspace_id: ticket_workspace_id,
                project_id,
                field_id,
                expected_root,
            },
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, project_id, ticket_type, external_source, external_id, fields_json, policy_labels_json, expected_root=None, passphrase=None))]
pub(crate) fn tickets_create_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    project_id: &str,
    ticket_type: &str,
    external_source: Option<&str>,
    external_id: Option<&str>,
    fields_json: &str,
    policy_labels_json: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let fields = parse_json(fields_json, "ticket fields json")?;
    let policy_labels = parse_string_list(policy_labels_json, "ticket policy labels json")?;
    tickets_write(path, workspace, passphrase, |loom, ns| {
        let ticket = loom_tickets::create_ticket(
            loom,
            ns,
            TicketCreateRequest {
                workspace_id: ticket_workspace_id,
                project_id,
                ticket_type,
                external_source,
                external_id,
                fields: &fields,
                policy_labels: &policy_labels,
                expected_root,
            },
        )
        .map_err(py_err)?;
        sync_ticket_references(loom, ns, &ticket)?;
        ticket_mutation_json(
            ticket,
            "ticket.created",
            expected_root,
            vec![MutationChange::ResourceCreated],
        )
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, ticket_id, set_fields_json, delete_fields_json, action=None, target_status=None, observed_source_status=None, observed_workflow_version=None, assignee=None, comment_id=None, comment_type=None, comment_body=None, expected_root=None, passphrase=None, comments_json=None, relation_sets_json=None, relation_removes_json=None))]
pub(crate) fn tickets_update_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    ticket_id: &str,
    set_fields_json: Option<&str>,
    delete_fields_json: &str,
    action: Option<&str>,
    target_status: Option<&str>,
    observed_source_status: Option<&str>,
    observed_workflow_version: Option<&str>,
    assignee: Option<&str>,
    comment_id: Option<&str>,
    comment_type: Option<&str>,
    comment_body: Option<&str>,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
    comments_json: Option<&str>,
    relation_sets_json: Option<&str>,
    relation_removes_json: Option<&str>,
) -> PyResult<String> {
    if comment_body.is_none() && (comment_id.is_some() || comment_type.is_some()) {
        return Err(py_err(loom_core::LoomError::invalid(
            "ticket update comment id and type require comment body",
        )));
    }
    let set_fields = set_fields_json
        .map(|value| parse_json(value, "ticket set fields json"))
        .transpose()?;
    let delete_fields = parse_string_list(delete_fields_json, "ticket delete fields json")?;
    let action = action
        .map(TicketLifecycleAction::parse)
        .transpose()
        .map_err(py_err)?;
    let comments_input =
        parse_optional_json_list::<PyTicketUpdateComment>(comments_json, "ticket comments json")?;
    let relation_sets_input = parse_optional_json_list::<PyTicketUpdateRelationSet>(
        relation_sets_json,
        "ticket relation sets json",
    )?;
    let relation_removes_input = parse_optional_json_list::<PyTicketUpdateRelationRemove>(
        relation_removes_json,
        "ticket relation removes json",
    )?;
    let relation_kinds = relation_sets_input
        .iter()
        .map(|relation| TicketRelationKind::parse(&relation.kind).map_err(py_err))
        .collect::<PyResult<Vec<_>>>()?;
    let changes = ticket_update_changes(
        set_fields.as_ref(),
        &delete_fields,
        action.is_some(),
        target_status,
        observed_source_status,
        assignee,
        comment_type
            .map(|value| Some(value.to_string()))
            .into_iter()
            .chain(
                comments_input
                    .iter()
                    .map(|comment| comment.comment_type.clone()),
            ),
        relation_sets_input.iter().map(|relation| {
            (
                relation
                    .relation_id
                    .clone()
                    .unwrap_or_else(|| relation.target_id.clone()),
                relation.kind.clone(),
                relation.target_id.clone(),
            )
        }),
        relation_removes_input
            .iter()
            .map(|relation| relation.relation_id.clone()),
    );
    let comment = comment_body.map(|body| loom_tickets::TicketUpdateCommentRequest {
        comment_id,
        comment_type,
        body,
    });
    let comments = comments_input
        .iter()
        .map(|comment| TicketUpdateCommentRequest {
            comment_id: comment.comment_id.as_deref(),
            comment_type: comment.comment_type.as_deref(),
            body: &comment.body,
        })
        .collect::<Vec<_>>();
    let relation_sets = relation_sets_input
        .iter()
        .zip(relation_kinds.iter())
        .map(|(relation, kind)| TicketUpdateRelationSetRequest {
            relation_id: relation.relation_id.as_deref(),
            kind: *kind,
            target_id: &relation.target_id,
        })
        .collect::<Vec<_>>();
    let relation_removes = relation_removes_input
        .iter()
        .map(|relation| TicketUpdateRelationRemoveRequest {
            relation_id: &relation.relation_id,
        })
        .collect::<Vec<_>>();
    tickets_write(path, workspace, passphrase, |loom, ns| {
        let ticket = loom_tickets::update_ticket(
            loom,
            ns,
            TicketUpdateRequest {
                workspace_id: ticket_workspace_id,
                ticket_id,
                set_fields: set_fields.as_ref(),
                delete_fields: &delete_fields,
                action,
                target_status,
                observed_source_status,
                observed_workflow_version,
                assignee,
                expected_root,
                comment,
                comments: &comments,
                relation_sets: &relation_sets,
                relation_removes: &relation_removes,
            },
        )
        .map_err(py_err)?;
        sync_ticket_references(loom, ns, &ticket)?;
        ticket_mutation_json(ticket, "ticket.updated", expected_root, changes)
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, ticket_id, expected_root=None, passphrase=None))]
pub(crate) fn tickets_delete_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    ticket_id: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_write(path, workspace, passphrase, |loom, ns| {
        let ticket = loom_tickets::delete_ticket(
            loom,
            ns,
            TicketDeleteRequest {
                workspace_id: ticket_workspace_id,
                ticket_id,
                expected_root,
            },
        )
        .map_err(py_err)?;
        sync_ticket_references(loom, ns, &ticket)?;
        ticket_mutation_json(
            ticket,
            "ticket.deleted",
            expected_root,
            vec![MutationChange::ResourceDeleted],
        )
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, ticket_id, passphrase=None))]
pub(crate) fn tickets_comments_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    ticket_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_tickets::list_ticket_comments(
            loom,
            ns,
            ticket_workspace_id,
            ticket_id,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, ticket_id, comment_id, comment_type, body, expected_root=None, passphrase=None))]
pub(crate) fn tickets_comment_add_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    ticket_id: &str,
    comment_id: Option<&str>,
    comment_type: Option<&str>,
    body: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut changes = vec![MutationChange::field_set(
        "comment_type",
        comment_type.unwrap_or(loom_tickets::TICKET_DEFAULT_COMMENT_TYPE),
    )];
    if let Some(comment_id) = comment_id {
        changes.push(MutationChange::field_set("comment_id", comment_id));
    }
    tickets_write(path, workspace, passphrase, |loom, ns| {
        let ticket = loom_tickets::add_ticket_comment(
            loom,
            ns,
            TicketCommentRequest {
                workspace_id: ticket_workspace_id,
                ticket_id,
                comment_id,
                comment_type,
                body,
                expected_root,
            },
        )
        .map_err(py_err)?;
        sync_ticket_references(loom, ns, &ticket)?;
        ticket_mutation_json(ticket, "ticket.comment_added", expected_root, changes)
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, ticket_id, comment_id, comment_type=None, body=None, expected_root=None, passphrase=None))]
pub(crate) fn tickets_comment_update_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    ticket_id: &str,
    comment_id: &str,
    comment_type: Option<&str>,
    body: Option<&str>,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut changes = vec![MutationChange::field_set("comment_id", comment_id)];
    if let Some(comment_type) = comment_type {
        changes.push(MutationChange::field_set("comment_type", comment_type));
    }
    if body.is_some() {
        changes.push(MutationChange::field_set("body", "updated"));
    }
    tickets_write(path, workspace, passphrase, |loom, ns| {
        let ticket = loom_tickets::update_ticket_comment(
            loom,
            ns,
            TicketCommentUpdateRequest {
                workspace_id: ticket_workspace_id,
                ticket_id,
                comment_id,
                comment_type,
                body,
                expected_root,
            },
        )
        .map_err(py_err)?;
        sync_ticket_references(loom, ns, &ticket)?;
        ticket_mutation_json(ticket, "ticket.comment_updated", expected_root, changes)
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, ticket_id, comment_id, expected_root=None, passphrase=None))]
pub(crate) fn tickets_comment_delete_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    ticket_id: &str,
    comment_id: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_write(path, workspace, passphrase, |loom, ns| {
        let ticket = loom_tickets::delete_ticket_comment(
            loom,
            ns,
            TicketCommentDeleteRequest {
                workspace_id: ticket_workspace_id,
                ticket_id,
                comment_id,
                expected_root,
            },
        )
        .map_err(py_err)?;
        sync_ticket_references(loom, ns, &ticket)?;
        ticket_mutation_json(
            ticket,
            "ticket.comment_deleted",
            expected_root,
            vec![MutationChange::field_deleted(
                "comment",
                Some(comment_id.to_string()),
            )],
        )
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, ticket_id, relation_id, kind, target_id, expected_root=None, passphrase=None))]
pub(crate) fn tickets_relation_set_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    ticket_id: &str,
    relation_id: Option<&str>,
    kind: &str,
    target_id: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let kind = TicketRelationKind::parse(kind).map_err(py_err)?;
    tickets_write(path, workspace, passphrase, |loom, ns| {
        let relation = loom_tickets::add_ticket_relation(
            loom,
            ns,
            TicketRelationRequest {
                workspace_id: ticket_workspace_id,
                ticket_id,
                relation_id,
                kind,
                target_id,
                expected_root,
            },
        )
        .map_err(py_err)?;
        let change = MutationChange::relation_set(
            relation.relation_id.clone(),
            relation.kind.clone(),
            relation.target_id.clone(),
        );
        relation_mutation_json(relation, "ticket.relation_set", expected_root, vec![change])
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, ticket_id, relation_id, expected_root=None, passphrase=None))]
pub(crate) fn tickets_relation_remove_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    ticket_id: &str,
    relation_id: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_write(path, workspace, passphrase, |loom, ns| {
        let relation = loom_tickets::remove_ticket_relation(
            loom,
            ns,
            TicketRelationRemoveRequest {
                workspace_id: ticket_workspace_id,
                ticket_id,
                relation_id,
                expected_root,
            },
        )
        .map_err(py_err)?;
        let change = MutationChange::relation_removed(
            relation.relation_id.clone(),
            relation.kind.clone(),
            relation.target_id.clone(),
        );
        relation_mutation_json(
            relation,
            "ticket.relation_removed",
            expected_root,
            vec![change],
        )
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, ticket_id, passphrase=None))]
pub(crate) fn tickets_relation_list_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    ticket_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_tickets::list_ticket_relations(
            loom,
            ns,
            ticket_workspace_id,
            ticket_id,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, ticket_id, projection=None, passphrase=None))]
pub(crate) fn tickets_get_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    ticket_id: &str,
    projection: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_read(path, workspace, passphrase, |loom, ns| {
        let projection = loom_tickets::parse_ticket_projection(projection).map_err(py_err)?;
        to_json(loom_tickets::get_ticket_with_projection(
            loom,
            ns,
            ticket_workspace_id,
            ticket_id,
            projection,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, request_json, passphrase=None))]
pub(crate) fn tickets_board_create_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    request_json: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_write(path, workspace, passphrase, |loom, ns| {
        let request: PythonBoardCreateRequest =
            serde_json::from_str(request_json).map_err(|error| {
                PyRuntimeError::new_err(format!("board create request json: {error}"))
            })?;
        let columns = board_columns(request.columns)?;
        let swimlanes = board_swimlanes(request.swimlanes)?;
        to_json(loom_tickets::create_board(
            loom,
            ns,
            BoardCreateRequest {
                workspace_id: ticket_workspace_id,
                board_id: &request.board_id,
                board_key: &request.board_key,
                name: &request.name,
                description: &request.description,
                project_id: &request.project_id,
                scope: board_scope(&request.scope, &request.project_id)?,
                mode: BoardMode::parse(&request.mode).map_err(py_err)?,
                columns: &columns,
                swimlanes: &swimlanes,
                card_display_fields: &request.card_display_fields,
                owner_principal: request.owner_principal.as_deref(),
                coordinator_principal: request.coordinator_principal.as_deref(),
                updated_by: &request.updated_by,
                expected_root: request.expected_root.as_deref(),
            },
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, board_id, passphrase=None))]
pub(crate) fn tickets_board_get_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    board_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_tickets::get_board(
            loom,
            ns,
            ticket_workspace_id,
            board_id,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, include_deleted=false, passphrase=None))]
pub(crate) fn tickets_board_list_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    include_deleted: bool,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_tickets::list_boards(
            loom,
            ns,
            ticket_workspace_id,
            include_deleted,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, board_id, request_json, passphrase=None))]
pub(crate) fn tickets_board_update_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    board_id: &str,
    request_json: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_write(path, workspace, passphrase, |loom, ns| {
        let request: PythonBoardUpdateRequest =
            serde_json::from_str(request_json).map_err(|error| {
                PyRuntimeError::new_err(format!("board update request json: {error}"))
            })?;
        let board_status = request
            .board_status
            .as_deref()
            .map(BoardStatus::parse)
            .transpose()
            .map_err(py_err)?;
        to_json(loom_tickets::update_board(
            loom,
            ns,
            BoardUpdateRequest {
                workspace_id: ticket_workspace_id,
                board_id,
                board_key: request.board_key.as_deref(),
                name: request.name.as_deref(),
                description: request.description.as_deref(),
                scope: None,
                owner_principal: None,
                coordinator_principal: None,
                card_display_fields: request.card_display_fields.as_deref(),
                board_status,
                updated_by: &request.updated_by,
                expected_root: request.expected_root.as_deref(),
            },
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, board_id, expected_root=None, passphrase=None))]
pub(crate) fn tickets_board_delete_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    board_id: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_tickets::update_board(
            loom,
            ns,
            BoardUpdateRequest {
                workspace_id: ticket_workspace_id,
                board_id,
                board_key: None,
                name: None,
                description: None,
                scope: None,
                owner_principal: None,
                coordinator_principal: None,
                card_display_fields: None,
                board_status: Some(BoardStatus::Deleted),
                updated_by: "python",
                expected_root,
            },
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, board_id, request_json, passphrase=None))]
pub(crate) fn tickets_board_configure_columns_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    board_id: &str,
    request_json: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_write(path, workspace, passphrase, |loom, ns| {
        let request: PythonBoardConfigureColumnsRequest = serde_json::from_str(request_json)
            .map_err(|error| {
                PyRuntimeError::new_err(format!("board columns request json: {error}"))
            })?;
        let columns = board_columns(request.columns)?;
        let swimlanes = board_swimlanes(request.swimlanes)?;
        let mode = request
            .mode
            .as_deref()
            .map(BoardMode::parse)
            .transpose()
            .map_err(py_err)?;
        to_json(loom_tickets::configure_board_columns(
            loom,
            ns,
            BoardColumnConfigureRequest {
                workspace_id: ticket_workspace_id,
                board_id,
                mode,
                columns: &columns,
                swimlanes: &swimlanes,
                updated_by: &request.updated_by,
                expected_root: request.expected_root.as_deref(),
            },
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, board_id, request_json, passphrase=None))]
pub(crate) fn tickets_board_move_card_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    board_id: &str,
    request_json: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_write(path, workspace, passphrase, |loom, ns| {
        let request: PythonBoardMoveCardRequest =
            serde_json::from_str(request_json).map_err(|error| {
                PyRuntimeError::new_err(format!("board move card request json: {error}"))
            })?;
        to_json(loom_tickets::move_board_card(
            loom,
            ns,
            BoardCardMoveRequest {
                workspace_id: ticket_workspace_id,
                board_id,
                ticket_id: &request.ticket_id,
                column_id: &request.column_id,
                rank_token: &request.rank_token,
                swimlane_id: request.swimlane_id.as_deref(),
                updated_by: &request.updated_by,
                expected_root: request.expected_root.as_deref(),
            },
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, projection=None, passphrase=None))]
pub(crate) fn tickets_list_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    projection: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_read(path, workspace, passphrase, |loom, ns| {
        let projection = loom_tickets::parse_ticket_projection(projection).map_err(py_err)?;
        to_json(loom_tickets::list_tickets_with_projection(
            loom,
            ns,
            ticket_workspace_id,
            projection,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ticket_workspace_id, ticket_id=None, passphrase=None))]
pub(crate) fn tickets_history_json(
    path: &str,
    workspace: &str,
    ticket_workspace_id: &str,
    ticket_id: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    tickets_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_tickets::history(
            loom,
            ns,
            ticket_workspace_id,
            ticket_id,
        ))
    })
}
