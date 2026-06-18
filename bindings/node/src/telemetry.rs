//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

fn ensure_metrics_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
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
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Metrics)
        .map_err(reason)?;
    Ok(ns)
}

fn ensure_logs_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
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
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Logs)
        .map_err(reason)?;
    Ok(ns)
}

fn ensure_traces_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
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
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Traces)
        .map_err(reason)?;
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

#[napi]
pub fn metrics_put_descriptor(
    loom_path: String,
    workspace: String,
    descriptor: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_metrics_ns(&mut loom, &workspace)?;
    let descriptor = loom_core::MetricDescriptor::decode(&descriptor).map_err(reason)?;
    loom_core::metrics_put_descriptor(&mut loom, ns, &descriptor).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}

#[napi]
pub fn metrics_get_descriptor(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    match loom_core::metrics_get_descriptor(&loom, ns, &name).map_err(reason)? {
        Some(descriptor) => Ok(Some(Uint8Array::from(descriptor.encode().map_err(reason)?))),
        None => Ok(None),
    }
}

#[napi]
pub fn metrics_put_observation(
    loom_path: String,
    workspace: String,
    descriptor_name: String,
    observation: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_metrics_ns(&mut loom, &workspace)?;
    let observation = loom_core::MetricObservation::decode(&observation).map_err(reason)?;
    loom_core::metrics_put_observation(&mut loom, ns, &descriptor_name, &observation)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}

#[napi]
pub fn metrics_query(
    loom_path: String,
    workspace: String,
    descriptor_name: String,
    from_timestamp_ms: BigInt,
    to_timestamp_ms: BigInt,
    max_series: u32,
    max_groups: u32,
    max_samples: u32,
    max_output_bytes: BigInt,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let query = loom_core::MetricQuery {
        from_timestamp_ms: bigint_to_u64(from_timestamp_ms, "from_timestamp_ms")?,
        to_timestamp_ms: bigint_to_u64(to_timestamp_ms, "to_timestamp_ms")?,
        max_series,
        max_groups,
        max_samples,
        max_output_bytes: bigint_to_u64(max_output_bytes, "max_output_bytes")?,
        now_timestamp_ms: now_ms(),
    };
    Ok(Uint8Array::from(
        loom_core::metrics_query_observations(&loom, ns, &descriptor_name, &query)
            .and_then(metrics_query_result_cbor)
            .map_err(reason)?,
    ))
}

#[napi]
pub fn logs_put_record(
    loom_path: String,
    workspace: String,
    record: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_logs_ns(&mut loom, &workspace)?;
    let record = loom_core::LogRecord::decode(&record).map_err(reason)?;
    let id = loom_core::logs_put_record(&mut loom, ns, &record).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(id)
}

#[napi]
pub fn logs_get_record(
    loom_path: String,
    workspace: String,
    record_id: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    match loom_core::logs_get_record(&loom, ns, &record_id).map_err(reason)? {
        Some(record) => Ok(Some(Uint8Array::from(record.encode().map_err(reason)?))),
        None => Ok(None),
    }
}

#[napi]
pub fn logs_query(
    loom_path: String,
    workspace: String,
    from_time_unix_nano: BigInt,
    to_time_unix_nano: BigInt,
    max_records: u32,
    max_output_bytes: BigInt,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let query = loom_core::LogQuery {
        from_time_unix_nano: bigint_to_u64(from_time_unix_nano, "from_time_unix_nano")?,
        to_time_unix_nano: bigint_to_u64(to_time_unix_nano, "to_time_unix_nano")?,
        max_records,
        max_output_bytes: bigint_to_u64(max_output_bytes, "max_output_bytes")?,
    };
    Ok(Uint8Array::from(
        loom_core::logs_query(&loom, ns, &query)
            .and_then(logs_query_result_cbor)
            .map_err(reason)?,
    ))
}

#[napi]
pub fn traces_put_span(
    loom_path: String,
    workspace: String,
    span: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_traces_ns(&mut loom, &workspace)?;
    let span = loom_core::SpanRecord::decode(&span).map_err(reason)?;
    loom_core::traces_put_span(&mut loom, ns, &span).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}

#[napi]
pub fn traces_get_span(
    loom_path: String,
    workspace: String,
    trace_id: String,
    span_id: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    match loom_core::traces_get_span(&loom, ns, &trace_id, &span_id).map_err(reason)? {
        Some(span) => Ok(Some(Uint8Array::from(span.encode().map_err(reason)?))),
        None => Ok(None),
    }
}

#[napi]
pub fn traces_trace_spans(
    loom_path: String,
    workspace: String,
    trace_id: String,
    max_spans: u32,
    max_output_bytes: BigInt,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(
        loom_core::traces_trace_spans(
            &loom,
            ns,
            &trace_id,
            max_spans,
            bigint_to_u64(max_output_bytes, "max_output_bytes")?,
        )
        .and_then(traces_query_result_cbor)
        .map_err(reason)?,
    ))
}

#[napi]
pub fn traces_query(
    loom_path: String,
    workspace: String,
    from_start_time_ns: BigInt,
    to_start_time_ns: BigInt,
    max_spans: u32,
    max_output_bytes: BigInt,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let query = loom_core::TraceQuery {
        from_start_time_ns: bigint_to_u64(from_start_time_ns, "from_start_time_ns")?,
        to_start_time_ns: bigint_to_u64(to_start_time_ns, "to_start_time_ns")?,
        max_spans,
        max_output_bytes: bigint_to_u64(max_output_bytes, "max_output_bytes")?,
    };
    Ok(Uint8Array::from(
        loom_core::traces_query(&loom, ns, &query)
            .and_then(traces_query_result_cbor)
            .map_err(reason)?,
    ))
}
