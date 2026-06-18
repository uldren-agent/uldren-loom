//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

fn ensure_metrics_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Metrics,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Metrics)?;
    Ok(ns)
}

fn metrics_query_result_cbor(result: loom_core::MetricQueryResult) -> LoomResult<Vec<u8>> {
    let observations = result
        .observations
        .iter()
        .map(|observation| observation.encode().map(CborValue::Bytes))
        .collect::<LoomResult<Vec<_>>>()?;
    cbor_encode(&CborValue::Array(vec![
        CborValue::Array(observations),
        CborValue::Bool(result.partial),
        CborValue::Bool(result.stale),
    ]))
    .map_err(|error| LoomError::invalid(format!("metric query result encoding failed: {error}")))
}

/// Store a canonical metric descriptor record in the metrics facet.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string; `descriptor` must be
/// null or readable for `descriptor_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_metrics_put_descriptor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    descriptor: *const c_uchar,
    descriptor_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_metrics_put_descriptor");
    let workspace = arg_str!(workspace, "loom_metrics_put_descriptor");
    let descriptor = unsafe { byte_slice(descriptor, descriptor_len) };
    let descriptor = match loom_core::MetricDescriptor::decode(descriptor) {
        Ok(descriptor) => descriptor,
        Err(error) => return fail(error),
    };
    let mut loom = match open_h_write(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match ensure_metrics_ns(&mut loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::metrics_put_descriptor(&mut loom, ns, &descriptor).and_then(|()| {
        save_loom(&mut loom)?;
        Ok(())
    }) {
        Ok(()) => 0,
        Err(error) => fail(error),
    }
}

/// Fetch a metric descriptor by name as canonical CBOR bytes.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `name` must be valid C strings;
/// `out_ptr`/`out_len`/`out_found` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_metrics_get_descriptor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_metrics_get_descriptor");
    let workspace = arg_str!(workspace, "loom_metrics_get_descriptor");
    let name = arg_str!(name, "loom_metrics_get_descriptor");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::metrics_get_descriptor(&loom, ns, name) {
        Ok(Some(descriptor)) => {
            if !out_found.is_null() {
                unsafe { *out_found = 1 };
            }
            match descriptor.encode() {
                Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
                Err(error) => fail(error),
            }
        }
        Ok(None) => {
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
        Err(error) => fail(error),
    }
}

/// Store a canonical metric observation under an existing descriptor.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `descriptor_name` must be valid C strings;
/// `observation` must be null or readable for `observation_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_metrics_put_observation(
    handle: *mut LoomSession,
    workspace: *const c_char,
    descriptor_name: *const c_char,
    observation: *const c_uchar,
    observation_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_metrics_put_observation");
    let workspace = arg_str!(workspace, "loom_metrics_put_observation");
    let descriptor_name = arg_str!(descriptor_name, "loom_metrics_put_observation");
    let observation = unsafe { byte_slice(observation, observation_len) };
    let observation = match loom_core::MetricObservation::decode(observation) {
        Ok(observation) => observation,
        Err(error) => return fail(error),
    };
    let mut loom = match open_h_write(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match ensure_metrics_ns(&mut loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::metrics_put_observation(&mut loom, ns, descriptor_name, &observation).and_then(
        |()| {
            save_loom(&mut loom)?;
            Ok(())
        },
    ) {
        Ok(()) => 0,
        Err(error) => fail(error),
    }
}

/// Query observations as canonical CBOR `[observations, partial, stale]`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `descriptor_name` must be valid C strings;
/// `out_ptr`/`out_len` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_metrics_query_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    descriptor_name: *const c_char,
    from_timestamp_ms: u64,
    to_timestamp_ms: u64,
    max_series: u32,
    max_groups: u32,
    max_samples: u32,
    max_output_bytes: u64,
    now_timestamp_ms: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_metrics_query_cbor");
    let workspace = arg_str!(workspace, "loom_metrics_query_cbor");
    let descriptor_name = arg_str!(descriptor_name, "loom_metrics_query_cbor");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    let query = loom_core::MetricQuery {
        from_timestamp_ms,
        to_timestamp_ms,
        max_series,
        max_groups,
        max_samples,
        max_output_bytes,
        now_timestamp_ms,
    };
    match loom_core::metrics_query_observations(&loom, ns, descriptor_name, &query)
        .and_then(metrics_query_result_cbor)
    {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(error) => fail(error),
    }
}
