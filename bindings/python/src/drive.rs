//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_drive::{
    HostedDriveConflictResolution, HostedDriveCreateUpload, HostedDriveGrantShare,
    HostedDrivePinRetention,
};

fn to_json<T: serde::Serialize>(value: loom_core::error::Result<T>) -> PyResult<String> {
    let value = value.map_err(py_err)?;
    serde_json::to_string(&value).map_err(|error| PyRuntimeError::new_err(error.to_string()))
}

fn parse_resolution(value: &str) -> PyResult<HostedDriveConflictResolution> {
    match value {
        "keep_current" => Ok(HostedDriveConflictResolution::KeepCurrent),
        "keep_conflict" => Ok(HostedDriveConflictResolution::KeepConflict),
        "keep_both" => Ok(HostedDriveConflictResolution::KeepBoth),
        _ => Err(PyRuntimeError::new_err("invalid drive conflict resolution")),
    }
}

fn drive_read<T>(
    path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> PyResult<T>,
) -> PyResult<T> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    f(&loom, workspace_id)
}

fn drive_write<T>(
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

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, folder_id, passphrase=None))]
pub(crate) fn drive_list_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    folder_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    drive_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::list_folder(
            loom,
            ns,
            drive_workspace_id,
            folder_id,
        ))
    })
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
    drive_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::stat_node(
            loom,
            ns,
            drive_workspace_id,
            folder_id,
            name,
        ))
    })
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
    let bytes = drive_read(path, workspace, passphrase, |loom, ns| {
        loom_drive::read_file(loom, ns, drive_workspace_id, file_id).map_err(py_err)
    })?;
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
    drive_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::list_versions(
            loom,
            ns,
            drive_workspace_id,
            file_id,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, passphrase=None))]
pub(crate) fn drive_list_conflicts_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    drive_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::list_conflicts(loom, ns, drive_workspace_id))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, passphrase=None))]
pub(crate) fn drive_list_shares_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    drive_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::list_shares(loom, ns, drive_workspace_id))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, drive_workspace_id, passphrase=None))]
pub(crate) fn drive_list_retention_json(
    path: &str,
    workspace: &str,
    drive_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    drive_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::list_retention(loom, ns, drive_workspace_id))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::create_folder(
            loom,
            ns,
            drive_workspace_id,
            parent_folder_id,
            folder_id,
            name,
            expected_root,
        ))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::create_upload(
            loom,
            ns,
            HostedDriveCreateUpload {
                workspace_id: drive_workspace_id,
                upload_id,
                parent_folder_id,
                name,
                file_id,
                expected_root,
                created_at_ms,
                replace_file,
            },
        ))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::upload_chunk(
            loom,
            ns,
            drive_workspace_id,
            upload_id,
            chunk,
        ))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::commit_upload(
            loom,
            ns,
            drive_workspace_id,
            upload_id,
        ))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::rename_node(
            loom,
            ns,
            drive_workspace_id,
            folder_id,
            node_id,
            new_name,
            expected_root,
        ))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::move_node(
            loom,
            ns,
            drive_workspace_id,
            source_folder_id,
            target_folder_id,
            node_id,
            expected_root,
        ))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::delete_node(
            loom,
            ns,
            drive_workspace_id,
            folder_id,
            node_id,
            expected_root,
        ))
    })
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
    let resolution = parse_resolution(resolution)?;
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::resolve_conflict(
            loom,
            ns,
            drive_workspace_id,
            conflict_id,
            resolution,
        ))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::grant_share(
            loom,
            ns,
            HostedDriveGrantShare {
                workspace_id: drive_workspace_id,
                grant_id,
                target_kind,
                target_id,
                principal,
                role,
                granted_at_ms,
                expires_at_ms,
            },
        ))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::revoke_share(
            loom,
            ns,
            drive_workspace_id,
            grant_id,
        ))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::apply_share_expiry(
            loom,
            ns,
            drive_workspace_id,
            now_ms,
        ))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::pin_retention(
            loom,
            ns,
            HostedDrivePinRetention {
                workspace_id: drive_workspace_id,
                pin_id,
                kind,
                root,
                target_entity_id,
                added_at_ms,
                expires_at_ms,
            },
        ))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::unpin_retention(
            loom,
            ns,
            drive_workspace_id,
            pin_id,
        ))
    })
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
    drive_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_drive::apply_retention(
            loom,
            ns,
            drive_workspace_id,
            now_ms,
        ))
    })
}
