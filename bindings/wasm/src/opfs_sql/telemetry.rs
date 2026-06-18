//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

fn ensure_metrics_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
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
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Metrics)
        .map_err(le)?;
    Ok(ns)
}

fn ensure_logs_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
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
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Logs)
        .map_err(le)?;
    Ok(ns)
}

fn ensure_traces_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
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
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Traces)
        .map_err(le)?;
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

#[wasm_bindgen]
impl LoomSql {
    pub fn metrics_put_descriptor(
        &mut self,
        workspace: String,
        descriptor: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_metrics_ns(&mut self.loom, &workspace)?;
        let descriptor = loom_core::MetricDescriptor::decode(&descriptor).map_err(le)?;
        loom_core::metrics_put_descriptor(&mut self.loom, ns, &descriptor).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    pub fn metrics_get_descriptor(
        &self,
        workspace: String,
        name: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        match loom_core::metrics_get_descriptor(&self.loom, ns, &name).map_err(le)? {
            Some(descriptor) => Ok(Some(Uint8Array::from(
                descriptor.encode().map_err(le)?.as_slice(),
            ))),
            None => Ok(None),
        }
    }

    pub fn metrics_put_observation(
        &mut self,
        workspace: String,
        descriptor_name: String,
        observation: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_metrics_ns(&mut self.loom, &workspace)?;
        let observation = loom_core::MetricObservation::decode(&observation).map_err(le)?;
        loom_core::metrics_put_observation(&mut self.loom, ns, &descriptor_name, &observation)
            .map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    pub fn metrics_query(
        &self,
        workspace: String,
        descriptor_name: String,
        from_timestamp_ms: u64,
        to_timestamp_ms: u64,
        max_series: u32,
        max_groups: u32,
        max_samples: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let query = loom_core::MetricQuery {
            from_timestamp_ms,
            to_timestamp_ms,
            max_series,
            max_groups,
            max_samples,
            max_output_bytes,
            now_timestamp_ms: now_ms(),
        };
        loom_core::metrics_query_observations(&self.loom, ns, &descriptor_name, &query)
            .and_then(metrics_query_result_cbor)
            .map_err(le)
    }

    pub fn logs_put_record(
        &mut self,
        workspace: String,
        record: Vec<u8>,
    ) -> Result<String, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_logs_ns(&mut self.loom, &workspace)?;
        let record = loom_core::LogRecord::decode(&record).map_err(le)?;
        let id = loom_core::logs_put_record(&mut self.loom, ns, &record).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(id)
    }

    pub fn logs_get_record(
        &self,
        workspace: String,
        record_id: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        match loom_core::logs_get_record(&self.loom, ns, &record_id).map_err(le)? {
            Some(record) => Ok(Some(Uint8Array::from(
                record.encode().map_err(le)?.as_slice(),
            ))),
            None => Ok(None),
        }
    }

    pub fn logs_query(
        &self,
        workspace: String,
        from_time_unix_nano: u64,
        to_time_unix_nano: u64,
        max_records: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let query = loom_core::LogQuery {
            from_time_unix_nano,
            to_time_unix_nano,
            max_records,
            max_output_bytes,
        };
        loom_core::logs_query(&self.loom, ns, &query)
            .and_then(logs_query_result_cbor)
            .map_err(le)
    }

    pub fn traces_put_span(&mut self, workspace: String, span: Vec<u8>) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_traces_ns(&mut self.loom, &workspace)?;
        let span = loom_core::SpanRecord::decode(&span).map_err(le)?;
        loom_core::traces_put_span(&mut self.loom, ns, &span).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    pub fn traces_get_span(
        &self,
        workspace: String,
        trace_id: String,
        span_id: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        match loom_core::traces_get_span(&self.loom, ns, &trace_id, &span_id).map_err(le)? {
            Some(span) => Ok(Some(Uint8Array::from(
                span.encode().map_err(le)?.as_slice(),
            ))),
            None => Ok(None),
        }
    }

    pub fn traces_trace_spans(
        &self,
        workspace: String,
        trace_id: String,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        loom_core::traces_trace_spans(&self.loom, ns, &trace_id, max_spans, max_output_bytes)
            .and_then(traces_query_result_cbor)
            .map_err(le)
    }

    pub fn traces_query(
        &self,
        workspace: String,
        from_start_time_ns: u64,
        to_start_time_ns: u64,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let query = loom_core::TraceQuery {
            from_start_time_ns,
            to_start_time_ns,
            max_spans,
            max_output_bytes,
        };
        loom_core::traces_query(&self.loom, ns, &query)
            .and_then(traces_query_result_cbor)
            .map_err(le)
    }
}
