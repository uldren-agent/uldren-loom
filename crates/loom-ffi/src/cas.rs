//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// ---------------------------------------------------------------------------------------------------
// Content-addressed store (CAS facet) - put/get/has/list over a workspace, by UUID or name.
// ---------------------------------------------------------------------------------------------------

/// Resolve a workspace for a CAS write by UUID or name, ensuring the `cas` facet exists. A name not yet
/// present is created carrying the `cas` facet; an unknown UUID is `NOT_FOUND`.
fn ensure_cas_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Cas,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Cas)?;
    Ok(ns)
}

fn cas_put_ns(h: &LoomSession, workspace: &str, bytes: &[u8]) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let ns = ensure_cas_ns(&mut loom, workspace)?;
    let digest = cas_put(&mut loom, ns, bytes)?;
    save_loom(&mut loom)?;
    Ok(digest.to_string())
}

fn cas_get_ns(h: &LoomSession, workspace: &str, digest: &str) -> LoomResult<Option<Vec<u8>>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let digest = Digest::parse(digest)?;
    cas_get(&loom, ns, &digest)
}

fn cas_has_ns(h: &LoomSession, workspace: &str, digest: &str) -> LoomResult<bool> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let digest = Digest::parse(digest)?;
    cas_has(&loom, ns, &digest)
}

fn cas_delete_ns(h: &LoomSession, workspace: &str, digest: &str) -> LoomResult<bool> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let digest = Digest::parse(digest)?;
    let present = cas_delete(&mut loom, ns, &digest)?;
    if present {
        save_loom(&mut loom)?;
    }
    Ok(present)
}

fn cas_list_json_ns(h: &LoomSession, workspace: &str) -> LoomResult<String> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let list = cas_list(&loom, ns)?;
    let mut out = String::from("[");
    for (i, d) in list.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_string(&d.to_string()));
    }
    out.push(']');
    Ok(out)
}

/// Store `len` bytes at `content` in the `cas` facet of workspace `workspace` (selected by UUID or name,
/// created with the `cas` facet if absent). Writes the content address (`"algo:hex"`) to `*out` (free
/// with [`loom_string_free`]) and returns `0`. Idempotent: identical bytes yield the same address.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` a valid C string; `content` null or `len` readable
/// bytes; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cas_put(
    handle: *mut LoomSession,
    workspace: *const c_char,
    content: *const c_uchar,
    len: usize,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cas_put");
    let workspace = arg_str!(workspace, "loom_cas_put");
    // SAFETY: caller guarantees `(content, len)` describe a readable (or null) buffer (see fn docs).
    let bytes = unsafe { byte_slice(content, len) };
    match cas_put_ns(h, workspace, bytes) {
        // SAFETY: `out` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out, &s) },
        Err(e) => fail(e),
    }
}

/// Fetch the blob addressed by `digest` from workspace `workspace`. On success returns `0` and sets
/// `*out_found`: when present, `*out_found = 1` and the bytes are written to `(*out_ptr, *out_len)` (free
/// with [`loom_bytes_free`]); when absent, `*out_found = 0` and `(*out_ptr, *out_len)` are `(null, 0)`.
/// An invalid digest is `INVALID_ARGUMENT`; a content/digest mismatch is `INTEGRITY_FAILURE`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`digest` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cas_get(
    handle: *mut LoomSession,
    workspace: *const c_char,
    digest: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cas_get");
    let workspace = arg_str!(workspace, "loom_cas_get");
    let digest = arg_str!(digest, "loom_cas_get");
    match cas_get_ns(h, workspace, digest) {
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

/// Whether a blob addressed by `digest` is present in workspace `workspace`; writes `1`/`0` to
/// `*out_found` and returns `0`. An invalid digest is `INVALID_ARGUMENT`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`digest` valid C strings; `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cas_has(
    handle: *mut LoomSession,
    workspace: *const c_char,
    digest: *const c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cas_has");
    let workspace = arg_str!(workspace, "loom_cas_has");
    let digest = arg_str!(digest, "loom_cas_has");
    match cas_has_ns(h, workspace, digest) {
        Ok(found) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` is writable per fn docs.
                unsafe { *out_found = i32::from(found) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Drop the blob addressed by `digest` from workspace `workspace`'s current working tree, making it
/// unreachable going forward; writes whether it was present (`1`/`0`) to `*out_found` and returns `0`.
/// CAS stays immutable - this unlinks a reference; the bytes are reclaimed by GC once unreferenced.
/// Removing an absent digest is a no-op. An invalid digest is `INVALID_ARGUMENT`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`digest` valid C strings; `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cas_delete(
    handle: *mut LoomSession,
    workspace: *const c_char,
    digest: *const c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cas_delete");
    let workspace = arg_str!(workspace, "loom_cas_delete");
    let digest = arg_str!(digest, "loom_cas_delete");
    match cas_delete_ns(h, workspace, digest) {
        Ok(found) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` is writable per fn docs.
                unsafe { *out_found = i32::from(found) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// List the content addresses (`"algo:hex"`) reachable in workspace `workspace`'s `cas` facet as a JSON
/// string array, sorted. Enumeration is within the workspace, not a global index. Writes an owned C
/// string to `*out` (free with [`loom_string_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` a valid C string; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cas_list_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cas_list_json");
    let workspace = arg_str!(workspace, "loom_cas_list_json");
    match cas_list_json_ns(h, workspace) {
        // SAFETY: `out` is writable per fn docs.
        Ok(s) => unsafe { ok_str(out, &s) },
        Err(e) => fail(e),
    }
}
