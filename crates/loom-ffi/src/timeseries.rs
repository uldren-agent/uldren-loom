//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// ---------------------------------------------------------------------------------------------------
// Time-series (TimeSeries facet) - put/get a point by i64 timestamp, half-open range, and latest.
// ---------------------------------------------------------------------------------------------------

fn ensure_ts_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::TimeSeries,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::TimeSeries)?;
    Ok(ns)
}

fn ts_put_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    ts: i64,
    value: &[u8],
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = ensure_ts_ns(&mut loom, workspace)?;
    ts_put(&mut loom, ns, collection, ts, value.to_vec())?;
    save_loom(&mut loom)?;
    Ok(())
}

fn ts_get_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    ts: i64,
) -> LoomResult<Option<Vec<u8>>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    ts_get(&loom, ns, collection, ts)
}

fn ts_range_cbor_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    from: i64,
    to: i64,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(ts_range(&loom, ns, collection, from, to)?.encode())
}

fn ts_latest_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
) -> LoomResult<Option<(i64, Vec<u8>)>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    ts_latest(&loom, ns, collection)
}

/// Record `value` at timestamp `ts` in series `collection` of workspace `workspace` (UUID or name, created
/// with the `time-series` facet if absent). Returns `0`. A repeated timestamp replaces the point.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings; `value` null or `value_len`
/// readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_ts_put(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    ts: i64,
    value: *const c_uchar,
    value_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_ts_put");
    let workspace = arg_str!(workspace, "loom_ts_put");
    let collection = arg_str!(collection, "loom_ts_put");
    // SAFETY: caller guarantees `(value, value_len)` is readable/null (see docs).
    let value = unsafe { byte_slice(value, value_len) };
    match ts_put_ns(h, workspace, collection, ts, value) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Fetch the point at `ts` in series `collection`. On success returns `0` and sets `*out_found`: present ->
/// `1` and bytes at `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]); absent -> `0` and `(null, 0)`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_ts_get(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    ts: i64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_ts_get");
    let workspace = arg_str!(workspace, "loom_ts_get");
    let collection = arg_str!(collection, "loom_ts_get");
    match ts_get_ns(h, workspace, collection, ts) {
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

/// The points of series `collection` with `from <= ts < to` (half-open, time order) as the Loom Canonical
/// CBOR array of `[ts, value]` pairs. Writes owned bytes to `(*out_ptr, *out_len)` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_ts_range_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    from: i64,
    to: i64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_ts_range_cbor");
    let workspace = arg_str!(workspace, "loom_ts_range_cbor");
    let collection = arg_str!(collection, "loom_ts_range_cbor");
    match ts_range_cbor_ns(h, workspace, collection, from, to) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// The most recent point of series `collection`. On success returns `0` and sets `*out_found`: present -> `1`,
/// `*out_ts` is its timestamp, and bytes at `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]);
/// absent/empty -> `0`, `*out_ts = 0`, `(null, 0)`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings;
/// `out_ts`/`out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_ts_latest(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    out_ts: *mut i64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_ts_latest");
    let workspace = arg_str!(workspace, "loom_ts_latest");
    let collection = arg_str!(collection, "loom_ts_latest");
    match ts_latest_ns(h, workspace, collection) {
        Ok(Some((ts, bytes))) => {
            // SAFETY: each non-null out-pointer is writable per docs.
            unsafe {
                if !out_ts.is_null() {
                    *out_ts = ts;
                }
                if !out_found.is_null() {
                    *out_found = 1;
                }
                ok_bytes(out_ptr, out_len, bytes)
            }
        }
        Ok(None) => {
            // SAFETY: each non-null out-pointer is writable per docs.
            unsafe {
                if !out_ts.is_null() {
                    *out_ts = 0;
                }
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
