use loom_pages::{
    PageCreateRequest, StructureBindRequest, StructureCreateRequest, StructureDecomposeItem,
    StructureDecomposeRequest, StructureLinkRequest, StructureMoveRequest, StructureNodeRequest,
};
use serde::Serialize;
use serde_json::Value as JsonValue;
use wasm_bindgen::prelude::*;

use super::{LoomStore, le, now_ms, resolve_workspace_arg, save_loom};

struct OwnedDecomposeItem {
    node_id: String,
    project_id: String,
    ticket_type: Option<String>,
    fields: Option<JsonValue>,
    policy_labels: Vec<String>,
}

fn to_json<T: Serialize>(value: loom_core::Result<T>) -> Result<String, JsError> {
    let value = value.map_err(le)?;
    serde_json::to_string(&value).map_err(|error| JsError::new(&error.to_string()))
}

fn structure_node_request<'a>(
    page_workspace_id: &'a str,
    structure_id: &'a str,
    node_id: &'a str,
    kind: &'a str,
    label: &'a str,
    body_digest: Option<&'a str>,
    entity_ref: &'a Option<String>,
    expected_root: Option<&'a str>,
) -> StructureNodeRequest<'a> {
    StructureNodeRequest {
        workspace_id: page_workspace_id,
        structure_id,
        node_id,
        kind,
        label,
        body_digest,
        entity_ref: entity_ref.clone(),
        expected_root,
    }
}

fn parse_decompose_items(value: &str) -> Result<Vec<OwnedDecomposeItem>, JsError> {
    let items: Vec<JsonValue> =
        serde_json::from_str(value).map_err(|error| JsError::new(&error.to_string()))?;
    items
        .into_iter()
        .map(|item| {
            let object = item
                .as_object()
                .ok_or_else(|| JsError::new("structure decompose item must be an object"))?;
            let node_id = object
                .get("nodeId")
                .and_then(JsonValue::as_str)
                .or_else(|| object.get("node_id").and_then(JsonValue::as_str))
                .ok_or_else(|| JsError::new("structure decompose item missing node_id"))?
                .to_string();
            let project_id = object
                .get("projectId")
                .and_then(JsonValue::as_str)
                .or_else(|| object.get("project_id").and_then(JsonValue::as_str))
                .ok_or_else(|| JsError::new("structure decompose item missing project_id"))?
                .to_string();
            let ticket_type = object
                .get("ticketType")
                .and_then(JsonValue::as_str)
                .or_else(|| object.get("ticket_type").and_then(JsonValue::as_str))
                .map(str::to_string);
            let fields = object.get("fields").cloned();
            let policy_labels = object
                .get("policyLabels")
                .or_else(|| object.get("policy_labels"))
                .map(|value| {
                    serde_json::from_value(value.clone())
                        .map_err(|error| JsError::new(&error.to_string()))
                })
                .transpose()?
                .unwrap_or_default();
            Ok(OwnedDecomposeItem {
                node_id,
                project_id,
                ticket_type,
                fields,
                policy_labels,
            })
        })
        .collect()
}

#[wasm_bindgen]
impl LoomStore {
    pub fn spaces_create_json(
        &mut self,
        workspace: String,
        page_workspace_id: String,
        space_id: String,
        title: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_pages::create_space(
            &mut self.loom,
            ns,
            &page_workspace_id,
            &space_id,
            &title,
            expected_root.as_deref(),
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn spaces_list_json(
        &self,
        workspace: String,
        page_workspace_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_pages::list_spaces(&self.loom, ns, &page_workspace_id))
    }

    pub fn spaces_get_json(
        &self,
        workspace: String,
        page_workspace_id: String,
        space_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_pages::get_space(
            &self.loom,
            ns,
            &page_workspace_id,
            &space_id,
        ))
    }

    pub fn pages_create_json(
        &mut self,
        workspace: String,
        page_workspace_id: String,
        page_id: String,
        space_id: String,
        parent_page_id: Option<String>,
        title: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_pages::create_page(
            &mut self.loom,
            ns,
            PageCreateRequest {
                workspace_id: &page_workspace_id,
                page_id: &page_id,
                space_id: &space_id,
                parent_page_id: parent_page_id.as_deref(),
                title: &title,
                expected_root: expected_root.as_deref(),
            },
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn pages_update_json(
        &mut self,
        workspace: String,
        page_workspace_id: String,
        page_id: String,
        body_text: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_pages::update_page_text(
            &mut self.loom,
            ns,
            &page_workspace_id,
            &page_id,
            &body_text,
            now_ms(),
            expected_root.as_deref(),
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn pages_publish_json(
        &mut self,
        workspace: String,
        page_workspace_id: String,
        page_id: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_pages::publish_page(
            &mut self.loom,
            ns,
            &page_workspace_id,
            &page_id,
            now_ms(),
            expected_root.as_deref(),
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn pages_get_json(
        &self,
        workspace: String,
        page_workspace_id: String,
        page_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_pages::get_page(
            &self.loom,
            ns,
            &page_workspace_id,
            &page_id,
        ))
    }

    pub fn pages_list_json(
        &self,
        workspace: String,
        page_workspace_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_pages::list_pages(&self.loom, ns, &page_workspace_id))
    }

    pub fn pages_history_json(
        &self,
        workspace: String,
        page_workspace_id: String,
        page_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_pages::page_history(
            &self.loom,
            ns,
            &page_workspace_id,
            &page_id,
        ))
    }

    pub fn structures_create_json(
        &mut self,
        workspace: String,
        page_workspace_id: String,
        structure_id: String,
        space_id: String,
        kind: String,
        title: String,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_pages::create_structure(
            &mut self.loom,
            ns,
            StructureCreateRequest {
                workspace_id: &page_workspace_id,
                structure_id: &structure_id,
                space_id: &space_id,
                kind: &kind,
                title: &title,
                expected_root: expected_root.as_deref(),
            },
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn structures_add_node_json(
        &mut self,
        workspace: String,
        page_workspace_id: String,
        structure_id: String,
        node_id: String,
        kind: String,
        label: String,
        body_digest: Option<String>,
        entity_ref: Option<String>,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let request = structure_node_request(
            &page_workspace_id,
            &structure_id,
            &node_id,
            &kind,
            &label,
            body_digest.as_deref(),
            &entity_ref,
            expected_root.as_deref(),
        );
        let out = to_json(loom_pages::add_structure_node(&mut self.loom, ns, request))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn structures_update_node_json(
        &mut self,
        workspace: String,
        page_workspace_id: String,
        structure_id: String,
        node_id: String,
        kind: String,
        label: String,
        body_digest: Option<String>,
        entity_ref: Option<String>,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let request = structure_node_request(
            &page_workspace_id,
            &structure_id,
            &node_id,
            &kind,
            &label,
            body_digest.as_deref(),
            &entity_ref,
            expected_root.as_deref(),
        );
        let out = to_json(loom_pages::update_structure_node(&mut self.loom, ns, request))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn structures_bind_json(
        &mut self,
        workspace: String,
        page_workspace_id: String,
        structure_id: String,
        node_id: String,
        entity_ref: Option<String>,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_pages::bind_structure_node(
            &mut self.loom,
            ns,
            StructureBindRequest {
                workspace_id: &page_workspace_id,
                structure_id: &structure_id,
                node_id: &node_id,
                entity_ref,
                expected_root: expected_root.as_deref(),
            },
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn structures_move_node_json(
        &mut self,
        workspace: String,
        page_workspace_id: String,
        structure_id: String,
        node_id: String,
        parent_node_id: Option<String>,
        label: Option<String>,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_pages::move_structure_node(
            &mut self.loom,
            ns,
            StructureMoveRequest {
                workspace_id: &page_workspace_id,
                structure_id: &structure_id,
                node_id: &node_id,
                parent_node_id: parent_node_id.as_deref(),
                label: label.as_deref(),
                expected_root: expected_root.as_deref(),
            },
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn structures_link_node_json(
        &mut self,
        workspace: String,
        page_workspace_id: String,
        structure_id: String,
        edge_id: String,
        src_node_id: String,
        dst_node_id: String,
        label: String,
        target_ref: Option<String>,
        expected_root: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_pages::link_structure_node(
            &mut self.loom,
            ns,
            StructureLinkRequest {
                workspace_id: &page_workspace_id,
                structure_id: &structure_id,
                edge_id: &edge_id,
                src_node_id: &src_node_id,
                dst_node_id: &dst_node_id,
                label: &label,
                target_ref,
                expected_root: expected_root.as_deref(),
            },
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn structures_decompose_to_tickets_json(
        &mut self,
        workspace: String,
        page_workspace_id: String,
        structure_id: String,
        items_json: String,
    ) -> Result<String, JsError> {
        let owned_items = parse_decompose_items(&items_json)?;
        let items = owned_items
            .iter()
            .map(|item| StructureDecomposeItem {
                node_id: item.node_id.as_str(),
                project_id: item.project_id.as_str(),
                ticket_type: item.ticket_type.as_deref(),
                fields: item.fields.as_ref(),
                policy_labels: &item.policy_labels,
            })
            .collect::<Vec<_>>();
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_pages::decompose_to_tickets(
            &mut self.loom,
            ns,
            StructureDecomposeRequest {
                workspace_id: &page_workspace_id,
                structure_id: &structure_id,
                items: &items,
            },
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn structures_get_json(
        &self,
        workspace: String,
        page_workspace_id: String,
        structure_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_pages::get_structure(
            &self.loom,
            ns,
            &page_workspace_id,
            &structure_id,
        ))
    }

    pub fn structures_list_json(
        &self,
        workspace: String,
        page_workspace_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_pages::list_structures(
            &self.loom,
            ns,
            &page_workspace_id,
        ))
    }
}
