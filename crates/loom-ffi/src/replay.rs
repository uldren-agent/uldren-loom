//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// ---- history replay (cherry-pick / revert / rebase) ---------------------------------------------

/// Cherry-pick the comma-separated commit digests `commits` onto the current branch in workspace
/// `ns_name`, preserving each original author and message. `dry_run` non-zero
/// previews conflicts without changing anything. Writes an outcome JSON to `(*out_ptr)` (free with
/// [`loom_string_free`]): `{"outcome":"replayed","tip":"..."}`, `{"outcome":"clean"}`,
/// `{"outcome":"conflicts","paths":[...]}`, or `{"outcome":"empty"}`. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cherry_pick(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    commits: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cherry_pick");
    let (n, c) = (
        arg_str!(ns_name, "loom_cherry_pick"),
        arg_str!(commits, "loom_cherry_pick"),
    );
    match cherry_pick_ns(h, n, c, dry_run != 0) {
        // SAFETY: `out_ptr` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out_ptr, &s) },
        Err(e) => fail(e),
    }
}

/// Revert the comma-separated commit digests `commits` on the current branch in workspace `ns_name`
///, each as a new commit authored by `author`. `dry_run` non-zero previews conflicts.
/// Writes the same outcome JSON as [`loom_cherry_pick`] to `(*out_ptr)` (free with
/// [`loom_string_free`]). Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_revert(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    commits: *const c_char,
    author: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_revert");
    let (n, c, a) = (
        arg_str!(ns_name, "loom_revert"),
        arg_str!(commits, "loom_revert"),
        arg_str!(author, "loom_revert"),
    );
    match revert_ns(h, n, c, a, dry_run != 0) {
        // SAFETY: `out_ptr` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out_ptr, &s) },
        Err(e) => fail(e),
    }
}

/// Rebase the current branch in workspace `ns_name` onto the commit `onto` resolves
/// to (`HEAD`, a branch name, or a digest), replaying first-parent commits linearly. `dry_run` non-zero
/// previews conflicts. Writes the same outcome JSON as [`loom_cherry_pick`] to `(*out_ptr)` (free with
/// [`loom_string_free`]). Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_rebase(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    onto: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_rebase");
    let (n, o) = (
        arg_str!(ns_name, "loom_rebase"),
        arg_str!(onto, "loom_rebase"),
    );
    match rebase_ns(h, n, o, dry_run != 0) {
        // SAFETY: `out_ptr` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out_ptr, &s) },
        Err(e) => fail(e),
    }
}

/// Squash the commits after `onto` up to the current branch tip in workspace `ns_name` (required
/// `facet`) into one commit (parent `onto`, the tip's tree, `author`/`message`). `onto` must be an
/// ancestor of the tip and not the tip itself (`INVALID_ARGUMENT`). Writes the new commit digest to
/// `(*out_ptr)` (free with [`loom_string_free`]). Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_squash(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    onto: *const c_char,
    author: *const c_char,
    message: *const c_char,
    out_ptr: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_squash");
    let (n, o, a, m) = (
        arg_str!(ns_name, "loom_squash"),
        arg_str!(onto, "loom_squash"),
        arg_str!(author, "loom_squash"),
        arg_str!(message, "loom_squash"),
    );
    match squash_ns(h, n, o, a, m) {
        // SAFETY: `out_ptr` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out_ptr, &s) },
        Err(e) => fail(e),
    }
}

/// Workspace-level path blame for `branch` in workspace `ns_name`: each current path paired with the
/// commit that last set it, as canonical CBOR
/// (`{ kind: "PathBlame", paths: [{ path, commit }] }`) into `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]). This is the entry-level counterpart of [`loom_sql_blame`] (row-level).
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vcs_blame(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    branch: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vcs_blame");
    let (n, b) = (
        arg_str!(ns_name, "loom_vcs_blame"),
        arg_str!(branch, "loom_vcs_blame"),
    );
    match vcs_blame_ns(h, n, b) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Cross-facet structural diff between commits `from_hex` and `to_hex` (content addresses) in
/// workspace `ns_name`, as the canonical `LMDIFF` CBOR envelope into `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]).
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vcs_diff(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    from_hex: *const c_char,
    to_hex: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vcs_diff");
    let (n, f, to) = (
        arg_str!(ns_name, "loom_vcs_diff"),
        arg_str!(from_hex, "loom_vcs_diff"),
        arg_str!(to_hex, "loom_vcs_diff"),
    );
    match vcs_diff_ns(h, n, f, to) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Read the staged table `table` for workspace `ns_name` as canonical CBOR
/// (`{ "columns", "rows" }`) into `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]).
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_read_table(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    table: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_sql_read_table");
    let (n, tbl) = (
        arg_str!(ns_name, "loom_sql_read_table"),
        arg_str!(table, "loom_sql_read_table"),
    );
    match read_table_ns(h, n, tbl) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Read the table `table` from historical commit `commit_hex` without changing the working tree.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_read_table_at(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    table: *const c_char,
    commit_hex: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_sql_read_table_at");
    let (n, tbl, commit) = (
        arg_str!(ns_name, "loom_sql_read_table_at"),
        arg_str!(table, "loom_sql_read_table_at"),
        arg_str!(commit_hex, "loom_sql_read_table_at"),
    );
    match read_table_at_ns(h, n, tbl, commit) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Scan secondary index `index` on `table` for the lookup prefix at `(prefix_ptr, prefix_len)` - a
/// canonical-CBOR array of faithful cells, the same codec as the result. Writes the matching
/// rows as canonical CBOR (`{ "columns", "rows" }`) to `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]). An empty prefix is the canonical CBOR of an empty array.
///
/// # Safety
/// `handle` must be from [`loom_open`]; the name/table/index arguments valid C strings; `(prefix_ptr,
/// prefix_len)` a readable buffer; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_index_scan(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    table: *const c_char,
    index: *const c_char,
    prefix_ptr: *const c_uchar,
    prefix_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_sql_index_scan");
    let (n, tbl, idx) = (
        arg_str!(ns_name, "loom_sql_index_scan"),
        arg_str!(table, "loom_sql_index_scan"),
        arg_str!(index, "loom_sql_index_scan"),
    );
    // SAFETY: caller guarantees `(prefix_ptr, prefix_len)` is a readable buffer (see fn docs).
    let prefix = unsafe { byte_slice(prefix_ptr, prefix_len) };
    match index_scan_ns(h, n, tbl, idx, prefix) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Scan secondary index `index` on `table` from historical commit `commit_hex`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments valid C strings; `(prefix_ptr, prefix_len)` a
/// readable buffer; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_index_scan_at(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    table: *const c_char,
    index: *const c_char,
    prefix_ptr: *const c_uchar,
    prefix_len: usize,
    commit_hex: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_sql_index_scan_at");
    let (n, tbl, idx, commit) = (
        arg_str!(ns_name, "loom_sql_index_scan_at"),
        arg_str!(table, "loom_sql_index_scan_at"),
        arg_str!(index, "loom_sql_index_scan_at"),
        arg_str!(commit_hex, "loom_sql_index_scan_at"),
    );
    // SAFETY: caller guarantees `(prefix_ptr, prefix_len)` is a readable buffer (see fn docs).
    let prefix = unsafe { byte_slice(prefix_ptr, prefix_len) };
    match index_scan_at_ns(h, n, tbl, idx, prefix, commit) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Blame the rows of `table` on `branch` for workspace `ns_name`: each current
/// row plus the commit that last set it. Writes canonical CBOR (`{ "rows": [ { "commit", "values" } ] }`) to
/// `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]).
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_blame(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    branch: *const c_char,
    table: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_sql_blame");
    let (n, b, tbl) = (
        arg_str!(ns_name, "loom_sql_blame"),
        arg_str!(branch, "loom_sql_blame"),
        arg_str!(table, "loom_sql_blame"),
    );
    match blame_table_ns(h, n, b, tbl) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Schema-aware table diff between commits. Existing `loom_sql_diff` remains the row-only payload.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_table_diff(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    table: *const c_char,
    from_hex: *const c_char,
    to_hex: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_sql_table_diff");
    let (n, tbl, f, to) = (
        arg_str!(ns_name, "loom_sql_table_diff"),
        arg_str!(table, "loom_sql_table_diff"),
        arg_str!(from_hex, "loom_sql_table_diff"),
        arg_str!(to_hex, "loom_sql_table_diff"),
    );
    match diff_table_records_ns(h, n, tbl, f, to) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Row-level diff of `table` between commits `from_hex` and `to_hex` (content addresses). Writes
/// canonical CBOR (`{ "diffs": [...] }`) to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]).
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_sql_diff(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    table: *const c_char,
    from_hex: *const c_char,
    to_hex: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_sql_diff");
    let (n, tbl, f, to) = (
        arg_str!(ns_name, "loom_sql_diff"),
        arg_str!(table, "loom_sql_diff"),
        arg_str!(from_hex, "loom_sql_diff"),
        arg_str!(to_hex, "loom_sql_diff"),
    );
    match diff_table_ns(h, n, tbl, f, to) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}
