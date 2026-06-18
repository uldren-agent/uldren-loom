//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_drive::{
    HostedDriveConflictResolution, HostedDriveCreateUpload, HostedDriveGrantShare,
    HostedDrivePinRetention,
};

fn to_json<T: serde::Serialize>(value: loom_core::error::Result<T>) -> napi::Result<String> {
    let value = value.map_err(reason)?;
    serde_json::to_string(&value).map_err(|error| napi::Error::from_reason(error.to_string()))
}

fn parse_resolution(value: &str) -> napi::Result<HostedDriveConflictResolution> {
    match value {
        "keep_current" => Ok(HostedDriveConflictResolution::KeepCurrent),
        "keep_conflict" => Ok(HostedDriveConflictResolution::KeepConflict),
        "keep_both" => Ok(HostedDriveConflictResolution::KeepBoth),
        _ => Err(napi::Error::from_reason(
            "invalid drive conflict resolution",
        )),
    }
}

fn expires_ms(value: Option<BigInt>, what: &str) -> napi::Result<Option<u64>> {
    value.map(|value| bigint_to_u64(value, what)).transpose()
}

fn drive_read<T>(
    loom_path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> napi::Result<T>,
) -> napi::Result<T> {
    let loom = open_loom_read_unlocked(loom_path, key_spec(passphrase).as_ref()).map_err(reason)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    f(&loom, workspace_id)
}

fn drive_write<T>(
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

#[napi]
pub fn drive_list_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    folder_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::list_folder(
            loom,
            ns,
            &drive_workspace_id,
            &folder_id,
        ))
    })
}

#[napi]
pub fn drive_stat_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    folder_id: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::stat_node(
            loom,
            ns,
            &drive_workspace_id,
            &folder_id,
            &name,
        ))
    })
}

#[napi]
pub fn drive_read_file(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    file_id: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    drive_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        Ok(Uint8Array::from(
            loom_drive::read_file(loom, ns, &drive_workspace_id, &file_id).map_err(reason)?,
        ))
    })
}

#[napi]
pub fn drive_list_versions_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    file_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::list_versions(
            loom,
            ns,
            &drive_workspace_id,
            &file_id,
        ))
    })
}

#[napi]
pub fn drive_list_conflicts_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::list_conflicts(loom, ns, &drive_workspace_id))
    })
}

#[napi]
pub fn drive_list_shares_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::list_shares(loom, ns, &drive_workspace_id))
    })
}

#[napi]
pub fn drive_list_retention_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::list_retention(loom, ns, &drive_workspace_id))
    })
}

#[napi]
pub fn drive_create_folder_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    parent_folder_id: String,
    folder_id: String,
    name: String,
    expected_root: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::create_folder(
            loom,
            ns,
            &drive_workspace_id,
            &parent_folder_id,
            &folder_id,
            &name,
            &expected_root,
        ))
    })
}

#[napi]
pub fn drive_create_upload_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    upload_id: String,
    parent_folder_id: String,
    name: String,
    file_id: String,
    expected_root: String,
    created_at_ms: BigInt,
    replace_file: bool,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let created_at_ms = bigint_to_u64(created_at_ms, "created_at_ms")?;
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::create_upload(
            loom,
            ns,
            HostedDriveCreateUpload {
                workspace_id: &drive_workspace_id,
                upload_id: &upload_id,
                parent_folder_id: &parent_folder_id,
                name: &name,
                file_id: &file_id,
                expected_root: &expected_root,
                created_at_ms,
                replace_file,
            },
        ))
    })
}

#[napi]
pub fn drive_upload_chunk_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    upload_id: String,
    chunk: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::upload_chunk(
            loom,
            ns,
            &drive_workspace_id,
            &upload_id,
            &chunk,
        ))
    })
}

#[napi]
pub fn drive_commit_upload_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    upload_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::commit_upload(
            loom,
            ns,
            &drive_workspace_id,
            &upload_id,
        ))
    })
}

#[napi]
pub fn drive_rename_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    folder_id: String,
    node_id: String,
    new_name: String,
    expected_root: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::rename_node(
            loom,
            ns,
            &drive_workspace_id,
            &folder_id,
            &node_id,
            &new_name,
            &expected_root,
        ))
    })
}

#[napi]
pub fn drive_move_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    source_folder_id: String,
    target_folder_id: String,
    node_id: String,
    expected_root: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::move_node(
            loom,
            ns,
            &drive_workspace_id,
            &source_folder_id,
            &target_folder_id,
            &node_id,
            &expected_root,
        ))
    })
}

#[napi]
pub fn drive_delete_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    folder_id: String,
    node_id: String,
    expected_root: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::delete_node(
            loom,
            ns,
            &drive_workspace_id,
            &folder_id,
            &node_id,
            &expected_root,
        ))
    })
}

#[napi]
pub fn drive_resolve_conflict_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    conflict_id: String,
    resolution: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let resolution = parse_resolution(&resolution)?;
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::resolve_conflict(
            loom,
            ns,
            &drive_workspace_id,
            &conflict_id,
            resolution,
        ))
    })
}

#[napi]
pub fn drive_grant_share_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    grant_id: String,
    target_kind: String,
    target_id: String,
    principal: String,
    role: String,
    granted_at_ms: BigInt,
    expires_at_ms: Option<BigInt>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let granted_at_ms = bigint_to_u64(granted_at_ms, "granted_at_ms")?;
    let expires_at_ms = expires_ms(expires_at_ms, "expires_at_ms")?;
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::grant_share(
            loom,
            ns,
            HostedDriveGrantShare {
                workspace_id: &drive_workspace_id,
                grant_id: &grant_id,
                target_kind: &target_kind,
                target_id: &target_id,
                principal: &principal,
                role: &role,
                granted_at_ms,
                expires_at_ms,
            },
        ))
    })
}

#[napi]
pub fn drive_revoke_share_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    grant_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::revoke_share(
            loom,
            ns,
            &drive_workspace_id,
            &grant_id,
        ))
    })
}

#[napi]
pub fn drive_apply_share_expiry_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    now_ms: BigInt,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let now_ms = bigint_to_u64(now_ms, "now_ms")?;
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::apply_share_expiry(
            loom,
            ns,
            &drive_workspace_id,
            now_ms,
        ))
    })
}

#[napi]
pub fn drive_pin_retention_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    pin_id: String,
    kind: String,
    root: String,
    target_entity_id: Option<String>,
    added_at_ms: BigInt,
    expires_at_ms: Option<BigInt>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let added_at_ms = bigint_to_u64(added_at_ms, "added_at_ms")?;
    let expires_at_ms = expires_ms(expires_at_ms, "expires_at_ms")?;
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::pin_retention(
            loom,
            ns,
            HostedDrivePinRetention {
                workspace_id: &drive_workspace_id,
                pin_id: &pin_id,
                kind: &kind,
                root: &root,
                target_entity_id: target_entity_id.as_deref(),
                added_at_ms,
                expires_at_ms,
            },
        ))
    })
}

#[napi]
pub fn drive_unpin_retention_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    pin_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::unpin_retention(
            loom,
            ns,
            &drive_workspace_id,
            &pin_id,
        ))
    })
}

#[napi]
pub fn drive_apply_retention_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    now_ms: BigInt,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let now_ms = bigint_to_u64(now_ms, "now_ms")?;
    drive_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_drive::apply_retention(
            loom,
            ns,
            &drive_workspace_id,
            now_ms,
        ))
    })
}
