//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// ---- restore / path-restricted checkout ---------------------------------------------------------

/// Restore one `path` in workspace `ns_name` to the snapshot `rev` resolves to
/// (`HEAD`, a branch name, or a digest); absent in the snapshot removes it. Working tree only - `HEAD`,
/// the branch, and the index are unchanged. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_restore_file(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    rev: *const c_char,
    path: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_restore_file");
    let (n, r, p) = (
        arg_str!(ns_name, "loom_restore_file"),
        arg_str!(rev, "loom_restore_file"),
        arg_str!(path, "loom_restore_file"),
    );
    match restore_file_ns(h, n, r, p) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Restore the subtree under `prefix` in workspace `ns_name` to the snapshot `rev`
/// resolves to (path-restricted checkout; `prefix` `""` restores the whole tree). Working tree only.
/// Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_restore_path(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    rev: *const c_char,
    prefix: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_restore_path");
    let (n, r, p) = (
        arg_str!(ns_name, "loom_restore_path"),
        arg_str!(rev, "loom_restore_path"),
        arg_str!(prefix, "loom_restore_path"),
    );
    match restore_path_ns(h, n, r, p) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}
