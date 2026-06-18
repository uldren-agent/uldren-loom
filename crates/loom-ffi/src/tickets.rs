//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

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

unsafe fn optional_str_arg<'a>(value: *const c_char, what: &str) -> LoomResult<Option<&'a str>> {
    if value.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|_| LoomError::invalid(format!("{what}: invalid UTF-8")))?;
    Ok((!value.is_empty()).then_some(value))
}

fn json_string<T: Serialize>(value: &T) -> LoomResult<String> {
    serde_json::to_string(value).map_err(|err| LoomError::invalid(err.to_string()))
}

fn parse_json(value: &str, what: &str) -> LoomResult<JsonValue> {
    serde_json::from_str(value).map_err(|err| LoomError::invalid(format!("{what}: {err}")))
}

fn parse_string_list(value: &str, what: &str) -> LoomResult<Vec<String>> {
    serde_json::from_str(value).map_err(|err| LoomError::invalid(format!("{what}: {err}")))
}

fn parse_optional_json_list<T: DeserializeOwned>(
    value: Option<&str>,
    what: &str,
) -> LoomResult<Vec<T>> {
    value
        .map(serde_json::from_str)
        .transpose()
        .map_err(|err| LoomError::invalid(format!("{what}: {err}")))?
        .map_or_else(|| Ok(Vec::new()), Ok)
}

fn parse_projection(
    value: Option<&str>,
) -> LoomResult<Option<loom_tickets::TicketProjectionProfile>> {
    loom_tickets::parse_ticket_projection(value)
}

fn parse_action(value: Option<&str>) -> LoomResult<Option<TicketLifecycleAction>> {
    value.map(TicketLifecycleAction::parse).transpose()
}

fn parse_cardinality(value: &str) -> LoomResult<loom_tickets::TicketFieldCardinality> {
    match value {
        "single" => Ok(loom_tickets::TicketFieldCardinality::Single),
        "optional" => Ok(loom_tickets::TicketFieldCardinality::Optional),
        "list" => Ok(loom_tickets::TicketFieldCardinality::List {
            min_items: 0,
            max_items: None,
        }),
        other => Err(LoomError::invalid(format!(
            "ticket field cardinality must be single, optional, or list: {other}"
        ))),
    }
}

fn parse_projection_list(
    value: &str,
    what: &str,
) -> LoomResult<Vec<loom_tickets::TicketProjectionProfile>> {
    parse_string_list(value, what)?
        .into_iter()
        .map(|profile| loom_tickets::TicketProjectionProfile::parse(&profile))
        .collect()
}

#[derive(Deserialize)]
struct FfiBoardColumn {
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
struct FfiBoardSwimlane {
    swimlane_id: String,
    name: String,
    #[serde(default)]
    predicate: Option<String>,
    #[serde(default)]
    rank: u64,
}

#[derive(Deserialize)]
struct FfiBoardCreateRequest {
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
    columns: Vec<FfiBoardColumn>,
    #[serde(default)]
    swimlanes: Vec<FfiBoardSwimlane>,
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
struct FfiBoardUpdateRequest {
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
struct FfiTicketUpdateComment {
    #[serde(default)]
    comment_id: Option<String>,
    #[serde(default)]
    comment_type: Option<String>,
    body: String,
}

#[derive(Deserialize)]
struct FfiTicketUpdateRelationSet {
    #[serde(default)]
    relation_id: Option<String>,
    kind: String,
    target_id: String,
}

#[derive(Deserialize)]
struct FfiTicketUpdateRelationRemove {
    relation_id: String,
}

#[derive(Deserialize)]
struct FfiBoardConfigureColumnsRequest {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    columns: Vec<FfiBoardColumn>,
    #[serde(default)]
    swimlanes: Vec<FfiBoardSwimlane>,
    #[serde(default = "default_updated_by")]
    updated_by: String,
    #[serde(default)]
    expected_root: Option<String>,
}

#[derive(Deserialize)]
struct FfiBoardMoveCardRequest {
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
    "ffi".to_string()
}

fn board_columns(columns: Vec<FfiBoardColumn>) -> LoomResult<Vec<BoardColumn>> {
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
        })
        .collect()
}

fn board_swimlanes(swimlanes: Vec<FfiBoardSwimlane>) -> LoomResult<Vec<BoardSwimlane>> {
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

fn board_scope(kind: &str, project_id: &str) -> LoomResult<BoardScope> {
    match kind {
        "project" => Ok(BoardScope::project(project_id.to_string())),
        "manual_set" => Ok(BoardScope::ManualSet),
        _ => Err(LoomError::invalid(
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

fn ticket_read_loom<T>(
    h: &LoomSession,
    workspace: &str,
    f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> LoomResult<T>,
) -> LoomResult<T> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    f(&loom, ns)
}

fn ticket_write_loom<T>(
    h: &LoomSession,
    workspace: &str,
    f: impl FnOnce(&mut Loom<FileStore>, WorkspaceId) -> LoomResult<T>,
) -> LoomResult<T> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let result = f(&mut loom, ns)?;
    save_loom(&mut loom)?;
    Ok(result)
}

fn sync_ticket_references(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    ticket: &TicketSummary,
) -> LoomResult<()> {
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

fn json_result<T: Serialize>(result: LoomResult<T>) -> LoomResult<String> {
    json_string(&result?)
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
) -> LoomResult<String> {
    let receipt = MutationReceipt::new(operation, "ticket", ticket.primary_key.clone())
        .operation_id(ticket.operation_id.clone())
        .roots(
            root_before.map(str::to_string),
            Some(ticket.profile_root.clone()),
        )
        .changes(changes);
    json_string(&MutationEnvelope::new(ticket, receipt))
}

fn relation_mutation_json(
    relation: TicketRelationSummary,
    operation: &str,
    root_before: Option<&str>,
    changes: Vec<MutationChange>,
) -> LoomResult<String> {
    let receipt = MutationReceipt::new(operation, "ticket_relation", relation.relation_id.clone())
        .operation_id(Some(relation.operation_id.clone()))
        .roots(
            root_before.map(str::to_string),
            Some(relation.profile_root.clone()),
        )
        .changes(changes);
    json_string(&MutationEnvelope::new(relation, receipt))
}

macro_rules! out_json {
    ($out:ident, $result:expr) => {
        match $result {
            Ok(s) => unsafe { ok_str($out, &s) },
            Err(e) => fail(e),
        }
    };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_project_create_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    project_id: *const c_char,
    key_prefix: *const c_char,
    name: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_project_create_json");
    let workspace = arg_str!(workspace, "loom_tickets_project_create_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_project_create_json");
    let project_id = arg_str!(project_id, "loom_tickets_project_create_json");
    let key_prefix = arg_str!(key_prefix, "loom_tickets_project_create_json");
    let name = arg_str!(name, "loom_tickets_project_create_json");
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_tickets_project_create_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            json_result(loom_tickets::create_project(
                loom,
                ns,
                ticket_workspace_id,
                project_id,
                key_prefix,
                name,
                expected_root,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_project_rekey_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    project_id: *const c_char,
    key_prefix: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_project_rekey_json");
    let workspace = arg_str!(workspace, "loom_tickets_project_rekey_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_project_rekey_json");
    let project_id = arg_str!(project_id, "loom_tickets_project_rekey_json");
    let key_prefix = arg_str!(key_prefix, "loom_tickets_project_rekey_json");
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_tickets_project_rekey_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            json_result(loom_tickets::rekey_project(
                loom,
                ns,
                ticket_workspace_id,
                project_id,
                key_prefix,
                expected_root,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_project_settings_get_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    project_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_project_settings_get_json");
    let workspace = arg_str!(workspace, "loom_tickets_project_settings_get_json");
    let ticket_workspace_id = arg_str!(
        ticket_workspace_id,
        "loom_tickets_project_settings_get_json"
    );
    let project_id = arg_str!(project_id, "loom_tickets_project_settings_get_json");
    out_json!(
        out,
        ticket_read_loom(h, workspace, |loom, ns| {
            json_result(loom_tickets::get_project(
                loom,
                ns,
                ticket_workspace_id,
                project_id,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_project_settings_set_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    project_id: *const c_char,
    default_projection: *const c_char,
    enable_projections_json: *const c_char,
    disable_projections_json: *const c_char,
    actor_enforcement: *const c_char,
    project_owner_principal: *const c_char,
    clear_project_owner_principal: bool,
    acceptance_authorities_json: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_project_settings_set_json");
    let workspace = arg_str!(workspace, "loom_tickets_project_settings_set_json");
    let ticket_workspace_id = arg_str!(
        ticket_workspace_id,
        "loom_tickets_project_settings_set_json"
    );
    let project_id = arg_str!(project_id, "loom_tickets_project_settings_set_json");
    let default_projection = match unsafe {
        optional_str_arg(default_projection, "loom_tickets_project_settings_set_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let actor_enforcement = match unsafe {
        optional_str_arg(actor_enforcement, "loom_tickets_project_settings_set_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let project_owner_principal = match unsafe {
        optional_str_arg(
            project_owner_principal,
            "loom_tickets_project_settings_set_json",
        )
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let acceptance_authorities_json = match unsafe {
        optional_str_arg(
            acceptance_authorities_json,
            "loom_tickets_project_settings_set_json",
        )
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let expected_root = match unsafe {
        optional_str_arg(expected_root, "loom_tickets_project_settings_set_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let enable_projections = match parse_projection_list(
        arg_str!(
            enable_projections_json,
            "loom_tickets_project_settings_set_json"
        ),
        "ticket enable projections json",
    ) {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let disable_projections = match parse_projection_list(
        arg_str!(
            disable_projections_json,
            "loom_tickets_project_settings_set_json"
        ),
        "ticket disable projections json",
    ) {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let default_projection = match default_projection
        .map(loom_tickets::TicketProjectionProfile::parse)
        .transpose()
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let actor_enforcement = match actor_enforcement
        .map(TicketLifecycleAuthorizationPolicy::parse)
        .transpose()
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let acceptance_authorities = match acceptance_authorities_json {
        Some(value) => match parse_string_list(value, "ticket acceptance authorities json") {
            Ok(value) => Some(value),
            Err(e) => return fail(e),
        },
        None => None,
    };
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            json_result(loom_tickets::set_project_settings(
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
                    acceptance_evidence_enforcement: None,
                    required_acceptance_evidence_keys: None,
                    owner_contract_summary: None,
                    owner_contract_details: None,
                    worker_contract_summary: None,
                    worker_contract_details: None,
                    expected_root,
                },
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_fields_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    project_id: *const c_char,
    projection: *const c_char,
    operation: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_fields_json");
    let workspace = arg_str!(workspace, "loom_tickets_fields_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_fields_json");
    let project_id = match unsafe { optional_str_arg(project_id, "loom_tickets_fields_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let projection = match unsafe { optional_str_arg(projection, "loom_tickets_fields_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let operation = match unsafe { optional_str_arg(operation, "loom_tickets_fields_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        ticket_read_loom(h, workspace, |loom, ns| {
            let projection = parse_projection(projection)?;
            match project_id {
                Some(project_id) => json_result(loom_tickets::ticket_field_catalog_for_project(
                    loom,
                    ns,
                    ticket_workspace_id,
                    project_id,
                    projection,
                    operation,
                )),
                None => json_result(loom_tickets::ticket_field_catalog(projection, operation)),
            }
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_field_put_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    project_id: *const c_char,
    field_id: *const c_char,
    key: *const c_char,
    name: *const c_char,
    description: *const c_char,
    field_type: *const c_char,
    option_set: *const c_char,
    max_length: u32,
    has_max_length: bool,
    required: bool,
    searchable: bool,
    orderable: bool,
    cardinality: *const c_char,
    applicable_type_ids_json: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_field_put_json");
    let workspace = arg_str!(workspace, "loom_tickets_field_put_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_field_put_json");
    let project_id = arg_str!(project_id, "loom_tickets_field_put_json");
    let field_id = arg_str!(field_id, "loom_tickets_field_put_json");
    let key = arg_str!(key, "loom_tickets_field_put_json");
    let name = arg_str!(name, "loom_tickets_field_put_json");
    let description = match unsafe { optional_str_arg(description, "loom_tickets_field_put_json") }
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let field_type = arg_str!(field_type, "loom_tickets_field_put_json");
    let option_set = match unsafe { optional_str_arg(option_set, "loom_tickets_field_put_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let cardinality = match parse_cardinality(arg_str!(cardinality, "loom_tickets_field_put_json"))
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let applicable_type_ids = match parse_string_list(
        arg_str!(applicable_type_ids_json, "loom_tickets_field_put_json"),
        "ticket applicable type ids json",
    ) {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_tickets_field_put_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            json_result(loom_tickets::put_ticket_field_definition(
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
                    max_length: has_max_length.then_some(max_length),
                    required,
                    searchable,
                    orderable,
                    cardinality,
                    applicable_type_ids: &applicable_type_ids,
                    expected_root,
                },
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_field_retire_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    project_id: *const c_char,
    field_id: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_field_retire_json");
    let workspace = arg_str!(workspace, "loom_tickets_field_retire_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_field_retire_json");
    let project_id = arg_str!(project_id, "loom_tickets_field_retire_json");
    let field_id = arg_str!(field_id, "loom_tickets_field_retire_json");
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_tickets_field_retire_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            json_result(loom_tickets::retire_ticket_field_definition(
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
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_create_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    project_id: *const c_char,
    ticket_type: *const c_char,
    external_source: *const c_char,
    external_id: *const c_char,
    fields_json: *const c_char,
    policy_labels_json: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_create_json");
    let workspace = arg_str!(workspace, "loom_tickets_create_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_create_json");
    let project_id = arg_str!(project_id, "loom_tickets_create_json");
    let ticket_type = arg_str!(ticket_type, "loom_tickets_create_json");
    let fields_json = arg_str!(fields_json, "loom_tickets_create_json");
    let policy_labels_json = arg_str!(policy_labels_json, "loom_tickets_create_json");
    let external_source =
        match unsafe { optional_str_arg(external_source, "loom_tickets_create_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let external_id = match unsafe { optional_str_arg(external_id, "loom_tickets_create_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let expected_root = match unsafe { optional_str_arg(expected_root, "loom_tickets_create_json") }
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let fields = match parse_json(fields_json, "ticket fields json") {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let policy_labels = match parse_string_list(policy_labels_json, "ticket policy labels json") {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
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
            )?;
            sync_ticket_references(loom, ns, &ticket)?;
            ticket_mutation_json(
                ticket,
                "ticket.created",
                expected_root,
                vec![MutationChange::ResourceCreated],
            )
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_update_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    ticket_id: *const c_char,
    set_fields_json: *const c_char,
    delete_fields_json: *const c_char,
    action: *const c_char,
    target_status: *const c_char,
    observed_source_status: *const c_char,
    observed_workflow_version: *const c_char,
    assignee: *const c_char,
    comment_id: *const c_char,
    comment_type: *const c_char,
    comment_body: *const c_char,
    expected_root: *const c_char,
    comments_json: *const c_char,
    relation_sets_json: *const c_char,
    relation_removes_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_update_json");
    let workspace = arg_str!(workspace, "loom_tickets_update_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_update_json");
    let ticket_id = arg_str!(ticket_id, "loom_tickets_update_json");
    let set_fields_json =
        match unsafe { optional_str_arg(set_fields_json, "loom_tickets_update_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let delete_fields_json = arg_str!(delete_fields_json, "loom_tickets_update_json");
    let action = match unsafe { optional_str_arg(action, "loom_tickets_update_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let target_status = match unsafe { optional_str_arg(target_status, "loom_tickets_update_json") }
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let observed_source_status =
        match unsafe { optional_str_arg(observed_source_status, "loom_tickets_update_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let observed_workflow_version =
        match unsafe { optional_str_arg(observed_workflow_version, "loom_tickets_update_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let assignee = match unsafe { optional_str_arg(assignee, "loom_tickets_update_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let comment_id = match unsafe { optional_str_arg(comment_id, "loom_tickets_update_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let comment_type = match unsafe { optional_str_arg(comment_type, "loom_tickets_update_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let comment_body = match unsafe { optional_str_arg(comment_body, "loom_tickets_update_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let expected_root = match unsafe { optional_str_arg(expected_root, "loom_tickets_update_json") }
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let comments_json = match unsafe { optional_str_arg(comments_json, "loom_tickets_update_json") }
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let relation_sets_json =
        match unsafe { optional_str_arg(relation_sets_json, "loom_tickets_update_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let relation_removes_json =
        match unsafe { optional_str_arg(relation_removes_json, "loom_tickets_update_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    if comment_body.is_none() && (comment_id.is_some() || comment_type.is_some()) {
        return fail(LoomError::invalid(
            "ticket update comment id and type require comment body",
        ));
    }
    let set_fields = match set_fields_json {
        Some(value) => match parse_json(value, "ticket set fields json") {
            Ok(value) => Some(value),
            Err(e) => return fail(e),
        },
        None => None,
    };
    let delete_fields = match parse_string_list(delete_fields_json, "ticket delete fields json") {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let action_applied = action.is_some();
    let action = match parse_action(action) {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let comments_input = match parse_optional_json_list::<FfiTicketUpdateComment>(
        comments_json,
        "ticket comments json",
    ) {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let relation_sets_input = match parse_optional_json_list::<FfiTicketUpdateRelationSet>(
        relation_sets_json,
        "ticket relation sets json",
    ) {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let relation_removes_input = match parse_optional_json_list::<FfiTicketUpdateRelationRemove>(
        relation_removes_json,
        "ticket relation removes json",
    ) {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let relation_kinds = match relation_sets_input
        .iter()
        .map(|relation| TicketRelationKind::parse(&relation.kind))
        .collect::<LoomResult<Vec<_>>>()
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let changes = ticket_update_changes(
        set_fields.as_ref(),
        &delete_fields,
        action_applied,
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
        evidence: None,
    });
    let comments = comments_input
        .iter()
        .map(|comment| TicketUpdateCommentRequest {
            comment_id: comment.comment_id.as_deref(),
            comment_type: comment.comment_type.as_deref(),
            body: &comment.body,
            evidence: None,
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
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
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
            )?;
            sync_ticket_references(loom, ns, &ticket)?;
            ticket_mutation_json(ticket, "ticket.updated", expected_root, changes)
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_delete_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    ticket_id: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_delete_json");
    let workspace = arg_str!(workspace, "loom_tickets_delete_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_delete_json");
    let ticket_id = arg_str!(ticket_id, "loom_tickets_delete_json");
    let expected_root = match unsafe { optional_str_arg(expected_root, "loom_tickets_delete_json") }
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            let ticket = loom_tickets::delete_ticket(
                loom,
                ns,
                TicketDeleteRequest {
                    workspace_id: ticket_workspace_id,
                    ticket_id,
                    expected_root,
                },
            )?;
            sync_ticket_references(loom, ns, &ticket)?;
            ticket_mutation_json(
                ticket,
                "ticket.deleted",
                expected_root,
                vec![MutationChange::ResourceDeleted],
            )
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_comments_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    ticket_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_comments_json");
    let workspace = arg_str!(workspace, "loom_tickets_comments_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_comments_json");
    let ticket_id = arg_str!(ticket_id, "loom_tickets_comments_json");
    out_json!(
        out,
        ticket_read_loom(h, workspace, |loom, ns| {
            json_result(loom_tickets::list_ticket_comments(
                loom,
                ns,
                ticket_workspace_id,
                ticket_id,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_comment_add_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    ticket_id: *const c_char,
    comment_id: *const c_char,
    comment_type: *const c_char,
    body: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_comment_add_json");
    let workspace = arg_str!(workspace, "loom_tickets_comment_add_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_comment_add_json");
    let ticket_id = arg_str!(ticket_id, "loom_tickets_comment_add_json");
    let comment_id = match unsafe { optional_str_arg(comment_id, "loom_tickets_comment_add_json") }
    {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let comment_type =
        match unsafe { optional_str_arg(comment_type, "loom_tickets_comment_add_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let body = arg_str!(body, "loom_tickets_comment_add_json");
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_tickets_comment_add_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let mut changes = vec![MutationChange::field_set(
        "comment_type",
        comment_type.unwrap_or(loom_tickets::TICKET_DEFAULT_COMMENT_TYPE),
    )];
    if let Some(comment_id) = comment_id {
        changes.push(MutationChange::field_set("comment_id", comment_id));
    }
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            let ticket = loom_tickets::add_ticket_comment(
                loom,
                ns,
                TicketCommentRequest {
                    workspace_id: ticket_workspace_id,
                    ticket_id,
                    comment_id,
                    comment_type,
                    body,
                    evidence: None,
                    expected_root,
                },
            )?;
            sync_ticket_references(loom, ns, &ticket)?;
            ticket_mutation_json(ticket, "ticket.comment_added", expected_root, changes)
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_comment_update_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    ticket_id: *const c_char,
    comment_id: *const c_char,
    comment_type: *const c_char,
    body: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_comment_update_json");
    let workspace = arg_str!(workspace, "loom_tickets_comment_update_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_comment_update_json");
    let ticket_id = arg_str!(ticket_id, "loom_tickets_comment_update_json");
    let comment_id = arg_str!(comment_id, "loom_tickets_comment_update_json");
    let comment_type =
        match unsafe { optional_str_arg(comment_type, "loom_tickets_comment_update_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let body = match unsafe { optional_str_arg(body, "loom_tickets_comment_update_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_tickets_comment_update_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let mut changes = vec![MutationChange::field_set("comment_id", comment_id)];
    if let Some(comment_type) = comment_type {
        changes.push(MutationChange::field_set("comment_type", comment_type));
    }
    if body.is_some() {
        changes.push(MutationChange::field_set("body", "updated"));
    }
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            let ticket = loom_tickets::update_ticket_comment(
                loom,
                ns,
                TicketCommentUpdateRequest {
                    workspace_id: ticket_workspace_id,
                    ticket_id,
                    comment_id,
                    comment_type,
                    body,
                    evidence: None,
                    expected_root,
                },
            )?;
            sync_ticket_references(loom, ns, &ticket)?;
            ticket_mutation_json(ticket, "ticket.comment_updated", expected_root, changes)
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_comment_delete_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    ticket_id: *const c_char,
    comment_id: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_comment_delete_json");
    let workspace = arg_str!(workspace, "loom_tickets_comment_delete_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_comment_delete_json");
    let ticket_id = arg_str!(ticket_id, "loom_tickets_comment_delete_json");
    let comment_id = arg_str!(comment_id, "loom_tickets_comment_delete_json");
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_tickets_comment_delete_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            let ticket = loom_tickets::delete_ticket_comment(
                loom,
                ns,
                TicketCommentDeleteRequest {
                    workspace_id: ticket_workspace_id,
                    ticket_id,
                    comment_id,
                    expected_root,
                },
            )?;
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
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_relation_set_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    ticket_id: *const c_char,
    relation_id: *const c_char,
    kind: *const c_char,
    target_id: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_relation_set_json");
    let workspace = arg_str!(workspace, "loom_tickets_relation_set_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_relation_set_json");
    let ticket_id = arg_str!(ticket_id, "loom_tickets_relation_set_json");
    let relation_id =
        match unsafe { optional_str_arg(relation_id, "loom_tickets_relation_set_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let kind = arg_str!(kind, "loom_tickets_relation_set_json");
    let target_id = arg_str!(target_id, "loom_tickets_relation_set_json");
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_tickets_relation_set_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let kind = match TicketRelationKind::parse(kind) {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
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
            )?;
            let change = MutationChange::relation_set(
                relation.relation_id.clone(),
                relation.kind.clone(),
                relation.target_id.clone(),
            );
            relation_mutation_json(relation, "ticket.relation_set", expected_root, vec![change])
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_relation_remove_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    ticket_id: *const c_char,
    relation_id: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_relation_remove_json");
    let workspace = arg_str!(workspace, "loom_tickets_relation_remove_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_relation_remove_json");
    let ticket_id = arg_str!(ticket_id, "loom_tickets_relation_remove_json");
    let relation_id = arg_str!(relation_id, "loom_tickets_relation_remove_json");
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_tickets_relation_remove_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            let relation = loom_tickets::remove_ticket_relation(
                loom,
                ns,
                TicketRelationRemoveRequest {
                    workspace_id: ticket_workspace_id,
                    ticket_id,
                    relation_id,
                    expected_root,
                },
            )?;
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
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_relation_list_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    ticket_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_relation_list_json");
    let workspace = arg_str!(workspace, "loom_tickets_relation_list_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_relation_list_json");
    let ticket_id = arg_str!(ticket_id, "loom_tickets_relation_list_json");
    out_json!(
        out,
        ticket_read_loom(h, workspace, |loom, ns| {
            json_result(loom_tickets::list_ticket_relations(
                loom,
                ns,
                ticket_workspace_id,
                ticket_id,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_get_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    ticket_id: *const c_char,
    projection: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_get_json");
    let workspace = arg_str!(workspace, "loom_tickets_get_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_get_json");
    let ticket_id = arg_str!(ticket_id, "loom_tickets_get_json");
    let projection = match unsafe { optional_str_arg(projection, "loom_tickets_get_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        ticket_read_loom(h, workspace, |loom, ns| {
            let projection = parse_projection(projection)?;
            json_result(loom_tickets::get_ticket_with_projection(
                loom,
                ns,
                ticket_workspace_id,
                ticket_id,
                projection,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_board_create_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    request_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_board_create_json");
    let workspace = arg_str!(workspace, "loom_tickets_board_create_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_board_create_json");
    let request_json = arg_str!(request_json, "loom_tickets_board_create_json");
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            let request: FfiBoardCreateRequest = serde_json::from_str(request_json)
                .map_err(|e| LoomError::invalid(format!("board create request json: {e}")))?;
            let columns = board_columns(request.columns)?;
            let swimlanes = board_swimlanes(request.swimlanes)?;
            json_result(loom_tickets::create_board(
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
                    mode: BoardMode::parse(&request.mode)?,
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
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_board_get_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    board_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_board_get_json");
    let workspace = arg_str!(workspace, "loom_tickets_board_get_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_board_get_json");
    let board_id = arg_str!(board_id, "loom_tickets_board_get_json");
    out_json!(
        out,
        ticket_read_loom(h, workspace, |loom, ns| {
            json_result(loom_tickets::get_board(
                loom,
                ns,
                ticket_workspace_id,
                board_id,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_board_list_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    include_deleted: bool,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_board_list_json");
    let workspace = arg_str!(workspace, "loom_tickets_board_list_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_board_list_json");
    out_json!(
        out,
        ticket_read_loom(h, workspace, |loom, ns| {
            json_result(loom_tickets::list_boards(
                loom,
                ns,
                ticket_workspace_id,
                include_deleted,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_board_update_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    board_id: *const c_char,
    request_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_board_update_json");
    let workspace = arg_str!(workspace, "loom_tickets_board_update_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_board_update_json");
    let board_id = arg_str!(board_id, "loom_tickets_board_update_json");
    let request_json = arg_str!(request_json, "loom_tickets_board_update_json");
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            let request: FfiBoardUpdateRequest = serde_json::from_str(request_json)
                .map_err(|e| LoomError::invalid(format!("board update request json: {e}")))?;
            let board_status = request
                .board_status
                .as_deref()
                .map(BoardStatus::parse)
                .transpose()?;
            json_result(loom_tickets::update_board(
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
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_board_delete_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    board_id: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_board_delete_json");
    let workspace = arg_str!(workspace, "loom_tickets_board_delete_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_board_delete_json");
    let board_id = arg_str!(board_id, "loom_tickets_board_delete_json");
    let expected_root =
        match unsafe { optional_str_arg(expected_root, "loom_tickets_board_delete_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            json_result(loom_tickets::update_board(
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
                    updated_by: "ffi",
                    expected_root,
                },
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_board_configure_columns_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    board_id: *const c_char,
    request_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_board_configure_columns_json");
    let workspace = arg_str!(workspace, "loom_tickets_board_configure_columns_json");
    let ticket_workspace_id = arg_str!(
        ticket_workspace_id,
        "loom_tickets_board_configure_columns_json"
    );
    let board_id = arg_str!(board_id, "loom_tickets_board_configure_columns_json");
    let request_json = arg_str!(request_json, "loom_tickets_board_configure_columns_json");
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            let request: FfiBoardConfigureColumnsRequest = serde_json::from_str(request_json)
                .map_err(|e| LoomError::invalid(format!("board columns request json: {e}")))?;
            let columns = board_columns(request.columns)?;
            let swimlanes = board_swimlanes(request.swimlanes)?;
            let mode = request.mode.as_deref().map(BoardMode::parse).transpose()?;
            json_result(loom_tickets::configure_board_columns(
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
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_board_move_card_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    board_id: *const c_char,
    request_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_board_move_card_json");
    let workspace = arg_str!(workspace, "loom_tickets_board_move_card_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_board_move_card_json");
    let board_id = arg_str!(board_id, "loom_tickets_board_move_card_json");
    let request_json = arg_str!(request_json, "loom_tickets_board_move_card_json");
    out_json!(
        out,
        ticket_write_loom(h, workspace, |loom, ns| {
            let request: FfiBoardMoveCardRequest = serde_json::from_str(request_json)
                .map_err(|e| LoomError::invalid(format!("board move card request json: {e}")))?;
            json_result(loom_tickets::move_board_card(
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
    )
}

/// Wire request shape for `loom_tickets_list_json`, decoded from its one optional JSON request
/// object. `lane` is a first-class Lane id resolved to a membership allowlist here; the rest maps to
/// `TicketListQuery`.
#[derive(serde::Deserialize, Default)]
struct FfiTicketListRequest {
    #[serde(default)]
    projection: Option<String>,
    #[serde(default)]
    statuses: Vec<String>,
    #[serde(default)]
    assignees: Vec<String>,
    #[serde(default)]
    priorities: Vec<String>,
    #[serde(default)]
    ticket_types: Vec<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    policy_labels: Vec<String>,
    #[serde(default)]
    lane: Option<String>,
    #[serde(default)]
    board: Option<String>,
    #[serde(default)]
    ready: bool,
    #[serde(default)]
    include_completed: bool,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    cursor: Option<String>,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_list_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    request: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_list_json");
    let workspace = arg_str!(workspace, "loom_tickets_list_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_list_json");
    let request = match unsafe { optional_str_arg(request, "loom_tickets_list_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        ticket_read_loom(h, workspace, |loom, ns| {
            let req: FfiTicketListRequest = match request {
                Some(text) => serde_json::from_str(text).map_err(|e| {
                    LoomError::invalid(format!("loom_tickets_list_json request: {e}"))
                })?,
                None => FfiTicketListRequest::default(),
            };
            let projection = parse_projection(req.projection.as_deref())?;
            let lane_member_ids = match req.lane.as_deref() {
                Some(lane_id) => Some(
                    loom_lanes::get_lane(loom, ns, lane_id)?
                        .ok_or_else(|| LoomError::not_found(format!("lane {lane_id:?} not found")))?
                        .lane_tickets
                        .iter()
                        .map(|ticket| ticket.ticket_id.clone())
                        .collect::<Vec<_>>(),
                ),
                None => None,
            };
            json_result(loom_tickets::list_tickets_page(
                loom,
                ns,
                ticket_workspace_id,
                &loom_tickets::TicketListQuery {
                    projection,
                    statuses: req.statuses,
                    assignees: req.assignees,
                    priorities: req.priorities,
                    ticket_types: req.ticket_types,
                    labels: req.labels,
                    policy_labels: req.policy_labels,
                    ready_only: req.ready,
                    include_completed: req.include_completed,
                    lane_member_ids,
                    board_id: req.board,
                    cursor: req.cursor,
                    limit: req.limit,
                },
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_tickets_history_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ticket_workspace_id: *const c_char,
    ticket_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_tickets_history_json");
    let workspace = arg_str!(workspace, "loom_tickets_history_json");
    let ticket_workspace_id = arg_str!(ticket_workspace_id, "loom_tickets_history_json");
    let ticket_id = match unsafe { optional_str_arg(ticket_id, "loom_tickets_history_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        ticket_read_loom(h, workspace, |loom, ns| {
            json_result(loom_tickets::history(
                loom,
                ns,
                ticket_workspace_id,
                ticket_id,
            ))
        })
    )
}
