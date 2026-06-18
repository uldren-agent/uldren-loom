//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_drive::{
    HostedDriveConflictResolution, HostedDriveCreateUpload, HostedDriveGrantShare,
    HostedDrivePinRetention,
};

unsafe fn optional_str_arg<'a>(value: *const c_char, what: &str) -> LoomResult<Option<&'a str>> {
    if value.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|_| LoomError::invalid(format!("{what}: invalid UTF-8")))?;
    Ok((!value.is_empty()).then_some(value))
}

fn json_string<T: serde::Serialize>(value: &T) -> LoomResult<String> {
    serde_json::to_string(value).map_err(|err| LoomError::invalid(err.to_string()))
}

fn parse_conflict_resolution(value: &str) -> LoomResult<HostedDriveConflictResolution> {
    match value {
        "current" | "keep-current" | "keep_current" => {
            Ok(HostedDriveConflictResolution::KeepCurrent)
        }
        "conflict" | "keep-conflict" | "keep_conflict" => {
            Ok(HostedDriveConflictResolution::KeepConflict)
        }
        "both" | "keep-both" | "keep_both" => Ok(HostedDriveConflictResolution::KeepBoth),
        _ => Err(LoomError::invalid(
            "drive conflict resolution must be current, conflict, or both",
        )),
    }
}

fn drive_read_loom<T>(
    h: &LoomSession,
    workspace: &str,
    f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> LoomResult<T>,
) -> LoomResult<T> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    f(&loom, ns)
}

fn drive_write_loom<T>(
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

fn json_result<T: serde::Serialize>(result: LoomResult<T>) -> LoomResult<String> {
    json_string(&result?)
}

macro_rules! out_json {
    ($out:ident, $result:expr) => {
        match $result {
            Ok(s) => unsafe { ok_str($out, &s) },
            Err(e) => fail(e),
        }
    };
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
        drive_read_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::list_folder(loom, ns, workspace_id, folder_id))
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
        drive_read_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::stat_node(
                loom,
                ns,
                workspace_id,
                folder_id,
                name,
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
    match drive_read_loom(h, workspace, |loom, ns| {
        loom_drive::read_file(loom, ns, workspace_id, file_id)
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
        drive_read_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::list_versions(loom, ns, workspace_id, file_id))
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
        drive_read_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::list_conflicts(loom, ns, workspace_id))
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
        drive_read_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::list_shares(loom, ns, workspace_id))
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
        drive_read_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::list_retention(loom, ns, workspace_id))
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
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::create_folder(
                loom,
                ns,
                workspace_id,
                parent_folder_id,
                folder_id,
                name,
                expected_root,
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
    let h = handle_ref!(handle, "loom_drive_create_upload_json");
    let workspace = arg_str!(workspace, "loom_drive_create_upload_json");
    let request = HostedDriveCreateUpload {
        workspace_id: arg_str!(workspace_id, "loom_drive_create_upload_json"),
        upload_id: arg_str!(upload_id, "loom_drive_create_upload_json"),
        parent_folder_id: arg_str!(parent_folder_id, "loom_drive_create_upload_json"),
        name: arg_str!(name, "loom_drive_create_upload_json"),
        file_id: arg_str!(file_id, "loom_drive_create_upload_json"),
        expected_root: arg_str!(expected_root, "loom_drive_create_upload_json"),
        created_at_ms,
        replace_file: replace_file != 0,
    };
    out_json!(
        out,
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::create_upload(loom, ns, request))
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
    let h = handle_ref!(handle, "loom_drive_upload_chunk_json");
    let workspace = arg_str!(workspace, "loom_drive_upload_chunk_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_upload_chunk_json");
    let upload_id = arg_str!(upload_id, "loom_drive_upload_chunk_json");
    // SAFETY: caller guarantees `(chunk, chunk_len)` is readable when non-null.
    let chunk = unsafe { byte_slice(chunk, chunk_len) };
    out_json!(
        out,
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::upload_chunk(
                loom,
                ns,
                workspace_id,
                upload_id,
                chunk,
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
    let h = handle_ref!(handle, "loom_drive_commit_upload_json");
    let workspace = arg_str!(workspace, "loom_drive_commit_upload_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_commit_upload_json");
    let upload_id = arg_str!(upload_id, "loom_drive_commit_upload_json");
    out_json!(
        out,
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::commit_upload(loom, ns, workspace_id, upload_id))
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
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::rename_node(
                loom,
                ns,
                workspace_id,
                folder_id,
                node_id,
                new_name,
                expected_root,
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
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::move_node(
                loom,
                ns,
                workspace_id,
                source_folder_id,
                target_folder_id,
                node_id,
                expected_root,
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
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::delete_node(
                loom,
                ns,
                workspace_id,
                folder_id,
                node_id,
                expected_root,
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
    let h = handle_ref!(handle, "loom_drive_resolve_conflict_json");
    let workspace = arg_str!(workspace, "loom_drive_resolve_conflict_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_resolve_conflict_json");
    let conflict_id = arg_str!(conflict_id, "loom_drive_resolve_conflict_json");
    let resolution =
        match parse_conflict_resolution(arg_str!(resolution, "loom_drive_resolve_conflict_json")) {
            Ok(resolution) => resolution,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::resolve_conflict(
                loom,
                ns,
                workspace_id,
                conflict_id,
                resolution,
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
    let h = handle_ref!(handle, "loom_drive_grant_share_json");
    let workspace = arg_str!(workspace, "loom_drive_grant_share_json");
    let request = HostedDriveGrantShare {
        workspace_id: arg_str!(workspace_id, "loom_drive_grant_share_json"),
        grant_id: arg_str!(grant_id, "loom_drive_grant_share_json"),
        target_kind: arg_str!(target_kind, "loom_drive_grant_share_json"),
        target_id: arg_str!(target_id, "loom_drive_grant_share_json"),
        principal: arg_str!(principal, "loom_drive_grant_share_json"),
        role: arg_str!(role, "loom_drive_grant_share_json"),
        granted_at_ms,
        expires_at_ms: (has_expires_at_ms != 0).then_some(expires_at_ms),
    };
    out_json!(
        out,
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::grant_share(loom, ns, request))
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
    let h = handle_ref!(handle, "loom_drive_revoke_share_json");
    let workspace = arg_str!(workspace, "loom_drive_revoke_share_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_revoke_share_json");
    let grant_id = arg_str!(grant_id, "loom_drive_revoke_share_json");
    out_json!(
        out,
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::revoke_share(loom, ns, workspace_id, grant_id))
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
    let h = handle_ref!(handle, "loom_drive_apply_share_expiry_json");
    let workspace = arg_str!(workspace, "loom_drive_apply_share_expiry_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_apply_share_expiry_json");
    out_json!(
        out,
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::apply_share_expiry(
                loom,
                ns,
                workspace_id,
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
    let h = handle_ref!(handle, "loom_drive_pin_retention_json");
    let workspace = arg_str!(workspace, "loom_drive_pin_retention_json");
    let request = HostedDrivePinRetention {
        workspace_id: arg_str!(workspace_id, "loom_drive_pin_retention_json"),
        pin_id: arg_str!(pin_id, "loom_drive_pin_retention_json"),
        kind: arg_str!(kind, "loom_drive_pin_retention_json"),
        root: arg_str!(root, "loom_drive_pin_retention_json"),
        // SAFETY: `target_entity_id` is optional and must be null or a valid C string per fn docs.
        target_entity_id: match unsafe {
            optional_str_arg(target_entity_id, "loom_drive_pin_retention_json")
        } {
            Ok(value) => value,
            Err(e) => return fail(e),
        },
        added_at_ms,
        expires_at_ms: (has_expires_at_ms != 0).then_some(expires_at_ms),
    };
    out_json!(
        out,
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::pin_retention(loom, ns, request))
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
    let h = handle_ref!(handle, "loom_drive_unpin_retention_json");
    let workspace = arg_str!(workspace, "loom_drive_unpin_retention_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_unpin_retention_json");
    let pin_id = arg_str!(pin_id, "loom_drive_unpin_retention_json");
    out_json!(
        out,
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::unpin_retention(loom, ns, workspace_id, pin_id))
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
    let h = handle_ref!(handle, "loom_drive_apply_retention_json");
    let workspace = arg_str!(workspace, "loom_drive_apply_retention_json");
    let workspace_id = arg_str!(workspace_id, "loom_drive_apply_retention_json");
    out_json!(
        out,
        drive_write_loom(h, workspace, |loom, ns| {
            json_result(loom_drive::apply_retention(loom, ns, workspace_id, now_ms))
        })
    )
}
