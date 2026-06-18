//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// ---------------------------------------------------------------------------------------------------
// Ledger (Ledger facet) - append-only, profile-aware hash chain: append/get/head/len/verify.
// ---------------------------------------------------------------------------------------------------

fn ensure_ledger_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Ledger,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Ledger)?;
    Ok(ns)
}

fn ledger_append_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    payload: &[u8],
) -> LoomResult<u64> {
    let mut loom = open_h_write(h)?;
    let ns = ensure_ledger_ns(&mut loom, workspace)?;
    let seq = ledger_append(&mut loom, ns, collection, payload.to_vec())?;
    save_loom(&mut loom)?;
    Ok(seq)
}

fn ledger_get_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    seq: u64,
) -> LoomResult<Option<Vec<u8>>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    ledger_get(&loom, ns, collection, seq)
}

fn ledger_head_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
) -> LoomResult<Option<String>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(ledger_head(&loom, ns, collection)?.map(|d| d.to_string()))
}

fn ledger_len_ns(h: &LoomSession, workspace: &str, collection: &str) -> LoomResult<u64> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    ledger_len(&loom, ns, collection)
}

fn ledger_verify_ns(h: &LoomSession, workspace: &str, collection: &str) -> LoomResult<()> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    ledger_verify(&loom, ns, collection)
}

/// Append `payload` to ledger `collection` of workspace `workspace` (UUID or name, created with the `ledger`
/// facet if absent). Writes the new entry's zero-based sequence to `*out_seq` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings; `payload` null or
/// `payload_len` readable bytes; `out_seq` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_ledger_append(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    payload: *const c_uchar,
    payload_len: usize,
    out_seq: *mut u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_ledger_append");
    let workspace = arg_str!(workspace, "loom_ledger_append");
    let collection = arg_str!(collection, "loom_ledger_append");
    // SAFETY: caller guarantees `(payload, payload_len)` is readable/null (see docs).
    let payload = unsafe { byte_slice(payload, payload_len) };
    match ledger_append_ns(h, workspace, collection, payload) {
        Ok(seq) => {
            if !out_seq.is_null() {
                // SAFETY: `out_seq` writable per docs.
                unsafe { *out_seq = seq };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Fetch the payload at `seq` in ledger `collection`. On success returns `0` and sets `*out_found`: present
/// -> `1` and bytes at `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]); absent -> `0`, `(null, 0)`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_ledger_get(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    seq: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_ledger_get");
    let workspace = arg_str!(workspace, "loom_ledger_get");
    let collection = arg_str!(collection, "loom_ledger_get");
    match ledger_get_ns(h, workspace, collection, seq) {
        Ok(Some(bytes)) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` writable per docs.
                unsafe { *out_found = 1 };
            }
            // SAFETY: `out_ptr`/`out_len` writable per docs.
            unsafe { ok_bytes(out_ptr, out_len, bytes) }
        }
        Ok(None) => {
            // SAFETY: each non-null out-pointer is writable per docs.
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

/// The head chain hash of ledger `collection` as a `"algo:hex"` C string. On success returns `0` and sets
/// `*out_found`: present -> `1` and an owned string at `*out` (free with [`loom_string_free`]);
/// absent/empty -> `0` and `*out = null`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings; `out`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_ledger_head(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    out: *mut *mut c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_ledger_head");
    let workspace = arg_str!(workspace, "loom_ledger_head");
    let collection = arg_str!(collection, "loom_ledger_head");
    match ledger_head_ns(h, workspace, collection) {
        Ok(Some(s)) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` writable per docs.
                unsafe { *out_found = 1 };
            }
            // SAFETY: `out` writable per docs.
            unsafe { ok_str(out, &s) }
        }
        Ok(None) => {
            // SAFETY: each non-null out-pointer is writable per docs.
            unsafe {
                if !out_found.is_null() {
                    *out_found = 0;
                }
                if !out.is_null() {
                    *out = core::ptr::null_mut();
                }
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// The number of entries in ledger `collection` (0 when absent). Writes it to `*out_len` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings; `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_ledger_len(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    out_len: *mut u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_ledger_len");
    let workspace = arg_str!(workspace, "loom_ledger_len");
    let collection = arg_str!(collection, "loom_ledger_len");
    match ledger_len_ns(h, workspace, collection) {
        Ok(len) => {
            if !out_len.is_null() {
                // SAFETY: `out_len` writable per docs.
                unsafe { *out_len = len };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Recompute ledger `collection`'s chain from genesis and confirm every stored hash matches. Returns `0` when
/// the chain is intact; an altered payload or broken link is `INTEGRITY_FAILURE`. An absent ledger is
/// intact (returns `0`).
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_ledger_verify(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_ledger_verify");
    let workspace = arg_str!(workspace, "loom_ledger_verify");
    let collection = arg_str!(collection, "loom_ledger_verify");
    match ledger_verify_ns(h, workspace, collection) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}
