//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// ---------------------------------------------------------------------------------------------------
// Append-log queue (Queue facet) - append/get/range/len over a workspace stream, by UUID or name.
// ---------------------------------------------------------------------------------------------------

/// Resolve a workspace for a queue write by UUID or name, ensuring the `queue` facet exists. A name not
/// yet present is created carrying the `queue` facet; an unknown UUID is `NOT_FOUND`.
fn ensure_queue_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Queue,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Queue)?;
    Ok(ns)
}

/// Reject empty stream names and path-traversal forms so the public queue API never writes an arbitrary
/// path under the queue facet.
fn validate_stream_name(name: &str) -> LoomResult<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(LoomError::invalid(format!("invalid stream name {name:?}")));
    }
    Ok(())
}

fn queue_append_ns(
    h: &LoomSession,
    workspace: &str,
    stream: &str,
    entry: &[u8],
) -> LoomResult<u64> {
    validate_stream_name(stream)?;
    let mut loom = open_h_write(h)?;
    let ns = ensure_queue_ns(&mut loom, workspace)?;
    let seq = loom.stream_append(ns, stream, entry)?;
    save_loom(&mut loom)?;
    Ok(seq as u64)
}

fn queue_get_ns(
    h: &LoomSession,
    workspace: &str,
    stream: &str,
    seq: usize,
) -> LoomResult<Option<Vec<u8>>> {
    validate_stream_name(stream)?;
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom.stream_get(ns, stream, seq)
}

fn queue_range_ns(
    h: &LoomSession,
    workspace: &str,
    stream: &str,
    lo: usize,
    hi: usize,
) -> LoomResult<Vec<u8>> {
    validate_stream_name(stream)?;
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let mut encoded = Stream::new();
    for entry in loom.stream_range(ns, stream, lo, hi)? {
        encoded.append(entry);
    }
    Ok(encoded.encode())
}

fn queue_len_ns(h: &LoomSession, workspace: &str, stream: &str) -> LoomResult<u64> {
    validate_stream_name(stream)?;
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom.stream_len(ns, stream)? as u64)
}

fn consumer_position_ns(
    h: &LoomSession,
    workspace: &str,
    stream: &str,
    consumer_id: &str,
) -> LoomResult<u64> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom.consumer_position(ns, stream, consumer_id)
}

fn consumer_read_ns(
    h: &LoomSession,
    workspace: &str,
    stream: &str,
    consumer_id: &str,
    max: usize,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let mut encoded = Stream::new();
    for entry in loom.consumer_read(ns, stream, consumer_id, max)? {
        encoded.append(entry);
    }
    Ok(encoded.encode())
}

fn consumer_advance_ns(
    h: &LoomSession,
    workspace: &str,
    stream: &str,
    consumer_id: &str,
    next_seq: u64,
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom.consumer_advance(ns, stream, consumer_id, next_seq)?;
    save_loom(&mut loom)?;
    Ok(())
}

fn consumer_reset_ns(
    h: &LoomSession,
    workspace: &str,
    stream: &str,
    consumer_id: &str,
    next_seq: u64,
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom.consumer_reset(ns, stream, consumer_id, next_seq)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Append `entry_len` bytes at `entry` to `stream` in workspace `workspace` (selected by UUID or name,
/// created with the `queue` facet if absent). Writes the assigned zero-based sequence to `*out_seq` and
/// returns `0`. Saves the loom after staging the stream update.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`stream` valid C strings; `entry` null or `entry_len`
/// readable bytes; `out_seq` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_queue_append(
    handle: *mut LoomSession,
    workspace: *const c_char,
    stream: *const c_char,
    entry: *const c_uchar,
    entry_len: usize,
    out_seq: *mut u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_queue_append");
    let workspace = arg_str!(workspace, "loom_queue_append");
    let stream = arg_str!(stream, "loom_queue_append");
    // SAFETY: caller guarantees `(entry, entry_len)` describe a readable (or null) buffer (see fn docs).
    let bytes = unsafe { byte_slice(entry, entry_len) };
    match queue_append_ns(h, workspace, stream, bytes) {
        Ok(seq) => {
            if !out_seq.is_null() {
                // SAFETY: `out_seq` is writable per fn docs.
                unsafe { *out_seq = seq };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Fetch the entry at `seq` from `stream` in workspace `workspace`. On success returns `0` and sets
/// `*out_found`: when present, `*out_found = 1` and the bytes are written to `(*out_ptr, *out_len)` (free
/// with [`loom_bytes_free`]); when absent, `*out_found = 0` and `(*out_ptr, *out_len)` are `(null, 0)`.
/// A `seq` beyond `usize` range is `INVALID_ARGUMENT`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`stream` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_queue_get(
    handle: *mut LoomSession,
    workspace: *const c_char,
    stream: *const c_char,
    seq: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_queue_get");
    let workspace = arg_str!(workspace, "loom_queue_get");
    let stream = arg_str!(stream, "loom_queue_get");
    let Ok(seq) = usize::try_from(seq) else {
        return fail_arg("loom_queue_get: seq out of range");
    };
    match queue_get_ns(h, workspace, stream, seq) {
        Ok(Some(bytes)) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` is writable per fn docs.
                unsafe { *out_found = 1 };
            }
            // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
            unsafe { ok_bytes(out_ptr, out_len, bytes) }
        }
        Ok(None) => {
            // SAFETY: each non-null out-pointer is writable per fn docs.
            unsafe {
                if !out_found.is_null() {
                    *out_found = 0;
                }
                if !out_ptr.is_null() {
                    *out_ptr = core::ptr::null_mut();
                }
                if !out_len.is_null() {
                    *out_len = 0;
                }
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Read the half-open range `[lo, hi)` of `stream` in workspace `workspace`, oldest first; writes the
/// entries as Loom Canonical CBOR (an array of byte strings) to `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]) and returns `0`. A `lo`/`hi` beyond `usize` range is `INVALID_ARGUMENT`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`stream` valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_queue_range(
    handle: *mut LoomSession,
    workspace: *const c_char,
    stream: *const c_char,
    lo: u64,
    hi: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_queue_range");
    let workspace = arg_str!(workspace, "loom_queue_range");
    let stream = arg_str!(stream, "loom_queue_range");
    let (Ok(lo), Ok(hi)) = (usize::try_from(lo), usize::try_from(hi)) else {
        return fail_arg("loom_queue_range: lo/hi out of range");
    };
    match queue_range_ns(h, workspace, stream, lo, hi) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Write the number of entries in `stream` of workspace `workspace` to `*out_len` and return `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`stream` valid C strings; `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_queue_len(
    handle: *mut LoomSession,
    workspace: *const c_char,
    stream: *const c_char,
    out_len: *mut u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_queue_len");
    let workspace = arg_str!(workspace, "loom_queue_len");
    let stream = arg_str!(stream, "loom_queue_len");
    match queue_len_ns(h, workspace, stream) {
        Ok(n) => {
            if !out_len.is_null() {
                // SAFETY: `out_len` is writable per fn docs.
                unsafe { *out_len = n };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Write the named consumer's next sequence for `stream` of workspace `workspace` to `*out_seq`; a
/// consumer with no stored offset reads as `0`. Returns `0` on success.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`stream`/`consumer_id` valid C strings; `out_seq` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_queue_consumer_position(
    handle: *mut LoomSession,
    workspace: *const c_char,
    stream: *const c_char,
    consumer_id: *const c_char,
    out_seq: *mut u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_queue_consumer_position");
    let workspace = arg_str!(workspace, "loom_queue_consumer_position");
    let stream = arg_str!(stream, "loom_queue_consumer_position");
    let consumer_id = arg_str!(consumer_id, "loom_queue_consumer_position");
    match consumer_position_ns(h, workspace, stream, consumer_id) {
        Ok(seq) => {
            if !out_seq.is_null() {
                // SAFETY: `out_seq` is writable per fn docs.
                unsafe { *out_seq = seq };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Read up to `max` entries from the consumer's stored next sequence in `stream`, oldest first, as Loom
/// Canonical CBOR (an array of byte strings) to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]).
/// Does not advance the consumer's progress. Returns `0` on success.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`stream`/`consumer_id` valid C strings;
/// `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_queue_consumer_read(
    handle: *mut LoomSession,
    workspace: *const c_char,
    stream: *const c_char,
    consumer_id: *const c_char,
    max: u32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_queue_consumer_read");
    let workspace = arg_str!(workspace, "loom_queue_consumer_read");
    let stream = arg_str!(stream, "loom_queue_consumer_read");
    let consumer_id = arg_str!(consumer_id, "loom_queue_consumer_read");
    match consumer_read_ns(h, workspace, stream, consumer_id, max as usize) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Advance the named consumer's next sequence for `stream` to `next_seq` and save the loom. Monotonic:
/// a backward `next_seq`, or one past the stream length, is `INVALID_ARGUMENT`. Returns `0` on success.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`stream`/`consumer_id` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_queue_consumer_advance(
    handle: *mut LoomSession,
    workspace: *const c_char,
    stream: *const c_char,
    consumer_id: *const c_char,
    next_seq: u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_queue_consumer_advance");
    let workspace = arg_str!(workspace, "loom_queue_consumer_advance");
    let stream = arg_str!(stream, "loom_queue_consumer_advance");
    let consumer_id = arg_str!(consumer_id, "loom_queue_consumer_advance");
    match consumer_advance_ns(h, workspace, stream, consumer_id, next_seq) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Set the named consumer's next sequence for `stream` to `next_seq` and save the loom; this may move
/// backward. A `next_seq` past the stream length is `INVALID_ARGUMENT`. Returns `0` on success.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`stream`/`consumer_id` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_queue_consumer_reset(
    handle: *mut LoomSession,
    workspace: *const c_char,
    stream: *const c_char,
    consumer_id: *const c_char,
    next_seq: u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_queue_consumer_reset");
    let workspace = arg_str!(workspace, "loom_queue_consumer_reset");
    let stream = arg_str!(stream, "loom_queue_consumer_reset");
    let consumer_id = arg_str!(consumer_id, "loom_queue_consumer_reset");
    match consumer_reset_ns(h, workspace, stream, consumer_id, next_seq) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Commit the working tree for workspace `ns_name`; writes the new commit's
/// content address to `*out`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_commit(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    author: *const c_char,
    message: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_commit");
    let (n, a, m) = (
        arg_str!(ns_name, "loom_commit"),
        arg_str!(author, "loom_commit"),
        arg_str!(message, "loom_commit"),
    );
    match commit_ns(h, n, a, m) {
        // SAFETY: `out` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out, &s) },
        Err(e) => fail(e),
    }
}

/// Create a branch at the current `HEAD` tip for workspace `ns_name`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_branch(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    branch: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_branch");
    let (n, b) = (
        arg_str!(ns_name, "loom_branch"),
        arg_str!(branch, "loom_branch"),
    );
    match branch_ns(h, n, b) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Check out `branch` for workspace `ns_name`, materializing its tip into the
/// working tree.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_checkout(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    branch: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_checkout");
    let (n, b) = (
        arg_str!(ns_name, "loom_checkout"),
        arg_str!(branch, "loom_checkout"),
    );
    match checkout_ns(h, n, b) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// First-parent commit log of `branch` for workspace `ns_name`; writes a
/// canonical-CBOR array of content addresses (newest first) to `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]).
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_log(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    branch: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_log");
    let (n, b) = (arg_str!(ns_name, "loom_log"), arg_str!(branch, "loom_log"));
    match log_ns(h, n, b) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Merge `from_branch` into the current `HEAD` branch for workspace `ns_name` with required
/// `facet`. When `cell_level` is non-zero, tables reconcile at cell granularity. Writes the merge
/// outcome as canonical CBOR to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]).
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_merge(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    from_branch: *const c_char,
    author: *const c_char,
    cell_level: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_merge");
    let (n, f, a) = (
        arg_str!(ns_name, "loom_merge"),
        arg_str!(from_branch, "loom_merge"),
        arg_str!(author, "loom_merge"),
    );
    match merge_ns(h, n, f, a, cell_level != 0) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Whether workspace `ns_name` has a conflicted merge awaiting
/// [`loom_merge_continue`] or [`loom_merge_abort`]; writes `1`/`0` to `*out` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name` valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_merge_in_progress(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    out: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_merge_in_progress");
    let n = arg_str!(ns_name, "loom_merge_in_progress");
    match merge_in_progress_ns(h, n) {
        Ok(b) => {
            if !out.is_null() {
                // SAFETY: `out` is writable per fn docs.
                unsafe { *out = i32::from(b) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// List the still-unresolved conflict paths of the in-progress merge as a JSON string array; writes an
/// owned C string to `*out` (free with [`loom_string_free`]) and returns `0`. The array is empty when no
/// merge is in progress.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name` valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_merge_conflicts(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_merge_conflicts");
    let n = arg_str!(ns_name, "loom_merge_conflicts");
    match merge_conflicts_ns(h, n) {
        // SAFETY: `out` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out, &s) },
        Err(e) => fail(e),
    }
}

/// Settle one conflicted `path` of the in-progress merge: `resolution` is `0` ours, `1` theirs, `2`
/// working (accept the currently staged content). Returns `0` on success.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`path` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_merge_resolve(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    path: *const c_char,
    resolution: i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_merge_resolve");
    let (n, p) = (
        arg_str!(ns_name, "loom_merge_resolve"),
        arg_str!(path, "loom_merge_resolve"),
    );
    match merge_resolve_ns(h, n, p, resolution) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Abandon the in-progress merge in workspace `ns_name`, restoring the pre-merge working tree. Returns
/// `0` on success, `INVALID_ARGUMENT` if no merge is in progress.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_merge_abort(handle: *mut LoomSession, ns_name: *const c_char) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_merge_abort");
    let n = arg_str!(ns_name, "loom_merge_abort");
    match merge_abort_ns(h, n) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Finish the in-progress merge in workspace `ns_name`: record a two-parent merge commit over the
/// resolved working tree and advance the branch. Writes the new commit's content address to `*out`
/// (free with [`loom_string_free`]) and returns `0`; `CONFLICT` if conflicts remain.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`author` valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_merge_continue(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    author: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_merge_continue");
    let (n, a) = (
        arg_str!(ns_name, "loom_merge_continue"),
        arg_str!(author, "loom_merge_continue"),
    );
    match merge_continue_ns(h, n, a) {
        // SAFETY: `out` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out, &s) },
        Err(e) => fail(e),
    }
}

/// Stage one `path` of workspace `ns_name` into the shared staging index.
/// Returns `0` on success.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`path` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_stage(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    path: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_stage");
    let (n, p) = (
        arg_str!(ns_name, "loom_stage"),
        arg_str!(path, "loom_stage"),
    );
    match stage_ns(h, n, p) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Stage the entire working tree of workspace `ns_name` (every change across every facet). Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_stage_all(handle: *mut LoomSession, ns_name: *const c_char) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_stage_all");
    let n = arg_str!(ns_name, "loom_stage_all");
    match stage_all_ns(h, n) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Unstage one `path` of workspace `ns_name`, reverting its index entry to HEAD. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`path` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_unstage(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    path: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_unstage");
    let (n, p) = (
        arg_str!(ns_name, "loom_unstage"),
        arg_str!(path, "loom_unstage"),
    );
    match unstage_ns(h, n, p) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// The status of workspace `ns_name` as JSON (`{ "staged", "unstaged", "untracked", "conflicts" }`,
/// where staged/unstaged are arrays of `{ "path", "kind" }`). Writes an owned C string to `*out` (free
/// with [`loom_string_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name` valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_status(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_status");
    let n = arg_str!(ns_name, "loom_status");
    match status_json_ns(h, n) {
        // SAFETY: `out` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out, &s) },
        Err(e) => fail(e),
    }
}

/// Commit only the staged index of workspace `ns_name` (the `commit --staged` form). Writes the new
/// commit's content address to `*out` (free with [`loom_string_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`author`/`message` valid C strings; `out`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_commit_staged(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    author: *const c_char,
    message: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_commit_staged");
    let (n, a, m) = (
        arg_str!(ns_name, "loom_commit_staged"),
        arg_str!(author, "loom_commit_staged"),
        arg_str!(message, "loom_commit_staged"),
    );
    match commit_staged_ns(h, n, a, m) {
        // SAFETY: `out` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out, &s) },
        Err(e) => fail(e),
    }
}

/// Create-or-replace file `path` of workspace `ns_name` with `len` bytes at
/// `content` and `mode` (a `0` mode uses the default `0o100644`). The parent directory must exist.
/// Returns `0` on success.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`path` valid C strings; `content` null or
/// `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_write_file(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    path: *const c_char,
    content: *const c_uchar,
    len: usize,
    mode: u32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_write_file");
    let (n, p) = (
        arg_str!(ns_name, "loom_write_file"),
        arg_str!(path, "loom_write_file"),
    );
    // SAFETY: caller guarantees `(content, len)` is a readable buffer (see fn docs).
    let bytes = unsafe { byte_slice(content, len) };
    match write_file_ns(h, n, p, bytes, mode) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Read file `path` of workspace `ns_name` into `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]).
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`path` valid C strings; `out_ptr`/`out_len`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_read_file(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    path: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_read_file");
    let (n, p) = (
        arg_str!(ns_name, "loom_read_file"),
        arg_str!(path, "loom_read_file"),
    );
    match read_file_ns(h, n, p) {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Remove file `path` of workspace `ns_name` from the working tree. Returns `0` on success.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`path` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_remove_file(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    path: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_remove_file");
    let (n, p) = (
        arg_str!(ns_name, "loom_remove_file"),
        arg_str!(path, "loom_remove_file"),
    );
    match remove_file_ns(h, n, p) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Append `len` bytes at `content` to file `path` of workspace `ns_name`, creating it if absent (the
/// parent directory must exist). Returns `0` on success.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`path` valid C strings; `content` null or
/// `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_append_file(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    path: *const c_char,
    content: *const c_uchar,
    len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_append_file");
    let (n, p) = (
        arg_str!(ns_name, "loom_append_file"),
        arg_str!(path, "loom_append_file"),
    );
    // SAFETY: caller guarantees `(content, len)` is a readable buffer (see fn docs).
    let bytes = unsafe { byte_slice(content, len) };
    match append_file_ns(h, n, p, bytes) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Create a symbolic link at `link_path` in workspace `ns_name` whose target is the
/// opaque path string `target` (it may be dangling). The parent must exist (`NOT_FOUND`); `link_path`
/// must not already exist (`ALREADY_EXISTS`). Returns `0` on success.
///
/// # Safety
/// `handle` must be from [`loom_open`]; all string arguments valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_symlink(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    target: *const c_char,
    link_path: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_symlink");
    let (n, tg, lp) = (
        arg_str!(ns_name, "loom_symlink"),
        arg_str!(target, "loom_symlink"),
        arg_str!(link_path, "loom_symlink"),
    );
    match symlink_ns(h, n, tg, lp) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Read the target of the symbolic link at `path` in workspace `ns_name` into
/// `(*out_ptr)` (free with [`loom_string_free`]). `NOT_FOUND` if absent; `INVALID_ARGUMENT` if the path
/// is not a symlink. Returns `0` on success.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `ns_name`/`path` valid C strings; `out_ptr` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_read_link(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    path: *const c_char,
    out_ptr: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_read_link");
    let (n, p) = (
        arg_str!(ns_name, "loom_read_link"),
        arg_str!(path, "loom_read_link"),
    );
    match read_link_ns(h, n, p) {
        // SAFETY: `out_ptr` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out_ptr, &s) },
        Err(e) => fail(e),
    }
}
