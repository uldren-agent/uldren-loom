//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

fn ensure_logs_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Logs,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Logs)?;
    Ok(ns)
}

fn logs_query_result_cbor(result: loom_core::LogQueryResult) -> LoomResult<Vec<u8>> {
    let records = result
        .records
        .iter()
        .map(|record| record.encode().map(CborValue::Bytes))
        .collect::<LoomResult<Vec<_>>>()?;
    cbor_encode(&CborValue::Array(vec![
        CborValue::Array(records),
        CborValue::Bool(result.partial),
    ]))
    .map_err(|error| LoomError::invalid(format!("log query result encoding failed: {error}")))
}

/// Store a canonical log record in the logs facet and return the record id.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string; `record` must be
/// null or readable for `record_len` bytes; `out_ptr`/`out_len` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_logs_put_record(
    handle: *mut LoomSession,
    workspace: *const c_char,
    record: *const c_uchar,
    record_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_logs_put_record");
    let workspace = arg_str!(workspace, "loom_logs_put_record");
    let record = unsafe { byte_slice(record, record_len) };
    let record = match loom_core::LogRecord::decode(record) {
        Ok(record) => record,
        Err(error) => return fail(error),
    };
    let mut loom = match open_h_write(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match ensure_logs_ns(&mut loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::logs_put_record(&mut loom, ns, &record).and_then(|record_id| {
        save_loom(&mut loom)?;
        Ok(record_id.into_bytes())
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(error) => fail(error),
    }
}

/// Fetch a log record by id as canonical CBOR bytes.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `record_id` must be valid C strings;
/// `out_ptr`/`out_len`/`out_found` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_logs_get_record(
    handle: *mut LoomSession,
    workspace: *const c_char,
    record_id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_logs_get_record");
    let workspace = arg_str!(workspace, "loom_logs_get_record");
    let record_id = arg_str!(record_id, "loom_logs_get_record");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::logs_get_record(&loom, ns, record_id) {
        Ok(Some(record)) => {
            if !out_found.is_null() {
                unsafe { *out_found = 1 };
            }
            match record.encode() {
                Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
                Err(error) => fail(error),
            }
        }
        Ok(None) => unsafe {
            if !out_found.is_null() {
                *out_found = 0;
            }
            ok_bytes(out_ptr, out_len, Vec::new())
        },
        Err(error) => fail(error),
    }
}

/// Query log records as canonical CBOR `[records, partial]`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string; `out_ptr`/`out_len`
/// must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_logs_query_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    from_time_unix_nano: u64,
    to_time_unix_nano: u64,
    max_records: u32,
    max_output_bytes: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_logs_query_cbor");
    let workspace = arg_str!(workspace, "loom_logs_query_cbor");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    let query = loom_core::LogQuery {
        from_time_unix_nano,
        to_time_unix_nano,
        max_records,
        max_output_bytes,
    };
    match loom_core::logs_query(&loom, ns, &query).and_then(logs_query_result_cbor) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(error) => fail(error),
    }
}
