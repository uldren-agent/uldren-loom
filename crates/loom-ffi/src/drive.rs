//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_client::generated_api::Drive;

unsafe fn optional_str_arg<'a>(value: *const c_char, what: &str) -> LoomResult<Option<&'a str>> {
    if value.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|_| LoomError::invalid(format!("{what}: invalid UTF-8")))?;
    Ok(Some(value))
}

macro_rules! out_json {
    ($out:ident, $result:expr) => {
        match $result {
            Ok(s) => unsafe { ok_str($out, &s) },
            Err(e) => fail(e),
        }
    };
}

macro_rules! require_json_out {
    ($out:ident, $what:literal) => {
        if $out.is_null() {
            return fail_arg(concat!($what, ": null out"));
        }
    };
}

fn drive_generated_string(
    h: &LoomSession,
    f: impl FnOnce(&loom_client::LocalLoomClient, loom_client::types::LoomSession) -> LoomResult<String>,
) -> LoomResult<String> {
    with_generated_client(h, f)
}

fn drive_generated_bytes(
    h: &LoomSession,
    f: impl FnOnce(
        &loom_client::LocalLoomClient,
        loom_client::types::LoomSession,
    ) -> LoomResult<Vec<u8>>,
) -> LoomResult<Vec<u8>> {
    with_generated_client(h, f)
}

/// List a Drive folder as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_list_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    folder_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_drive_list_json");
    let workspace = arg_str!(workspace, "loom_drive_list_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_list_json");
    let folder_id = arg_str!(folder_id, "loom_drive_list_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_list_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                folder_id.to_string(),
            ))
        })
    )
}

/// Read Drive metadata for a named folder entry as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_stat_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    folder_id: *const c_char,
    name: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_drive_stat_json");
    let workspace = arg_str!(workspace, "loom_drive_stat_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_stat_json");
    let folder_id = arg_str!(folder_id, "loom_drive_stat_json");
    let name = arg_str!(name, "loom_drive_stat_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_stat_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                folder_id.to_string(),
                name.to_string(),
            ))
        })
    )
}

/// Read the latest Drive file bytes.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; output pointers writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_read(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    file_id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_drive_read");
    let workspace = arg_str!(workspace, "loom_drive_read");
    let workspace_id = arg_str!(workspace_id, "loom_drive_read");
    let file_id = arg_str!(file_id, "loom_drive_read");
    match drive_generated_bytes(h, |client, session| {
        crate::generated_local::block_generated(Drive::drive_read_file(
            client,
            session,
            workspace.to_string(),
            workspace_id.to_string(),
            file_id.to_string(),
        ))
    }) {
        // SAFETY: output pointers are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// List Drive file versions as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_list_versions_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    file_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_drive_list_versions_json");
    let workspace = arg_str!(workspace, "loom_drive_list_versions_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_list_versions_json");
    let file_id = arg_str!(file_id, "loom_drive_list_versions_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_list_versions_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                file_id.to_string(),
            ))
        })
    )
}

/// List Drive conflicts as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_list_conflicts_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_drive_list_conflicts_json");
    let workspace = arg_str!(workspace, "loom_drive_list_conflicts_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_list_conflicts_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_list_conflicts_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
            ))
        })
    )
}

/// List Drive share grants as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_list_shares_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_drive_list_shares_json");
    let workspace = arg_str!(workspace, "loom_drive_list_shares_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_list_shares_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_list_shares_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
            ))
        })
    )
}

/// List Drive retention pins as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_list_retention_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_drive_list_retention_json");
    let workspace = arg_str!(workspace, "loom_drive_list_retention_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_list_retention_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_list_retention_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
            ))
        })
    )
}

/// Create a Drive folder and return the write summary as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_create_folder_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    parent_folder_id: *const c_char,
    folder_id: *const c_char,
    name: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_drive_create_folder_json");
    let workspace = arg_str!(workspace, "loom_drive_create_folder_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_create_folder_json");
    let parent_folder_id = arg_str!(parent_folder_id, "loom_drive_create_folder_json");
    let folder_id = arg_str!(folder_id, "loom_drive_create_folder_json");
    let name = arg_str!(name, "loom_drive_create_folder_json");
    let expected_root = arg_str!(expected_root, "loom_drive_create_folder_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_create_folder_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                parent_folder_id.to_string(),
                folder_id.to_string(),
                name.to_string(),
                expected_root.to_string(),
            ))
        })
    )
}

/// Create a Drive upload session and return the session summary as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_create_upload_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    upload_id: *const c_char,
    parent_folder_id: *const c_char,
    name: *const c_char,
    file_id: *const c_char,
    expected_root: *const c_char,
    created_at_ms: u64,
    replace_file: i32,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_drive_create_upload_json");
    let h = handle_ref!(handle, "loom_drive_create_upload_json");
    let workspace = arg_str!(workspace, "loom_drive_create_upload_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_create_upload_json");
    let upload_id = arg_str!(upload_id, "loom_drive_create_upload_json");
    let parent_folder_id = arg_str!(parent_folder_id, "loom_drive_create_upload_json");
    let name = arg_str!(name, "loom_drive_create_upload_json");
    let file_id = arg_str!(file_id, "loom_drive_create_upload_json");
    let expected_root = arg_str!(expected_root, "loom_drive_create_upload_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_create_upload_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                upload_id.to_string(),
                parent_folder_id.to_string(),
                name.to_string(),
                file_id.to_string(),
                expected_root.to_string(),
                created_at_ms,
                replace_file != 0,
            ))
        })
    )
}

/// Append a Drive upload chunk and return the session summary as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `chunk` readable for
/// `chunk_len`; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_upload_chunk_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    upload_id: *const c_char,
    chunk: *const c_uchar,
    chunk_len: usize,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_drive_upload_chunk_json");
    let h = handle_ref!(handle, "loom_drive_upload_chunk_json");
    let workspace = arg_str!(workspace, "loom_drive_upload_chunk_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_upload_chunk_json");
    let upload_id = arg_str!(upload_id, "loom_drive_upload_chunk_json");
    // SAFETY: caller guarantees `(chunk, chunk_len)` is readable when non-null.
    let chunk = unsafe { byte_slice(chunk, chunk_len) };
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_upload_chunk_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                upload_id.to_string(),
                chunk.to_vec(),
            ))
        })
    )
}

/// Commit a Drive upload and return the write summary as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_commit_upload_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    upload_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_drive_commit_upload_json");
    let h = handle_ref!(handle, "loom_drive_commit_upload_json");
    let workspace = arg_str!(workspace, "loom_drive_commit_upload_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_commit_upload_json");
    let upload_id = arg_str!(upload_id, "loom_drive_commit_upload_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_commit_upload_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                upload_id.to_string(),
            ))
        })
    )
}

/// Rename a Drive node and return the write summary as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_rename_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    folder_id: *const c_char,
    node_id: *const c_char,
    new_name: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_drive_rename_json");
    let workspace = arg_str!(workspace, "loom_drive_rename_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_rename_json");
    let folder_id = arg_str!(folder_id, "loom_drive_rename_json");
    let node_id = arg_str!(node_id, "loom_drive_rename_json");
    let new_name = arg_str!(new_name, "loom_drive_rename_json");
    let expected_root = arg_str!(expected_root, "loom_drive_rename_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_rename_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                folder_id.to_string(),
                node_id.to_string(),
                new_name.to_string(),
                expected_root.to_string(),
            ))
        })
    )
}

/// Move a Drive node and return the write summary as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_move_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    source_folder_id: *const c_char,
    target_folder_id: *const c_char,
    node_id: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_drive_move_json");
    let workspace = arg_str!(workspace, "loom_drive_move_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_move_json");
    let source_folder_id = arg_str!(source_folder_id, "loom_drive_move_json");
    let target_folder_id = arg_str!(target_folder_id, "loom_drive_move_json");
    let node_id = arg_str!(node_id, "loom_drive_move_json");
    let expected_root = arg_str!(expected_root, "loom_drive_move_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_move_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                source_folder_id.to_string(),
                target_folder_id.to_string(),
                node_id.to_string(),
                expected_root.to_string(),
            ))
        })
    )
}

/// Delete a Drive node and return the write summary as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_delete_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    folder_id: *const c_char,
    node_id: *const c_char,
    expected_root: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_drive_delete_json");
    let workspace = arg_str!(workspace, "loom_drive_delete_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_delete_json");
    let folder_id = arg_str!(folder_id, "loom_drive_delete_json");
    let node_id = arg_str!(node_id, "loom_drive_delete_json");
    let expected_root = arg_str!(expected_root, "loom_drive_delete_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_delete_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                folder_id.to_string(),
                node_id.to_string(),
                expected_root.to_string(),
            ))
        })
    )
}

/// Resolve a Drive conflict and return the write summary as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_resolve_conflict_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    conflict_id: *const c_char,
    resolution: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_drive_resolve_conflict_json");
    let h = handle_ref!(handle, "loom_drive_resolve_conflict_json");
    let workspace = arg_str!(workspace, "loom_drive_resolve_conflict_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_resolve_conflict_json");
    let conflict_id = arg_str!(conflict_id, "loom_drive_resolve_conflict_json");
    let resolution = arg_str!(resolution, "loom_drive_resolve_conflict_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_resolve_conflict_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                conflict_id.to_string(),
                resolution.to_string(),
            ))
        })
    )
}

/// Grant Drive sharing and return the write summary as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_grant_share_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    grant_id: *const c_char,
    target_kind: *const c_char,
    target_id: *const c_char,
    principal: *const c_char,
    role: *const c_char,
    granted_at_ms: u64,
    expires_at_ms: u64,
    has_expires_at_ms: i32,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_drive_grant_share_json");
    let h = handle_ref!(handle, "loom_drive_grant_share_json");
    let workspace = arg_str!(workspace, "loom_drive_grant_share_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_grant_share_json");
    let grant_id = arg_str!(grant_id, "loom_drive_grant_share_json");
    let target_kind = arg_str!(target_kind, "loom_drive_grant_share_json");
    let target_id = arg_str!(target_id, "loom_drive_grant_share_json");
    let principal = arg_str!(principal, "loom_drive_grant_share_json");
    let role = arg_str!(role, "loom_drive_grant_share_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_grant_share_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                grant_id.to_string(),
                target_kind.to_string(),
                target_id.to_string(),
                principal.to_string(),
                role.to_string(),
                granted_at_ms,
                (has_expires_at_ms != 0).then_some(expires_at_ms),
            ))
        })
    )
}

/// Revoke Drive sharing and return the write summary as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_revoke_share_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    grant_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_drive_revoke_share_json");
    let h = handle_ref!(handle, "loom_drive_revoke_share_json");
    let workspace = arg_str!(workspace, "loom_drive_revoke_share_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_revoke_share_json");
    let grant_id = arg_str!(grant_id, "loom_drive_revoke_share_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_revoke_share_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                grant_id.to_string(),
            ))
        })
    )
}

/// Apply Drive share expiry and return a JSON summary.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_apply_share_expiry_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    now_ms: u64,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_drive_apply_share_expiry_json");
    let h = handle_ref!(handle, "loom_drive_apply_share_expiry_json");
    let workspace = arg_str!(workspace, "loom_drive_apply_share_expiry_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_apply_share_expiry_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_apply_share_expiry_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                now_ms,
            ))
        })
    )
}

/// Pin Drive retention and return the write summary as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; optional string null or valid;
/// `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_pin_retention_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    pin_id: *const c_char,
    kind: *const c_char,
    root: *const c_char,
    target_entity_id: *const c_char,
    added_at_ms: u64,
    expires_at_ms: u64,
    has_expires_at_ms: i32,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_drive_pin_retention_json");
    let h = handle_ref!(handle, "loom_drive_pin_retention_json");
    let workspace = arg_str!(workspace, "loom_drive_pin_retention_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_pin_retention_json");
    let pin_id = arg_str!(pin_id, "loom_drive_pin_retention_json");
    let kind = arg_str!(kind, "loom_drive_pin_retention_json");
    let root = arg_str!(root, "loom_drive_pin_retention_json");
    let target_entity_id =
        match unsafe { optional_str_arg(target_entity_id, "loom_drive_pin_retention_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_pin_retention_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                pin_id.to_string(),
                kind.to_string(),
                root.to_string(),
                target_entity_id.map(str::to_string),
                added_at_ms,
                (has_expires_at_ms != 0).then_some(expires_at_ms),
            ))
        })
    )
}

/// Remove a Drive retention pin and return the write summary as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_unpin_retention_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    pin_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_drive_unpin_retention_json");
    let h = handle_ref!(handle, "loom_drive_unpin_retention_json");
    let workspace = arg_str!(workspace, "loom_drive_unpin_retention_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_unpin_retention_json");
    let pin_id = arg_str!(pin_id, "loom_drive_unpin_retention_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_unpin_retention_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                pin_id.to_string(),
            ))
        })
    )
}

/// Apply Drive retention expiry and return a JSON summary.
///
/// # Safety
/// `handle` must be from [`loom_open`]; strings must be valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_drive_apply_retention_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    workspace_id: *const c_char,
    now_ms: u64,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_drive_apply_retention_json");
    let h = handle_ref!(handle, "loom_drive_apply_retention_json");
    let workspace = arg_str!(workspace, "loom_drive_apply_retention_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_apply_retention_json");
    out_json!(
        out,
        drive_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Drive::drive_apply_retention_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                now_ms,
            ))
        })
    )
}
