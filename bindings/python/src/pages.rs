//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_pages::{
    PageCreateRequest, StructureBindRequest, StructureCreateRequest, StructureDecomposeItem,
    StructureDecomposeRequest, StructureLinkRequest, StructureMoveRequest, StructureNodeRequest,
};
use serde::Serialize;
use serde_json::Value as JsonValue;

fn to_json<T: Serialize>(value: loom_core::error::Result<T>) -> PyResult<String> {
    let value = value.map_err(py_err)?;
    serde_json::to_string(&value).map_err(|error| PyRuntimeError::new_err(error.to_string()))
}

fn pages_read<T>(
    path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> PyResult<T>,
) -> PyResult<T> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    f(&loom, workspace_id)
}

fn pages_write<T>(
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

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, space_id, title, expected_root=None, passphrase=None))]
pub(crate) fn spaces_create_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    space_id: &str,
    title: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::create_space(
            loom,
            ns,
            page_workspace_id,
            space_id,
            title,
            expected_root,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, passphrase=None))]
pub(crate) fn spaces_list_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::list_spaces(loom, ns, page_workspace_id))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, space_id, passphrase=None))]
pub(crate) fn spaces_get_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    space_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::get_space(loom, ns, page_workspace_id, space_id))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, page_id, space_id, parent_page_id=None, title="", expected_root=None, passphrase=None))]
pub(crate) fn pages_create_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    page_id: &str,
    space_id: &str,
    parent_page_id: Option<&str>,
    title: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::create_page(
            loom,
            ns,
            PageCreateRequest {
                workspace_id: page_workspace_id,
                page_id,
                space_id,
                parent_page_id,
                title,
                expected_root,
            },
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, page_id, body_text, expected_root=None, passphrase=None))]
pub(crate) fn pages_update_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    page_id: &str,
    body_text: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::update_page_text(
            loom,
            ns,
            page_workspace_id,
            page_id,
            body_text,
            now_ms(),
            expected_root,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, page_id, expected_root=None, passphrase=None))]
pub(crate) fn pages_publish_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    page_id: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::publish_page(
            loom,
            ns,
            page_workspace_id,
            page_id,
            now_ms(),
            expected_root,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, page_id, passphrase=None))]
pub(crate) fn pages_get_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    page_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::get_page(loom, ns, page_workspace_id, page_id))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, passphrase=None))]
pub(crate) fn pages_list_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::list_pages(loom, ns, page_workspace_id))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, page_id, passphrase=None))]
pub(crate) fn pages_history_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    page_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::page_history(
            loom,
            ns,
            page_workspace_id,
            page_id,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, structure_id, space_id, kind, title, expected_root=None, passphrase=None))]
pub(crate) fn structures_create_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    structure_id: &str,
    space_id: &str,
    kind: &str,
    title: &str,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::create_structure(
            loom,
            ns,
            StructureCreateRequest {
                workspace_id: page_workspace_id,
                structure_id,
                space_id,
                kind,
                title,
                expected_root,
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
    entity_ref: Option<&'a str>,
    expected_root: Option<&'a str>,
) -> StructureNodeRequest<'a> {
    StructureNodeRequest {
        workspace_id: page_workspace_id,
        structure_id,
        node_id,
        kind,
        label,
        body_digest,
        entity_ref: entity_ref.map(str::to_string),
        expected_root,
    }
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, structure_id, node_id, kind, label, body_digest=None, entity_ref=None, expected_root=None, passphrase=None))]
pub(crate) fn structures_add_node_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    structure_id: &str,
    node_id: &str,
    kind: &str,
    label: &str,
    body_digest: Option<&str>,
    entity_ref: Option<&str>,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::add_structure_node(
            loom,
            ns,
            structure_node_request(
                page_workspace_id,
                structure_id,
                node_id,
                kind,
                label,
                body_digest,
                entity_ref,
                expected_root,
            ),
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, structure_id, node_id, kind, label, body_digest=None, entity_ref=None, expected_root=None, passphrase=None))]
pub(crate) fn structures_update_node_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    structure_id: &str,
    node_id: &str,
    kind: &str,
    label: &str,
    body_digest: Option<&str>,
    entity_ref: Option<&str>,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::update_structure_node(
            loom,
            ns,
            structure_node_request(
                page_workspace_id,
                structure_id,
                node_id,
                kind,
                label,
                body_digest,
                entity_ref,
                expected_root,
            ),
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, structure_id, node_id, entity_ref=None, expected_root=None, passphrase=None))]
pub(crate) fn structures_bind_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    structure_id: &str,
    node_id: &str,
    entity_ref: Option<&str>,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::bind_structure_node(
            loom,
            ns,
            StructureBindRequest {
                workspace_id: page_workspace_id,
                structure_id,
                node_id,
                entity_ref: entity_ref.map(str::to_string),
                expected_root,
            },
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, structure_id, node_id, parent_node_id=None, label=None, expected_root=None, passphrase=None))]
pub(crate) fn structures_move_node_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    structure_id: &str,
    node_id: &str,
    parent_node_id: Option<&str>,
    label: Option<&str>,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::move_structure_node(
            loom,
            ns,
            StructureMoveRequest {
                workspace_id: page_workspace_id,
                structure_id,
                node_id,
                parent_node_id,
                label,
                expected_root,
            },
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, structure_id, edge_id, src_node_id, dst_node_id, label, target_ref=None, expected_root=None, passphrase=None))]
pub(crate) fn structures_link_node_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    structure_id: &str,
    edge_id: &str,
    src_node_id: &str,
    dst_node_id: &str,
    label: &str,
    target_ref: Option<&str>,
    expected_root: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::link_structure_node(
            loom,
            ns,
            StructureLinkRequest {
                workspace_id: page_workspace_id,
                structure_id,
                edge_id,
                src_node_id,
                dst_node_id,
                label,
                target_ref: target_ref.map(str::to_string),
                expected_root,
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

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, structure_id, items_json, passphrase=None))]
pub(crate) fn structures_decompose_to_tickets_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    structure_id: &str,
    items_json: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let items: Vec<StructureDecomposeItemJson> = serde_json::from_str(items_json)
        .map_err(|error| PyRuntimeError::new_err(format!("structure decompose items: {error}")))?;
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
    pages_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::decompose_to_tickets(
            loom,
            ns,
            StructureDecomposeRequest {
                workspace_id: page_workspace_id,
                structure_id,
                items: &borrowed,
            },
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, structure_id, passphrase=None))]
pub(crate) fn structures_get_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    structure_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::get_structure(
            loom,
            ns,
            page_workspace_id,
            structure_id,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, page_workspace_id, passphrase=None))]
pub(crate) fn structures_list_json(
    path: &str,
    workspace: &str,
    page_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    pages_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_pages::list_structures(loom, ns, page_workspace_id))
    })
}
