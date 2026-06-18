//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_pages::{
    PageCreateRequest, StructureBindRequest, StructureCreateRequest, StructureDecomposeItem,
    StructureDecomposeRequest, StructureLinkRequest, StructureMoveRequest, StructureNodeRequest,
};
use serde::Serialize;
use serde_json::Value as JsonValue;

fn to_json<T: Serialize>(value: loom_core::error::Result<T>) -> napi::Result<String> {
    let value = value.map_err(reason)?;
    serde_json::to_string(&value).map_err(|error| napi::Error::from_reason(error.to_string()))
}

fn pages_read<T>(
    loom_path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> napi::Result<T>,
) -> napi::Result<T> {
    let loom = open_loom_read_unlocked(loom_path, key_spec(passphrase).as_ref()).map_err(reason)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    f(&loom, workspace_id)
}

fn pages_write<T>(
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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[napi]
pub fn spaces_create_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    space_id: String,
    title: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::create_space(
            loom,
            ns,
            &page_workspace_id,
            &space_id,
            &title,
            expected_root.as_deref(),
        ))
    })
}

#[napi]
pub fn spaces_list_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::list_spaces(loom, ns, &page_workspace_id))
    })
}

#[napi]
pub fn spaces_get_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    space_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::get_space(
            loom,
            ns,
            &page_workspace_id,
            &space_id,
        ))
    })
}

#[napi]
pub fn pages_create_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    page_id: String,
    space_id: String,
    parent_page_id: Option<String>,
    title: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::create_page(
            loom,
            ns,
            PageCreateRequest {
                workspace_id: &page_workspace_id,
                page_id: &page_id,
                space_id: &space_id,
                parent_page_id: parent_page_id.as_deref(),
                title: &title,
                expected_root: expected_root.as_deref(),
            },
        ))
    })
}

#[napi]
pub fn pages_update_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    page_id: String,
    body_text: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::update_page_text(
            loom,
            ns,
            &page_workspace_id,
            &page_id,
            &body_text,
            now_ms(),
            expected_root.as_deref(),
        ))
    })
}

#[napi]
pub fn pages_publish_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    page_id: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::publish_page(
            loom,
            ns,
            &page_workspace_id,
            &page_id,
            now_ms(),
            expected_root.as_deref(),
        ))
    })
}

#[napi]
pub fn pages_get_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    page_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::get_page(loom, ns, &page_workspace_id, &page_id))
    })
}

#[napi]
pub fn pages_list_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::list_pages(loom, ns, &page_workspace_id))
    })
}

#[napi]
pub fn pages_history_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    page_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::page_history(
            loom,
            ns,
            &page_workspace_id,
            &page_id,
        ))
    })
}

#[napi]
pub fn structures_create_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    structure_id: String,
    space_id: String,
    kind: String,
    title: String,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::create_structure(
            loom,
            ns,
            StructureCreateRequest {
                workspace_id: &page_workspace_id,
                structure_id: &structure_id,
                space_id: &space_id,
                kind: &kind,
                title: &title,
                expected_root: expected_root.as_deref(),
            },
        ))
    })
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

#[napi]
pub fn structures_add_node_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    structure_id: String,
    node_id: String,
    kind: String,
    label: String,
    body_digest: Option<String>,
    entity_ref: Option<String>,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::add_structure_node(
            loom,
            ns,
            structure_node_request(
                &page_workspace_id,
                &structure_id,
                &node_id,
                &kind,
                &label,
                body_digest.as_deref(),
                &entity_ref,
                expected_root.as_deref(),
            ),
        ))
    })
}

#[napi]
pub fn structures_update_node_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    structure_id: String,
    node_id: String,
    kind: String,
    label: String,
    body_digest: Option<String>,
    entity_ref: Option<String>,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::update_structure_node(
            loom,
            ns,
            structure_node_request(
                &page_workspace_id,
                &structure_id,
                &node_id,
                &kind,
                &label,
                body_digest.as_deref(),
                &entity_ref,
                expected_root.as_deref(),
            ),
        ))
    })
}

#[napi]
pub fn structures_bind_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    structure_id: String,
    node_id: String,
    entity_ref: Option<String>,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::bind_structure_node(
            loom,
            ns,
            StructureBindRequest {
                workspace_id: &page_workspace_id,
                structure_id: &structure_id,
                node_id: &node_id,
                entity_ref: entity_ref.clone(),
                expected_root: expected_root.as_deref(),
            },
        ))
    })
}

#[napi]
pub fn structures_move_node_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    structure_id: String,
    node_id: String,
    parent_node_id: Option<String>,
    label: Option<String>,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::move_structure_node(
            loom,
            ns,
            StructureMoveRequest {
                workspace_id: &page_workspace_id,
                structure_id: &structure_id,
                node_id: &node_id,
                parent_node_id: parent_node_id.as_deref(),
                label: label.as_deref(),
                expected_root: expected_root.as_deref(),
            },
        ))
    })
}

#[napi]
pub fn structures_link_node_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    structure_id: String,
    edge_id: String,
    src_node_id: String,
    dst_node_id: String,
    label: String,
    target_ref: Option<String>,
    expected_root: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::link_structure_node(
            loom,
            ns,
            StructureLinkRequest {
                workspace_id: &page_workspace_id,
                structure_id: &structure_id,
                edge_id: &edge_id,
                src_node_id: &src_node_id,
                dst_node_id: &dst_node_id,
                label: &label,
                target_ref: target_ref.clone(),
                expected_root: expected_root.as_deref(),
            },
        ))
    })
}

#[derive(serde::Deserialize)]
struct StructureDecomposeItemJson {
    node_id: String,
    project_id: String,
    ticket_type: Option<String>,
    fields: Option<JsonValue>,
    #[serde(default)]
    policy_labels: Vec<String>,
}

#[napi]
pub fn structures_decompose_to_tickets_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    structure_id: String,
    items_json: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let items: Vec<StructureDecomposeItemJson> = serde_json::from_str(&items_json)
        .map_err(|error| napi::Error::from_reason(format!("structure decompose items: {error}")))?;
    let borrowed = items
        .iter()
        .map(|item| StructureDecomposeItem {
            node_id: item.node_id.as_str(),
            project_id: item.project_id.as_str(),
            ticket_type: item.ticket_type.as_deref(),
            fields: item.fields.as_ref(),
            policy_labels: &item.policy_labels,
        })
        .collect::<Vec<_>>();
    pages_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::decompose_to_tickets(
            loom,
            ns,
            StructureDecomposeRequest {
                workspace_id: &page_workspace_id,
                structure_id: &structure_id,
                items: &borrowed,
            },
        ))
    })
}

#[napi]
pub fn structures_get_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    structure_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::get_structure(
            loom,
            ns,
            &page_workspace_id,
            &structure_id,
        ))
    })
}

#[napi]
pub fn structures_list_json(
    loom_path: String,
    workspace: String,
    page_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    pages_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_pages::list_structures(loom, ns, &page_workspace_id))
    })
}
