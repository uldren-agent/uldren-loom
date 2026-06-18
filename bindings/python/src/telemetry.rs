//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

fn ensure_metrics_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Metrics,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Metrics)
        .map_err(py_err)?;
    Ok(ns)
}

fn ensure_logs_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Logs,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Logs)
        .map_err(py_err)?;
    Ok(ns)
}

fn ensure_traces_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Traces,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Traces)
        .map_err(py_err)?;
    Ok(ns)
}

fn metrics_query_result_cbor(result: loom_core::MetricQueryResult) -> loom_core::Result<Vec<u8>> {
    let observations = result
        .observations
        .iter()
        .map(|observation| observation.encode().map(CborValue::Bytes))
        .collect::<loom_core::Result<Vec<_>>>()?;
    cbor_encode(&CborValue::Array(vec![
        CborValue::Array(observations),
        CborValue::Bool(result.partial),
        CborValue::Bool(result.stale),
    ]))
    .map_err(|error| {
        loom_core::LoomError::invalid(format!("metric query result encoding failed: {error}"))
    })
}

fn logs_query_result_cbor(result: loom_core::LogQueryResult) -> loom_core::Result<Vec<u8>> {
    let records = result
        .records
        .iter()
        .map(|record| record.encode().map(CborValue::Bytes))
        .collect::<loom_core::Result<Vec<_>>>()?;
    cbor_encode(&CborValue::Array(vec![
        CborValue::Array(records),
        CborValue::Bool(result.partial),
    ]))
    .map_err(|error| {
        loom_core::LoomError::invalid(format!("log query result encoding failed: {error}"))
    })
}

fn traces_query_result_cbor(result: loom_core::TraceQueryResult) -> loom_core::Result<Vec<u8>> {
    let spans = result
        .spans
        .iter()
        .map(|span| span.encode().map(CborValue::Bytes))
        .collect::<loom_core::Result<Vec<_>>>()?;
    cbor_encode(&CborValue::Array(vec![
        CborValue::Array(spans),
        CborValue::Bool(result.partial),
    ]))
    .map_err(|error| {
        loom_core::LoomError::invalid(format!("trace query result encoding failed: {error}"))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, descriptor, passphrase=None))]
pub(crate) fn metrics_put_descriptor(
    path: &str,
    workspace: &str,
    descriptor: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_metrics_ns(&mut loom, workspace)?;
    let descriptor = loom_core::MetricDescriptor::decode(descriptor).map_err(py_err)?;
    loom_core::metrics_put_descriptor(&mut loom, ns, &descriptor).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn metrics_get_descriptor<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    match loom_core::metrics_get_descriptor(&loom, ns, name).map_err(py_err)? {
        Some(descriptor) => Ok(Some(PyBytes::new(
            py,
            &descriptor.encode().map_err(py_err)?,
        ))),
        None => Ok(None),
    }
}

#[pyfunction]
#[pyo3(signature = (path, workspace, descriptor_name, observation, passphrase=None))]
pub(crate) fn metrics_put_observation(
    path: &str,
    workspace: &str,
    descriptor_name: &str,
    observation: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_metrics_ns(&mut loom, workspace)?;
    let observation = loom_core::MetricObservation::decode(observation).map_err(py_err)?;
    loom_core::metrics_put_observation(&mut loom, ns, descriptor_name, &observation)
        .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (
    path,
    workspace,
    descriptor_name,
    from_timestamp_ms,
    to_timestamp_ms,
    max_series,
    max_groups,
    max_samples,
    max_output_bytes,
    passphrase=None
))]
pub(crate) fn metrics_query<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    descriptor_name: &str,
    from_timestamp_ms: u64,
    to_timestamp_ms: u64,
    max_series: u32,
    max_groups: u32,
    max_samples: u32,
    max_output_bytes: u64,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let query = loom_core::MetricQuery {
        from_timestamp_ms,
        to_timestamp_ms,
        max_series,
        max_groups,
        max_samples,
        max_output_bytes,
        now_timestamp_ms: now_ms(),
    };
    let bytes = loom_core::metrics_query_observations(&loom, ns, descriptor_name, &query)
        .and_then(metrics_query_result_cbor)
        .map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, record, passphrase=None))]
pub(crate) fn logs_put_record(
    path: &str,
    workspace: &str,
    record: &[u8],
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_logs_ns(&mut loom, workspace)?;
    let record = loom_core::LogRecord::decode(record).map_err(py_err)?;
    let id = loom_core::logs_put_record(&mut loom, ns, &record).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(id)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, record_id, passphrase=None))]
pub(crate) fn logs_get_record<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    record_id: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    match loom_core::logs_get_record(&loom, ns, record_id).map_err(py_err)? {
        Some(record) => Ok(Some(PyBytes::new(py, &record.encode().map_err(py_err)?))),
        None => Ok(None),
    }
}

#[pyfunction]
#[pyo3(signature = (
    path,
    workspace,
    from_time_unix_nano,
    to_time_unix_nano,
    max_records,
    max_output_bytes,
    passphrase=None
))]
pub(crate) fn logs_query<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    from_time_unix_nano: u64,
    to_time_unix_nano: u64,
    max_records: u32,
    max_output_bytes: u64,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let query = loom_core::LogQuery {
        from_time_unix_nano,
        to_time_unix_nano,
        max_records,
        max_output_bytes,
    };
    let bytes = loom_core::logs_query(&loom, ns, &query)
        .and_then(logs_query_result_cbor)
        .map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, span, passphrase=None))]
pub(crate) fn traces_put_span(
    path: &str,
    workspace: &str,
    span: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_traces_ns(&mut loom, workspace)?;
    let span = loom_core::SpanRecord::decode(span).map_err(py_err)?;
    loom_core::traces_put_span(&mut loom, ns, &span).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (path, workspace, trace_id, span_id, passphrase=None))]
pub(crate) fn traces_get_span<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    trace_id: &str,
    span_id: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    match loom_core::traces_get_span(&loom, ns, trace_id, span_id).map_err(py_err)? {
        Some(span) => Ok(Some(PyBytes::new(py, &span.encode().map_err(py_err)?))),
        None => Ok(None),
    }
}

#[pyfunction]
#[pyo3(signature = (path, workspace, trace_id, max_spans, max_output_bytes, passphrase=None))]
pub(crate) fn traces_trace_spans<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    trace_id: &str,
    max_spans: u32,
    max_output_bytes: u64,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = loom_core::traces_trace_spans(&loom, ns, trace_id, max_spans, max_output_bytes)
        .and_then(traces_query_result_cbor)
        .map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (
    path,
    workspace,
    from_start_time_ns,
    to_start_time_ns,
    max_spans,
    max_output_bytes,
    passphrase=None
))]
pub(crate) fn traces_query<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    from_start_time_ns: u64,
    to_start_time_ns: u64,
    max_spans: u32,
    max_output_bytes: u64,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let query = loom_core::TraceQuery {
        from_start_time_ns,
        to_start_time_ns,
        max_spans,
        max_output_bytes,
    };
    let bytes = loom_core::traces_query(&loom, ns, &query)
        .and_then(traces_query_result_cbor)
        .map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}
