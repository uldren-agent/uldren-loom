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

fn to_json<T: Serialize>(value: loom_core::error::Result<T>) -> napi::Result<String> {
    let value = value.map_err(reason)?;
    serde_json::to_string(&value).map_err(|error| napi::Error::from_reason(error.to_string()))
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
) -> napi::Result<String> {
    let receipt = MutationReceipt::new(operation, "ticket", ticket.primary_key.clone())
        .operation_id(ticket.operation_id.clone())
        .roots(
            root_before.map(str::to_string),
            Some(ticket.profile_root.clone()),
        )
        .changes(changes);
    serde_json::to_string(&MutationEnvelope::new(ticket, receipt))
        .map_err(|error| napi::Error::from_reason(error.to_string()))
}

fn relation_mutation_json(
    relation: TicketRelationSummary,
    operation: &str,
    root_before: Option<&str>,
    changes: Vec<MutationChange>,
) -> napi::Result<String> {
    let receipt = MutationReceipt::new(operation, "ticket_relation", relation.relation_id.clone())
        .operation_id(Some(relation.operation_id.clone()))
        .roots(
            root_before.map(str::to_string),
            Some(relation.profile_root.clone()),
        )
        .changes(changes);
    serde_json::to_string(&MutationEnvelope::new(relation, receipt))
        .map_err(|error| napi::Error::from_reason(error.to_string()))
}

fn parse_json(value: &str, what: &str) -> napi::Result<JsonValue> {
    serde_json::from_str(value)
        .map_err(|error| napi::Error::from_reason(format!("{what}: {error}")))
}

fn parse_string_list(value: &str, what: &str) -> napi::Result<Vec<String>> {
    serde_json::from_str(value)
        .map_err(|error| napi::Error::from_reason(format!("{what}: {error}")))
}

fn parse_optional_json_list<T: DeserializeOwned>(
    value: Option<&str>,
    what: &str,
) -> napi::Result<Vec<T>> {
    value
        .map(|value| serde_json::from_str(value))
        .transpose()
        .map_err(|error| napi::Error::from_reason(format!("{what}: {error}")))
        .map(|value| value.unwrap_or_default())
}

#[derive(Deserialize)]
struct JsTicketUpdateComment {
    #[serde(default)]
    comment_id: Option<String>,
    #[serde(default)]
    comment_type: Option<String>,
    body: String,
}

#[derive(Deserialize)]
struct JsTicketUpdateRelationSet {
    #[serde(default)]
    relation_id: Option<String>,
    kind: String,
    target_id: String,
}

#[derive(Deserialize)]
struct JsTicketUpdateRelationRemove {
    relation_id: String,
}

fn parse_projection_list(
    value: &str,
    what: &str,
) -> napi::Result<Vec<loom_tickets::TicketProjectionProfile>> {
    parse_string_list(value, what)?
        .into_iter()
        .map(|profile| loom_tickets::TicketProjectionProfile::parse(&profile).map_err(reason))
        .collect()
}

fn parse_cardinality(value: &str) -> napi::Result<loom_tickets::TicketFieldCardinality> {
    match value {
        "single" => Ok(loom_tickets::TicketFieldCardinality::Single),
        "optional" => Ok(loom_tickets::TicketFieldCardinality::Optional),
        "list" => Ok(loom_tickets::TicketFieldCardinality::List {
            min_items: 0,
            max_items: None,
        }),
        other => Err(napi::Error::from_reason(format!(
            "ticket field cardinality must be single, optional, or list: {other}"
        ))),
    }
}

#[derive(Deserialize)]
struct NodeBoardColumn {
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
struct NodeBoardSwimlane {
    swimlane_id: String,
    name: String,
    #[serde(default)]
    predicate: Option<String>,
    #[serde(default)]
    rank: u64,
}

#[derive(Deserialize)]
struct NodeBoardCreateRequest {
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
    columns: Vec<NodeBoardColumn>,
    #[serde(default)]
    swimlanes: Vec<NodeBoardSwimlane>,
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
struct NodeBoardUpdateRequest {
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
struct NodeBoardConfigureColumnsRequest {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    columns: Vec<NodeBoardColumn>,
    #[serde(default)]
    swimlanes: Vec<NodeBoardSwimlane>,
    #[serde(default = "default_updated_by")]
    updated_by: String,
    #[serde(default)]
    expected_root: Option<String>,
}

#[derive(Deserialize)]
struct NodeBoardMoveCardRequest {
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
    "node".to_string()
}

fn board_columns(columns: Vec<NodeBoardColumn>) -> napi::Result<Vec<BoardColumn>> {
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
            .map_err(reason)
        })
        .collect()
}

fn board_swimlanes(swimlanes: Vec<NodeBoardSwimlane>) -> napi::Result<Vec<BoardSwimlane>> {
    swimlanes
        .into_iter()
        .map(|swimlane| {
            BoardSwimlane::new(
                swimlane.swimlane_id,
                swimlane.name,
                swimlane.predicate,
                swimlane.rank,
            )
            .map_err(reason)
        })
        .collect()
}

fn board_scope(kind: &str, project_id: &str) -> napi::Result<BoardScope> {
    match kind {
        "project" => Ok(BoardScope::project(project_id.to_string())),
        "manual_set" => Ok(BoardScope::ManualSet),
        _ => Err(napi::Error::from_reason(
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
    loom_path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> napi::Result<T>,
) -> napi::Result<T> {
    let loom = open_loom_read_unlocked(loom_path, key_spec(passphrase).as_ref()).map_err(reason)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    f(&loom, workspace_id)
}

fn tickets_write<T>(
    loom_path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    f: impl FnOnce(&mut Loom<FileStore>, WorkspaceId) -> napi::Result<T>,
) -> napi::Result<T> {
    let mut loom = open_loom_unlocked(loom_path, key_spec(passphrase).as_ref()).map_err(reason)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    let out = f(&mut loom, workspace_id)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(out)
}

fn sync_ticket_references(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    ticket: &TicketSummary,
) -> napi::Result<()> {
    loom_tickets::update_ticket_field_references(
        loom,
        workspace,
        &ticket.workspace_id,
        &ticket.ticket_id,
        &ticket.fields,
    )
    .map_err(reason)?;
    if let Some(operation_id) = ticket.operation_id.as_deref() {
        loom_tickets::enqueue_ticket_reference_candidates(
            loom,
            workspace,
            loom_tickets::TicketReferenceCandidateRequest {
                workspace_id: &ticket.workspace_id,
                ticket_id: &ticket.ticket_id,
                operation_id,
                source_root: Digest::parse(&ticket.profile_root).map_err(reason)?,
                fields: &ticket.fields,
                now_ms: now_ms(),
            },
        )
        .map_err(reason)?;
    }
    Ok(())
}

#[napi]
pub fn tickets_project_create_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    project_id: String,
    key_prefix: String,
    name: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_tickets::create_project(
            loom,
            ns,
            &ticket_workspace_id,
            &project_id,
            &key_prefix,
            &name,
            expected_root.as_deref(),
        ))
    })
}

#[napi]
pub fn tickets_project_rekey_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    project_id: String,
    key_prefix: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_tickets::rekey_project(
            loom,
            ns,
            &ticket_workspace_id,
            &project_id,
            &key_prefix,
            expected_root.as_deref(),
        ))
    })
}

#[napi]
pub fn tickets_project_settings_get_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    project_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_tickets::get_project(
            loom,
            ns,
            &ticket_workspace_id,
            &project_id,
        ))
    })
}

#[napi]
pub fn tickets_project_settings_set_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    project_id: String,
    default_projection: Option<String>,
    enable_projections_json: String,
    disable_projections_json: String,
    actor_enforcement: Option<String>,
    project_owner_principal: Option<String>,
    clear_project_owner_principal: bool,
    acceptance_authorities_json: Option<String>,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let default_projection = default_projection
        .as_deref()
        .map(loom_tickets::TicketProjectionProfile::parse)
        .transpose()
        .map_err(reason)?;
    let enable_projections =
        parse_projection_list(&enable_projections_json, "ticket enable projections json")?;
    let disable_projections =
        parse_projection_list(&disable_projections_json, "ticket disable projections json")?;
    let actor_enforcement = actor_enforcement
        .as_deref()
        .map(TicketLifecycleAuthorizationPolicy::parse)
        .transpose()
        .map_err(reason)?;
    let acceptance_authorities = acceptance_authorities_json
        .as_deref()
        .map(|value| parse_string_list(value, "ticket acceptance authorities json"))
        .transpose()?;
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_tickets::set_project_settings(
            loom,
            ns,
            loom_tickets::TicketProjectSettingsRequest {
                workspace_id: &ticket_workspace_id,
                project_id: &project_id,
                default_projection,
                enable_projections: &enable_projections,
                disable_projections: &disable_projections,
                actor_enforcement,
                project_owner_principal: project_owner_principal.as_deref(),
                clear_project_owner_principal,
                acceptance_authorities: acceptance_authorities.as_deref(),
                expected_root: expected_root.as_deref(),
            },
        ))
    })
}

#[napi]
pub fn tickets_fields_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    project_id: Option<String>,
    projection: Option<String>,
    operation: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let projection =
            loom_tickets::parse_ticket_projection(projection.as_deref()).map_err(reason)?;
        match project_id.as_deref() {
            Some(project_id) => to_json(loom_tickets::ticket_field_catalog_for_project(
                loom,
                ns,
                &ticket_workspace_id,
                project_id,
                projection,
                operation.as_deref(),
            )),
            None => to_json(loom_tickets::ticket_field_catalog(
                projection,
                operation.as_deref(),
            )),
        }
    })
}

#[napi]
pub fn tickets_field_put_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    project_id: String,
    field_id: String,
    key: String,
    name: String,
    description: Option<String>,
    field_type: String,
    option_set: Option<String>,
    max_length: Option<u32>,
    required: bool,
    searchable: bool,
    orderable: bool,
    cardinality: String,
    applicable_type_ids_json: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let cardinality = parse_cardinality(&cardinality)?;
    let applicable_type_ids =
        parse_string_list(&applicable_type_ids_json, "ticket applicable type ids json")?;
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_tickets::put_ticket_field_definition(
            loom,
            ns,
            TicketFieldDefinitionWriteRequest {
                workspace_id: &ticket_workspace_id,
                project_id: &project_id,
                field_id: &field_id,
                key: &key,
                name: &name,
                description: description.as_deref(),
                field_type: &field_type,
                option_set: option_set.as_deref(),
                max_length,
                required,
                searchable,
                orderable,
                cardinality,
                applicable_type_ids: &applicable_type_ids,
                expected_root: expected_root.as_deref(),
            },
        ))
    })
}

#[napi]
pub fn tickets_field_retire_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    project_id: String,
    field_id: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_tickets::retire_ticket_field_definition(
            loom,
            ns,
            TicketFieldDefinitionRetireRequest {
                workspace_id: &ticket_workspace_id,
                project_id: &project_id,
                field_id: &field_id,
                expected_root: expected_root.as_deref(),
            },
        ))
    })
}

#[napi]
pub fn tickets_create_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    project_id: String,
    ticket_type: String,
    external_source: Option<String>,
    external_id: Option<String>,
    fields_json: String,
    policy_labels_json: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let fields = parse_json(&fields_json, "ticket fields json")?;
    let policy_labels = parse_string_list(&policy_labels_json, "ticket policy labels json")?;
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let ticket = loom_tickets::create_ticket(
            loom,
            ns,
            TicketCreateRequest {
                workspace_id: &ticket_workspace_id,
                project_id: &project_id,
                ticket_type: &ticket_type,
                external_source: external_source.as_deref(),
                external_id: external_id.as_deref(),
                fields: &fields,
                policy_labels: &policy_labels,
                expected_root: expected_root.as_deref(),
            },
        )
        .map_err(reason)?;
        sync_ticket_references(loom, ns, &ticket)?;
        ticket_mutation_json(
            ticket,
            "ticket.created",
            expected_root.as_deref(),
            vec![MutationChange::ResourceCreated],
        )
    })
}

#[napi]
pub fn tickets_update_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    ticket_id: String,
    set_fields_json: Option<String>,
    delete_fields_json: String,
    action: Option<String>,
    target_status: Option<String>,
    observed_source_status: Option<String>,
    observed_workflow_version: Option<String>,
    assignee: Option<String>,
    comment_id: Option<String>,
    comment_type: Option<String>,
    comment_body: Option<String>,
    expected_root: Option<String>,
    passphrase: Option<String>,
    comments_json: Option<String>,
    relation_sets_json: Option<String>,
    relation_removes_json: Option<String>,
) -> napi::Result<String> {
    if comment_body.is_none() && (comment_id.is_some() || comment_type.is_some()) {
        return Err(reason(loom_core::LoomError::invalid(
            "ticket update comment id and type require comment body",
        )));
    }
    let set_fields = set_fields_json
        .as_deref()
        .map(|value| parse_json(value, "ticket set fields json"))
        .transpose()?;
    let delete_fields = parse_string_list(&delete_fields_json, "ticket delete fields json")?;
    let action = action
        .as_deref()
        .map(TicketLifecycleAction::parse)
        .transpose()
        .map_err(reason)?;
    let comments_input = parse_optional_json_list::<JsTicketUpdateComment>(
        comments_json.as_deref(),
        "ticket comments json",
    )?;
    let relation_sets_input = parse_optional_json_list::<JsTicketUpdateRelationSet>(
        relation_sets_json.as_deref(),
        "ticket relation sets json",
    )?;
    let relation_removes_input = parse_optional_json_list::<JsTicketUpdateRelationRemove>(
        relation_removes_json.as_deref(),
        "ticket relation removes json",
    )?;
    let relation_kinds = relation_sets_input
        .iter()
        .map(|relation| TicketRelationKind::parse(&relation.kind).map_err(reason))
        .collect::<napi::Result<Vec<_>>>()?;
    let changes = ticket_update_changes(
        set_fields.as_ref(),
        &delete_fields,
        action.is_some(),
        target_status.as_deref(),
        observed_source_status.as_deref(),
        assignee.as_deref(),
        comment_type
            .as_ref()
            .map(|value| Some(value.clone()))
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
    let comment = comment_body
        .as_ref()
        .map(|body| loom_tickets::TicketUpdateCommentRequest {
            comment_id: comment_id.as_deref(),
            comment_type: comment_type.as_deref(),
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
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let ticket = loom_tickets::update_ticket(
            loom,
            ns,
            TicketUpdateRequest {
                workspace_id: &ticket_workspace_id,
                ticket_id: &ticket_id,
                set_fields: set_fields.as_ref(),
                delete_fields: &delete_fields,
                action,
                target_status: target_status.as_deref(),
                observed_source_status: observed_source_status.as_deref(),
                observed_workflow_version: observed_workflow_version.as_deref(),
                assignee: assignee.as_deref(),
                expected_root: expected_root.as_deref(),
                comment,
                comments: &comments,
                relation_sets: &relation_sets,
                relation_removes: &relation_removes,
            },
        )
        .map_err(reason)?;
        sync_ticket_references(loom, ns, &ticket)?;
        ticket_mutation_json(ticket, "ticket.updated", expected_root.as_deref(), changes)
    })
}

#[napi]
pub fn tickets_delete_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    ticket_id: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let ticket = loom_tickets::delete_ticket(
            loom,
            ns,
            TicketDeleteRequest {
                workspace_id: &ticket_workspace_id,
                ticket_id: &ticket_id,
                expected_root: expected_root.as_deref(),
            },
        )
        .map_err(reason)?;
        sync_ticket_references(loom, ns, &ticket)?;
        ticket_mutation_json(
            ticket,
            "ticket.deleted",
            expected_root.as_deref(),
            vec![MutationChange::ResourceDeleted],
        )
    })
}

#[napi]
pub fn tickets_comments_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    ticket_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_tickets::list_ticket_comments(
            loom,
            ns,
            &ticket_workspace_id,
            &ticket_id,
        ))
    })
}

#[napi]
pub fn tickets_comment_add_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    ticket_id: String,
    comment_id: Option<String>,
    comment_type: Option<String>,
    body: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut changes = vec![MutationChange::field_set(
        "comment_type",
        comment_type
            .as_deref()
            .unwrap_or(loom_tickets::TICKET_DEFAULT_COMMENT_TYPE),
    )];
    if let Some(comment_id) = comment_id.as_deref() {
        changes.push(MutationChange::field_set("comment_id", comment_id));
    }
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let ticket = loom_tickets::add_ticket_comment(
            loom,
            ns,
            TicketCommentRequest {
                workspace_id: &ticket_workspace_id,
                ticket_id: &ticket_id,
                comment_id: comment_id.as_deref(),
                comment_type: comment_type.as_deref(),
                body: &body,
                expected_root: expected_root.as_deref(),
            },
        )
        .map_err(reason)?;
        sync_ticket_references(loom, ns, &ticket)?;
        ticket_mutation_json(
            ticket,
            "ticket.comment_added",
            expected_root.as_deref(),
            changes,
        )
    })
}

#[napi]
pub fn tickets_comment_update_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    ticket_id: String,
    comment_id: String,
    comment_type: Option<String>,
    body: Option<String>,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut changes = vec![MutationChange::field_set("comment_id", comment_id.as_str())];
    if let Some(comment_type) = comment_type.as_deref() {
        changes.push(MutationChange::field_set("comment_type", comment_type));
    }
    if body.is_some() {
        changes.push(MutationChange::field_set("body", "updated"));
    }
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let ticket = loom_tickets::update_ticket_comment(
            loom,
            ns,
            TicketCommentUpdateRequest {
                workspace_id: &ticket_workspace_id,
                ticket_id: &ticket_id,
                comment_id: &comment_id,
                comment_type: comment_type.as_deref(),
                body: body.as_deref(),
                expected_root: expected_root.as_deref(),
            },
        )
        .map_err(reason)?;
        sync_ticket_references(loom, ns, &ticket)?;
        ticket_mutation_json(
            ticket,
            "ticket.comment_updated",
            expected_root.as_deref(),
            changes,
        )
    })
}

#[napi]
pub fn tickets_comment_delete_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    ticket_id: String,
    comment_id: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let ticket = loom_tickets::delete_ticket_comment(
            loom,
            ns,
            TicketCommentDeleteRequest {
                workspace_id: &ticket_workspace_id,
                ticket_id: &ticket_id,
                comment_id: &comment_id,
                expected_root: expected_root.as_deref(),
            },
        )
        .map_err(reason)?;
        sync_ticket_references(loom, ns, &ticket)?;
        ticket_mutation_json(
            ticket,
            "ticket.comment_deleted",
            expected_root.as_deref(),
            vec![MutationChange::field_deleted("comment", Some(comment_id))],
        )
    })
}

#[napi]
pub fn tickets_relation_set_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    ticket_id: String,
    relation_id: Option<String>,
    kind: String,
    target_id: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let kind = TicketRelationKind::parse(&kind).map_err(reason)?;
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let relation = loom_tickets::add_ticket_relation(
            loom,
            ns,
            TicketRelationRequest {
                workspace_id: &ticket_workspace_id,
                ticket_id: &ticket_id,
                relation_id: relation_id.as_deref(),
                kind,
                target_id: &target_id,
                expected_root: expected_root.as_deref(),
            },
        )
        .map_err(reason)?;
        let change = MutationChange::relation_set(
            relation.relation_id.clone(),
            relation.kind.clone(),
            relation.target_id.clone(),
        );
        relation_mutation_json(
            relation,
            "ticket.relation_set",
            expected_root.as_deref(),
            vec![change],
        )
    })
}

#[napi]
pub fn tickets_relation_remove_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    ticket_id: String,
    relation_id: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let relation = loom_tickets::remove_ticket_relation(
            loom,
            ns,
            TicketRelationRemoveRequest {
                workspace_id: &ticket_workspace_id,
                ticket_id: &ticket_id,
                relation_id: &relation_id,
                expected_root: expected_root.as_deref(),
            },
        )
        .map_err(reason)?;
        let change = MutationChange::relation_removed(
            relation.relation_id.clone(),
            relation.kind.clone(),
            relation.target_id.clone(),
        );
        relation_mutation_json(
            relation,
            "ticket.relation_removed",
            expected_root.as_deref(),
            vec![change],
        )
    })
}

#[napi]
pub fn tickets_relation_list_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    ticket_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_tickets::list_ticket_relations(
            loom,
            ns,
            &ticket_workspace_id,
            &ticket_id,
        ))
    })
}

#[napi]
pub fn tickets_get_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    ticket_id: String,
    projection: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let projection =
            loom_tickets::parse_ticket_projection(projection.as_deref()).map_err(reason)?;
        to_json(loom_tickets::get_ticket_with_projection(
            loom,
            ns,
            &ticket_workspace_id,
            &ticket_id,
            projection,
        ))
    })
}

#[napi]
pub fn tickets_board_create_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    request_json: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let request: NodeBoardCreateRequest =
            serde_json::from_str(&request_json).map_err(|error| {
                napi::Error::from_reason(format!("board create request json: {error}"))
            })?;
        let columns = board_columns(request.columns)?;
        let swimlanes = board_swimlanes(request.swimlanes)?;
        to_json(loom_tickets::create_board(
            loom,
            ns,
            BoardCreateRequest {
                workspace_id: &ticket_workspace_id,
                board_id: &request.board_id,
                board_key: &request.board_key,
                name: &request.name,
                description: &request.description,
                project_id: &request.project_id,
                scope: board_scope(&request.scope, &request.project_id)?,
                mode: BoardMode::parse(&request.mode).map_err(reason)?,
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

#[napi]
pub fn tickets_board_get_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    board_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_tickets::get_board(
            loom,
            ns,
            &ticket_workspace_id,
            &board_id,
        ))
    })
}

#[napi]
pub fn tickets_board_list_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    include_deleted: bool,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_tickets::list_boards(
            loom,
            ns,
            &ticket_workspace_id,
            include_deleted,
        ))
    })
}

#[napi]
pub fn tickets_board_update_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    board_id: String,
    request_json: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let request: NodeBoardUpdateRequest =
            serde_json::from_str(&request_json).map_err(|error| {
                napi::Error::from_reason(format!("board update request json: {error}"))
            })?;
        let board_status = request
            .board_status
            .as_deref()
            .map(BoardStatus::parse)
            .transpose()
            .map_err(reason)?;
        to_json(loom_tickets::update_board(
            loom,
            ns,
            BoardUpdateRequest {
                workspace_id: &ticket_workspace_id,
                board_id: &board_id,
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

#[napi]
pub fn tickets_board_delete_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    board_id: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_tickets::update_board(
            loom,
            ns,
            BoardUpdateRequest {
                workspace_id: &ticket_workspace_id,
                board_id: &board_id,
                board_key: None,
                name: None,
                description: None,
                scope: None,
                owner_principal: None,
                coordinator_principal: None,
                card_display_fields: None,
                board_status: Some(BoardStatus::Deleted),
                updated_by: "node",
                expected_root: expected_root.as_deref(),
            },
        ))
    })
}

#[napi]
pub fn tickets_board_configure_columns_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    board_id: String,
    request_json: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let request: NodeBoardConfigureColumnsRequest = serde_json::from_str(&request_json)
            .map_err(|error| {
                napi::Error::from_reason(format!("board columns request json: {error}"))
            })?;
        let columns = board_columns(request.columns)?;
        let swimlanes = board_swimlanes(request.swimlanes)?;
        let mode = request
            .mode
            .as_deref()
            .map(BoardMode::parse)
            .transpose()
            .map_err(reason)?;
        to_json(loom_tickets::configure_board_columns(
            loom,
            ns,
            BoardColumnConfigureRequest {
                workspace_id: &ticket_workspace_id,
                board_id: &board_id,
                mode,
                columns: &columns,
                swimlanes: &swimlanes,
                updated_by: &request.updated_by,
                expected_root: request.expected_root.as_deref(),
            },
        ))
    })
}

#[napi]
pub fn tickets_board_move_card_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    board_id: String,
    request_json: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let request: NodeBoardMoveCardRequest =
            serde_json::from_str(&request_json).map_err(|error| {
                napi::Error::from_reason(format!("board move card request json: {error}"))
            })?;
        to_json(loom_tickets::move_board_card(
            loom,
            ns,
            BoardCardMoveRequest {
                workspace_id: &ticket_workspace_id,
                board_id: &board_id,
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

#[napi]
pub fn tickets_list_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    projection: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        let projection =
            loom_tickets::parse_ticket_projection(projection.as_deref()).map_err(reason)?;
        to_json(loom_tickets::list_tickets_with_projection(
            loom,
            ns,
            &ticket_workspace_id,
            projection,
        ))
    })
}

#[napi]
pub fn tickets_history_json(
    loom_path: String,
    workspace: String,
    ticket_workspace_id: String,
    ticket_id: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    tickets_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_tickets::history(
            loom,
            ns,
            &ticket_workspace_id,
            ticket_id.as_deref(),
        ))
    })
}
