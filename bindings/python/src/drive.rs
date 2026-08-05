//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::Drive as GeneratedDrive;

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, folder_id, passphrase=None))]
pub(crate) fn drive_list_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    folder_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_list_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            folder_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, folder_id, name, passphrase=None))]
pub(crate) fn drive_stat_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    folder_id: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_stat_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            folder_id.to_string(),
            name.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, file_id, passphrase=None))]
pub(crate) fn drive_read_file<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    file_id: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    let bytes = block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_read_file(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            file_id.to_string(),
        ),
    )
    .map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, file_id, passphrase=None))]
pub(crate) fn drive_list_versions_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    file_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_list_versions_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            file_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, passphrase=None))]
pub(crate) fn drive_list_conflicts_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_list_conflicts_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, passphrase=None))]
pub(crate) fn drive_list_shares_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_list_shares_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, passphrase=None))]
pub(crate) fn drive_list_retention_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_list_retention_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, parent_folder_id, folder_id, name, expected_root, passphrase=None))]
pub(crate) fn drive_create_folder_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    parent_folder_id: &str,
    folder_id: &str,
    name: &str,
    expected_root: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_create_folder_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            parent_folder_id.to_string(),
            folder_id.to_string(),
            name.to_string(),
            expected_root.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, upload_id, parent_folder_id, name, file_id, expected_root, created_at_ms, replace_file, passphrase=None))]
pub(crate) fn drive_create_upload_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    upload_id: &str,
    parent_folder_id: &str,
    name: &str,
    file_id: &str,
    expected_root: &str,
    created_at_ms: u64,
    replace_file: bool,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_create_upload_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            upload_id.to_string(),
            parent_folder_id.to_string(),
            name.to_string(),
            file_id.to_string(),
            expected_root.to_string(),
            created_at_ms,
            replace_file,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, upload_id, chunk, passphrase=None))]
pub(crate) fn drive_upload_chunk_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    upload_id: &str,
    chunk: &[u8],
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_upload_chunk_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            upload_id.to_string(),
            chunk.to_vec(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, upload_id, passphrase=None))]
pub(crate) fn drive_commit_upload_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    upload_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_commit_upload_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            upload_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, folder_id, node_id, new_name, expected_root, passphrase=None))]
pub(crate) fn drive_rename_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    folder_id: &str,
    node_id: &str,
    new_name: &str,
    expected_root: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_rename_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            folder_id.to_string(),
            node_id.to_string(),
            new_name.to_string(),
            expected_root.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, source_folder_id, target_folder_id, node_id, expected_root, passphrase=None))]
pub(crate) fn drive_move_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    source_folder_id: &str,
    target_folder_id: &str,
    node_id: &str,
    expected_root: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_move_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            source_folder_id.to_string(),
            target_folder_id.to_string(),
            node_id.to_string(),
            expected_root.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, folder_id, node_id, expected_root, passphrase=None))]
pub(crate) fn drive_delete_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    folder_id: &str,
    node_id: &str,
    expected_root: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_delete_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            folder_id.to_string(),
            node_id.to_string(),
            expected_root.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, conflict_id, resolution, passphrase=None))]
pub(crate) fn drive_resolve_conflict_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    conflict_id: &str,
    resolution: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_resolve_conflict_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            conflict_id.to_string(),
            resolution.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, grant_id, target_kind, target_id, principal, role, granted_at_ms, expires_at_ms=None, passphrase=None))]
pub(crate) fn drive_grant_share_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    grant_id: &str,
    target_kind: &str,
    target_id: &str,
    principal: &str,
    role: &str,
    granted_at_ms: u64,
    expires_at_ms: Option<u64>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_grant_share_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            grant_id.to_string(),
            target_kind.to_string(),
            target_id.to_string(),
            principal.to_string(),
            role.to_string(),
            granted_at_ms,
            expires_at_ms,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, grant_id, passphrase=None))]
pub(crate) fn drive_revoke_share_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    grant_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_revoke_share_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            grant_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, now_ms, passphrase=None))]
pub(crate) fn drive_apply_share_expiry_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    now_ms: u64,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_apply_share_expiry_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            now_ms,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, pin_id, kind, root, target_entity_id, added_at_ms, expires_at_ms=None, passphrase=None))]
pub(crate) fn drive_pin_retention_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    pin_id: &str,
    kind: &str,
    root: &str,
    target_entity_id: Option<&str>,
    added_at_ms: u64,
    expires_at_ms: Option<u64>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_pin_retention_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            pin_id.to_string(),
            kind.to_string(),
            root.to_string(),
            target_entity_id.map(str::to_string),
            added_at_ms,
            expires_at_ms,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, pin_id, passphrase=None))]
pub(crate) fn drive_unpin_retention_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    pin_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_unpin_retention_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            pin_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, now_ms, passphrase=None))]
pub(crate) fn drive_apply_retention_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    now_ms: u64,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_apply_retention_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            drive_workspace_id.to_string(),
            now_ms,
        ),
    )
    .map_err(py_err)
}
