use loom_tickets::{
    TicketCommentDeleteRequest, TicketCommentRequest, TicketCommentUpdateRequest,
    TicketCreateRequest, TicketDeleteRequest, TicketFieldDefinitionRetireRequest,
    TicketFieldDefinitionWriteRequest, TicketLifecycleAction, TicketLifecycleAuthorizationPolicy,
    TicketRelationKind, TicketRelationRemoveRequest, TicketRelationRequest, TicketSummary,
    TicketUpdateCommentRequest, TicketUpdateRelationRemoveRequest, TicketUpdateRelationSetRequest,
    TicketUpdateRequest,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value as JsonValue;
use wasm_bindgen::prelude::*;

use super::{Digest, LoomStore, le, now_ms, resolve_workspace_arg, save_loom};

fn to_json<T: Serialize>(value: loom_core::Result<T>) -> Result<String, JsError> {
    let value = value.map_err(le)?;
    serde_json::to_string(&value).map_err(|error| JsError::new(&error.to_string()))
}

fn parse_json(value: &str, what: &str) -> Result<JsonValue, JsError> {
    serde_json::from_str(value).map_err(|error| JsError::new(&format!("{what}: {error}")))
}

fn parse_string_list(value: &str, what: &str) -> Result<Vec<String>, JsError> {
    serde_json::from_str(value).map_err(|error| JsError::new(&format!("{what}: {error}")))
}

fn parse_optional_json_list<T: DeserializeOwned>(
    value: Option<&str>,
    what: &str,
) -> Result<Vec<T>, JsError> {
    value
        .map(|value| serde_json::from_str(value))
        .transpose()
        .map_err(|error| JsError::new(&format!("{what}: {error}")))
        .map(|value| value.unwrap_or_default())
}

#[derive(Deserialize)]
struct WasmTicketUpdateComment {
    #[serde(default)]
    comment_id: Option<String>,
    #[serde(default)]
    comment_type: Option<String>,
    body: String,
}

#[derive(Deserialize)]
struct WasmTicketUpdateRelationSet {
    #[serde(default)]
    relation_id: Option<String>,
    kind: String,
    target_id: String,
}

#[derive(Deserialize)]
struct WasmTicketUpdateRelationRemove {
    relation_id: String,
}

fn parse_projection_list(
    value: &str,
    what: &str,
) -> Result<Vec<loom_tickets::TicketProjectionProfile>, JsError> {
    parse_string_list(value, what)?
        .into_iter()
        .map(|profile| loom_tickets::TicketProjectionProfile::parse(&profile).map_err(le))
        .collect()
}

fn parse_cardinality(value: &str) -> Result<loom_tickets::TicketFieldCardinality, JsError> {
    match value {
        "single" => Ok(loom_tickets::TicketFieldCardinality::Single),
        "optional" => Ok(loom_tickets::TicketFieldCardinality::Optional),
        "list" => Ok(loom_tickets::TicketFieldCardinality::List {
            min_items: 0,
            max_items: None,
        }),
        other => Err(JsError::new(&format!(
            "ticket field cardinality must be single, optional, or list: {other}"
        ))),
    }
}

fn sync_ticket_references(
    store: &mut LoomStore,
    workspace: loom_core::workspace::WorkspaceId,
    ticket: &TicketSummary,
) -> Result<(), JsError> {
    loom_tickets::update_ticket_field_references(
        &mut store.loom,
        workspace,
        &ticket.workspace_id,
        &ticket.ticket_id,
        &ticket.fields,
    )
    .map_err(le)?;
    if let Some(operation_id) = ticket.operation_id.as_deref() {
        loom_tickets::enqueue_ticket_reference_candidates(
            &mut store.loom,
            workspace,
            loom_tickets::TicketReferenceCandidateRequest {
                workspace_id: &ticket.workspace_id,
                ticket_id: &ticket.ticket_id,
                operation_id,
                source_root: Digest::parse(&ticket.profile_root).map_err(le)?,
                fields: &ticket.fields,
                now_ms: now_ms(),
            },
        )
        .map_err(le)?;
    }
    Ok(())
}

#[wasm_bindgen]
impl LoomStore {
    pub fn tickets_project_create_json(
        &mut self,
        workspace: String,
        ticket_workspace_id: String,
        project_id: String,
        key_prefix: String,
        name: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_tickets::create_project(
            &mut self.loom,
            ns,
            &ticket_workspace_id,
            &project_id,
            &key_prefix,
            &name,
            expected_root.as_deref(),
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_project_rekey_json(
        &mut self,
        workspace: String,
        ticket_workspace_id: String,
        project_id: String,
        key_prefix: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_tickets::rekey_project(
            &mut self.loom,
            ns,
            &ticket_workspace_id,
            &project_id,
            &key_prefix,
            expected_root.as_deref(),
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_project_settings_get_json(
        &self,
        workspace: String,
        ticket_workspace_id: String,
        project_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_tickets::get_project(
            &self.loom,
            ns,
            &ticket_workspace_id,
            &project_id,
        ))
    }

    pub fn tickets_project_settings_set_json(
        &mut self,
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
    ) -> Result<String, JsError> {
        let default_projection = default_projection
            .as_deref()
            .map(loom_tickets::TicketProjectionProfile::parse)
            .transpose()
            .map_err(le)?;
        let enable_projections =
            parse_projection_list(&enable_projections_json, "ticket enable projections json")?;
        let disable_projections =
            parse_projection_list(&disable_projections_json, "ticket disable projections json")?;
        let actor_enforcement = actor_enforcement
            .as_deref()
            .map(TicketLifecycleAuthorizationPolicy::parse)
            .transpose()
            .map_err(le)?;
        let acceptance_authorities = acceptance_authorities_json
            .as_deref()
            .map(|value| parse_string_list(value, "ticket acceptance authorities json"))
            .transpose()?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_tickets::set_project_settings(
            &mut self.loom,
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
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_fields_json(
        &self,
        workspace: String,
        ticket_workspace_id: String,
        project_id: Option<String>,
        projection: Option<String>,
        operation: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let projection =
            loom_tickets::parse_ticket_projection(projection.as_deref()).map_err(le)?;
        match project_id.as_deref() {
            Some(project_id) => to_json(loom_tickets::ticket_field_catalog_for_project(
                &self.loom,
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
    }

    pub fn tickets_field_put_json(
        &mut self,
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
    ) -> Result<String, JsError> {
        let cardinality = parse_cardinality(&cardinality)?;
        let applicable_type_ids =
            parse_string_list(&applicable_type_ids_json, "ticket applicable type ids json")?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_tickets::put_ticket_field_definition(
            &mut self.loom,
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
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_field_retire_json(
        &mut self,
        workspace: String,
        ticket_workspace_id: String,
        project_id: String,
        field_id: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_tickets::retire_ticket_field_definition(
            &mut self.loom,
            ns,
            TicketFieldDefinitionRetireRequest {
                workspace_id: &ticket_workspace_id,
                project_id: &project_id,
                field_id: &field_id,
                expected_root: expected_root.as_deref(),
            },
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_create_json(
        &mut self,
        workspace: String,
        ticket_workspace_id: String,
        project_id: String,
        ticket_type: String,
        external_source: Option<String>,
        external_id: Option<String>,
        fields_json: String,
        policy_labels_json: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let fields = parse_json(&fields_json, "ticket fields json")?;
        let policy_labels = parse_string_list(&policy_labels_json, "ticket policy labels json")?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let ticket = loom_tickets::create_ticket(
            &mut self.loom,
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
        .map_err(le)?;
        sync_ticket_references(self, ns, &ticket)?;
        let out =
            serde_json::to_string(&ticket).map_err(|error| JsError::new(&error.to_string()))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_update_json(
        &mut self,
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
        comments_json: Option<String>,
        relation_sets_json: Option<String>,
        relation_removes_json: Option<String>,
    ) -> Result<String, JsError> {
        if comment_body.is_none() && (comment_id.is_some() || comment_type.is_some()) {
            return Err(le(loom_core::LoomError::invalid(
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
            .map_err(le)?;
        let comments_input = parse_optional_json_list::<WasmTicketUpdateComment>(
            comments_json.as_deref(),
            "ticket comments json",
        )?;
        let relation_sets_input = parse_optional_json_list::<WasmTicketUpdateRelationSet>(
            relation_sets_json.as_deref(),
            "ticket relation sets json",
        )?;
        let relation_removes_input = parse_optional_json_list::<WasmTicketUpdateRelationRemove>(
            relation_removes_json.as_deref(),
            "ticket relation removes json",
        )?;
        let relation_kinds = relation_sets_input
            .iter()
            .map(|relation| TicketRelationKind::parse(&relation.kind).map_err(le))
            .collect::<Result<Vec<_>, JsError>>()?;
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
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let ticket = loom_tickets::update_ticket(
            &mut self.loom,
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
        .map_err(le)?;
        sync_ticket_references(self, ns, &ticket)?;
        let out =
            serde_json::to_string(&ticket).map_err(|error| JsError::new(&error.to_string()))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_delete_json(
        &mut self,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let ticket = loom_tickets::delete_ticket(
            &mut self.loom,
            ns,
            TicketDeleteRequest {
                workspace_id: &ticket_workspace_id,
                ticket_id: &ticket_id,
                expected_root: expected_root.as_deref(),
            },
        )
        .map_err(le)?;
        sync_ticket_references(self, ns, &ticket)?;
        let out =
            serde_json::to_string(&ticket).map_err(|error| JsError::new(&error.to_string()))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_comments_json(
        &self,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_tickets::list_ticket_comments(
            &self.loom,
            ns,
            &ticket_workspace_id,
            &ticket_id,
        ))
    }

    pub fn tickets_comment_add_json(
        &mut self,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
        comment_id: Option<String>,
        comment_type: Option<String>,
        body: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let ticket = loom_tickets::add_ticket_comment(
            &mut self.loom,
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
        .map_err(le)?;
        sync_ticket_references(self, ns, &ticket)?;
        let out =
            serde_json::to_string(&ticket).map_err(|error| JsError::new(&error.to_string()))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_comment_update_json(
        &mut self,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
        comment_id: String,
        comment_type: Option<String>,
        body: Option<String>,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let ticket = loom_tickets::update_ticket_comment(
            &mut self.loom,
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
        .map_err(le)?;
        sync_ticket_references(self, ns, &ticket)?;
        let out =
            serde_json::to_string(&ticket).map_err(|error| JsError::new(&error.to_string()))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_comment_delete_json(
        &mut self,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
        comment_id: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let ticket = loom_tickets::delete_ticket_comment(
            &mut self.loom,
            ns,
            TicketCommentDeleteRequest {
                workspace_id: &ticket_workspace_id,
                ticket_id: &ticket_id,
                comment_id: &comment_id,
                expected_root: expected_root.as_deref(),
            },
        )
        .map_err(le)?;
        sync_ticket_references(self, ns, &ticket)?;
        let out =
            serde_json::to_string(&ticket).map_err(|error| JsError::new(&error.to_string()))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_relation_set_json(
        &mut self,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
        relation_id: Option<String>,
        kind: String,
        target_id: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let kind = TicketRelationKind::parse(&kind).map_err(le)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_tickets::add_ticket_relation(
            &mut self.loom,
            ns,
            TicketRelationRequest {
                workspace_id: &ticket_workspace_id,
                ticket_id: &ticket_id,
                relation_id: relation_id.as_deref(),
                kind,
                target_id: &target_id,
                expected_root: expected_root.as_deref(),
            },
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_relation_remove_json(
        &mut self,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
        relation_id: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_tickets::remove_ticket_relation(
            &mut self.loom,
            ns,
            TicketRelationRemoveRequest {
                workspace_id: &ticket_workspace_id,
                ticket_id: &ticket_id,
                relation_id: &relation_id,
                expected_root: expected_root.as_deref(),
            },
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn tickets_get_json(
        &self,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
        projection: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let projection =
            loom_tickets::parse_ticket_projection(projection.as_deref()).map_err(le)?;
        to_json(loom_tickets::get_ticket_with_projection(
            &self.loom,
            ns,
            &ticket_workspace_id,
            &ticket_id,
            projection,
        ))
    }

    pub fn tickets_list_json(
        &self,
        workspace: String,
        ticket_workspace_id: String,
        projection: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let projection =
            loom_tickets::parse_ticket_projection(projection.as_deref()).map_err(le)?;
        to_json(loom_tickets::list_tickets_with_projection(
            &self.loom,
            ns,
            &ticket_workspace_id,
            projection,
        ))
    }

    pub fn tickets_history_json(
        &self,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_tickets::history(
            &self.loom,
            ns,
            &ticket_workspace_id,
            ticket_id.as_deref(),
        ))
    }
}
