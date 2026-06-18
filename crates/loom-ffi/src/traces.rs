//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

fn ensure_traces_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Traces,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Traces)?;
    Ok(ns)
}

fn traces_query_result_cbor(result: loom_core::TraceQueryResult) -> LoomResult<Vec<u8>> {
    let spans = result
        .spans
        .iter()
        .map(|span| span.encode().map(CborValue::Bytes))
        .collect::<LoomResult<Vec<_>>>()?;
    cbor_encode(&CborValue::Array(vec![
        CborValue::Array(spans),
        CborValue::Bool(result.partial),
    ]))
    .map_err(|error| LoomError::invalid(format!("trace query result encoding failed: {error}")))
}

/// Store a canonical span record in the traces facet.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string; `span` must be
/// null or readable for `span_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_traces_put_span(
    handle: *mut LoomSession,
    workspace: *const c_char,
    span: *const c_uchar,
    span_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_traces_put_span");
    let workspace = arg_str!(workspace, "loom_traces_put_span");
    let span = unsafe { byte_slice(span, span_len) };
    let span = match loom_core::SpanRecord::decode(span) {
        Ok(span) => span,
        Err(error) => return fail(error),
    };
    let mut loom = match open_h_write(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match ensure_traces_ns(&mut loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::traces_put_span(&mut loom, ns, &span).and_then(|()| {
        save_loom(&mut loom)?;
        Ok(())
    }) {
        Ok(()) => 0,
        Err(error) => fail(error),
    }
}

/// Fetch a span by trace id and span id as canonical CBOR bytes.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`, `trace_id`, and `span_id` must be valid C
/// strings; `out_ptr`/`out_len`/`out_found` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_traces_get_span(
    handle: *mut LoomSession,
    workspace: *const c_char,
    trace_id: *const c_char,
    span_id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_traces_get_span");
    let workspace = arg_str!(workspace, "loom_traces_get_span");
    let trace_id = arg_str!(trace_id, "loom_traces_get_span");
    let span_id = arg_str!(span_id, "loom_traces_get_span");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::traces_get_span(&loom, ns, trace_id, span_id) {
        Ok(Some(span)) => {
            if !out_found.is_null() {
                unsafe { *out_found = 1 };
            }
            match span.encode() {
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

/// Query one trace as canonical CBOR `[spans, partial]`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `trace_id` must be valid C strings;
/// `out_ptr`/`out_len` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_traces_trace_spans_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    trace_id: *const c_char,
    max_spans: u32,
    max_output_bytes: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_traces_trace_spans_cbor");
    let workspace = arg_str!(workspace, "loom_traces_trace_spans_cbor");
    let trace_id = arg_str!(trace_id, "loom_traces_trace_spans_cbor");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::traces_trace_spans(&loom, ns, trace_id, max_spans, max_output_bytes)
        .and_then(traces_query_result_cbor)
    {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(error) => fail(error),
    }
}

/// Query spans as canonical CBOR `[spans, partial]`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string; `out_ptr`/`out_len`
/// must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_traces_query_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    from_start_time_ns: u64,
    to_start_time_ns: u64,
    max_spans: u32,
    max_output_bytes: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_traces_query_cbor");
    let workspace = arg_str!(workspace, "loom_traces_query_cbor");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    let query = loom_core::TraceQuery {
        from_start_time_ns,
        to_start_time_ns,
        max_spans,
        max_output_bytes,
    };
    match loom_core::traces_query(&loom, ns, &query).and_then(traces_query_result_cbor) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(error) => fail(error),
    }
}
