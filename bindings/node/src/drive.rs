//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::Drive as GeneratedDrive;

fn expires_ms(value: Option<BigInt>, what: &str) -> napi::Result<Option<u64>> {
    value.map(|value| bigint_to_u64(value, what)).transpose()
}

#[napi]
pub fn drive_list_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    folder_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_list_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            folder_id,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_stat_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            folder_id,
            name,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn drive_read_file(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    file_id: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_read_file(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            file_id,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn drive_list_versions_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    file_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_list_versions_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            file_id,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn drive_list_conflicts_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_list_conflicts_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn drive_list_shares_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_list_shares_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn drive_list_retention_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_list_retention_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_create_folder_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            parent_folder_id,
            folder_id,
            name,
            expected_root,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_create_upload_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            upload_id,
            parent_folder_id,
            name,
            file_id,
            expected_root,
            created_at_ms,
            replace_file,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_upload_chunk_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            upload_id,
            chunk.to_vec(),
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn drive_commit_upload_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    upload_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_commit_upload_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            upload_id,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_rename_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            folder_id,
            node_id,
            new_name,
            expected_root,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_move_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            source_folder_id,
            target_folder_id,
            node_id,
            expected_root,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_delete_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            folder_id,
            node_id,
            expected_root,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_resolve_conflict_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            conflict_id,
            resolution,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_grant_share_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            grant_id,
            target_kind,
            target_id,
            principal,
            role,
            granted_at_ms,
            expires_at_ms,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn drive_revoke_share_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    grant_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_revoke_share_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            grant_id,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_apply_share_expiry_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            now_ms,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_pin_retention_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            pin_id,
            kind,
            root,
            target_entity_id,
            added_at_ms,
            expires_at_ms,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn drive_unpin_retention_json(
    loom_path: String,
    workspace: String,
    drive_workspace_id: String,
    pin_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_unpin_retention_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            pin_id,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedDrive>::drive_apply_retention_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            drive_workspace_id,
            now_ms,
        ),
    )
    .map_err(reason)
}
