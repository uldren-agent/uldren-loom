//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_interchange_io::{
    import_meetings_bytes, import_report_json, meetings_source_payload_path,
    parse_meetings_input_profile, validate_meetings_source_payload_leaf,
};

fn meetings_import_snapshot_ns(
    h: &LoomSession,
    workspace: &str,
    input_profile: &str,
    snapshot: &[u8],
    dry_run: bool,
) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    let profile = parse_meetings_input_profile(input_profile)?;
    let result = import_meetings_bytes(&mut loom, workspace_id, profile, snapshot, dry_run)?;
    import_report_json(&result.report)
}

fn meetings_source_read_ns(
    h: &LoomSession,
    workspace: &str,
    source_id: &str,
    leaf: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    validate_meetings_source_payload_leaf(leaf)?;
    let profile_id = workspace_id.to_string();
    let path = meetings_source_payload_path(&profile_id, source_id, leaf);
    loom.read_file_reserved(workspace_id, &path)
}

/// Import a normalized Meetings snapshot into an existing workspace and return the import report as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `input_profile` valid C strings;
/// `snapshot` null or readable for `snapshot_len` bytes; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_meetings_import_snapshot(
    handle: *mut LoomSession,
    workspace: *const c_char,
    input_profile: *const c_char,
    snapshot: *const c_uchar,
    snapshot_len: usize,
    dry_run: i32,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_meetings_import_snapshot");
    let workspace = arg_str!(workspace, "loom_meetings_import_snapshot");
    let input_profile = arg_str!(input_profile, "loom_meetings_import_snapshot");
    // SAFETY: caller guarantees `(snapshot, snapshot_len)` is readable when non-null.
    let bytes = unsafe { byte_slice(snapshot, snapshot_len) };
    match meetings_import_snapshot_ns(h, workspace, input_profile, bytes, dry_run != 0) {
        // SAFETY: `out` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out, &s) },
        Err(e) => fail(e),
    }
}

/// Read a retained Meetings source payload from a workspace.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`, `source_id`, and `leaf` valid C strings;
/// `out_ptr` and `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_meetings_source_read(
    handle: *mut LoomSession,
    workspace: *const c_char,
    source_id: *const c_char,
    leaf: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_meetings_source_read");
    let workspace = arg_str!(workspace, "loom_meetings_source_read");
    let source_id = arg_str!(source_id, "loom_meetings_source_read");
    let leaf = arg_str!(leaf, "loom_meetings_source_read");
    match meetings_source_read_ns(h, workspace, source_id, leaf) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}
