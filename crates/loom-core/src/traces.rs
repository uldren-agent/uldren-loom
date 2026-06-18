//! Persistent native trace spans.

use crate::acl::AclRight;
use crate::error::{Code, Result};
use crate::provider::ObjectStore;
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};
use loom_traces::SpanRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceQuery {
    pub from_start_time_ns: u64,
    pub to_start_time_ns: u64,
    pub max_spans: u32,
    pub max_output_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraceQueryResult {
    pub spans: Vec<SpanRecord>,
    pub partial: bool,
}

fn trace_id_path_component(trace_id_hex: &str) -> Result<&str> {
    if trace_id_hex.len() != 32
        || trace_id_hex
            .bytes()
            .any(|byte| !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase())
    {
        return Err(crate::LoomError::invalid("trace id is invalid"));
    }
    Ok(trace_id_hex)
}

fn span_id_path_component(span_id_hex: &str) -> Result<&str> {
    if span_id_hex.len() != 16
        || span_id_hex
            .bytes()
            .any(|byte| !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase())
    {
        return Err(crate::LoomError::invalid("span id is invalid"));
    }
    Ok(span_id_hex)
}

fn trace_dir(trace_id_hex: &str) -> Result<String> {
    Ok(facet_path(
        FacetKind::Traces,
        &format!("traces/{}", trace_id_path_component(trace_id_hex)?),
    ))
}

fn span_path(trace_id_hex: &str, span_id_hex: &str) -> Result<String> {
    Ok(format!(
        "{}/{}",
        trace_dir(trace_id_hex)?,
        span_id_path_component(span_id_hex)?
    ))
}

fn read_span<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    path: &str,
) -> Result<Option<SpanRecord>> {
    match loom.read_file_reserved(ns, path) {
        Ok(bytes) => SpanRecord::decode(&bytes).map(Some),
        Err(error) if error.code == Code::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn traces_put_span<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    span: &SpanRecord,
) -> Result<()> {
    span.validate()?;
    let trace_id = span.trace_id_hex();
    let span_id = span.span_id_hex();
    let path = span_path(&trace_id, &span_id)?;
    loom.authorize_facet_path(ns, FacetKind::Traces, &path, AclRight::Write)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Traces), true)?;
    loom.create_directory_reserved(ns, &facet_path(FacetKind::Traces, "traces"), true)?;
    loom.create_directory_reserved(ns, &trace_dir(&trace_id)?, true)?;
    loom.write_file_reserved(ns, &path, &span.encode()?, 0o100644)
}

pub fn traces_get_span<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    trace_id_hex: &str,
    span_id_hex: &str,
) -> Result<Option<SpanRecord>> {
    let path = span_path(trace_id_hex, span_id_hex)?;
    loom.authorize_facet_path(ns, FacetKind::Traces, &path, AclRight::Read)?;
    read_span(loom, ns, &path)
}

pub fn traces_trace_spans<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    trace_id_hex: &str,
    max_spans: u32,
    max_output_bytes: u64,
) -> Result<TraceQueryResult> {
    if max_spans == 0 || max_output_bytes == 0 {
        return Err(crate::LoomError::invalid("trace query bounds are invalid"));
    }
    let dir = trace_dir(trace_id_hex)?;
    loom.authorize_facet_path(ns, FacetKind::Traces, &dir, AclRight::Read)?;
    let entries = match loom.list_directory(ns, &dir) {
        Ok(entries) => entries,
        Err(error) if error.code == Code::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    bounded_spans_from_entries(
        loom,
        ns,
        trace_id_hex,
        entries,
        None,
        max_spans,
        max_output_bytes,
    )
}

pub fn traces_query<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    query: &TraceQuery,
) -> Result<TraceQueryResult> {
    if query.from_start_time_ns >= query.to_start_time_ns
        || query.max_spans == 0
        || query.max_output_bytes == 0
    {
        return Err(crate::LoomError::invalid("trace query bounds are invalid"));
    }
    let root = facet_path(FacetKind::Traces, "traces");
    loom.authorize_facet_path(ns, FacetKind::Traces, &root, AclRight::Read)?;
    let traces = match loom.list_directory(ns, &root) {
        Ok(entries) => entries,
        Err(error) if error.code == Code::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    let mut spans = Vec::new();
    for trace in traces {
        let dir = trace_dir(&trace.name)?;
        for entry in loom.list_directory(ns, &dir)? {
            let path = span_path(&trace.name, &entry.name)?;
            let span = SpanRecord::decode(&loom.read_file_reserved(ns, &path)?)?;
            if span.start_time_ns < query.from_start_time_ns
                || span.start_time_ns >= query.to_start_time_ns
            {
                continue;
            }
            spans.push(span);
        }
    }
    spans.sort_by_key(|span| (span.start_time_ns, span.trace_id_hex(), span.span_id_hex()));
    apply_bounds(spans, query.max_spans, query.max_output_bytes)
}

fn bounded_spans_from_entries<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    trace_id_hex: &str,
    entries: Vec<crate::fs::DirEntry>,
    range: Option<(u64, u64)>,
    max_spans: u32,
    max_output_bytes: u64,
) -> Result<TraceQueryResult> {
    let mut spans = Vec::new();
    for entry in entries {
        let path = span_path(trace_id_hex, &entry.name)?;
        let span = SpanRecord::decode(&loom.read_file_reserved(ns, &path)?)?;
        if let Some((from, to)) = range
            && (span.start_time_ns < from || span.start_time_ns >= to)
        {
            continue;
        }
        spans.push(span);
    }
    spans.sort_by_key(|span| (span.start_time_ns, span.span_id_hex()));
    apply_bounds(spans, max_spans, max_output_bytes)
}

fn apply_bounds(
    spans: Vec<SpanRecord>,
    max_spans: u32,
    max_output_bytes: u64,
) -> Result<TraceQueryResult> {
    let mut bounded = Vec::new();
    let mut output_bytes = 0_u64;
    let mut partial = false;
    for span in spans {
        let encoded_len = span.encode()?.len() as u64;
        if output_bytes.saturating_add(encoded_len) > max_output_bytes
            || bounded.len() >= max_spans as usize
        {
            partial = true;
            break;
        }
        output_bytes = output_bytes.saturating_add(encoded_len);
        bounded.push(span);
    }
    Ok(TraceQueryResult {
        spans: bounded,
        partial,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::memory::MemoryStore;
    use crate::workspace::FacetKind;
    use loom_traces::{SpanContext, SpanDetails, SpanEvent, SpanKind, SpanStatusCode, TraceValue};
    use std::collections::BTreeMap;

    fn span(span_id: [u8; 8], start: u64) -> SpanRecord {
        SpanRecord::new(
            SpanContext::new([1; 16], span_id, 1).unwrap(),
            "GET /items".into(),
            SpanKind::Server,
            start,
            start + 10,
        )
        .unwrap()
        .with_details(SpanDetails {
            observed_time_ns: Some(start + 20),
            status_code: SpanStatusCode::Ok,
            attributes: BTreeMap::from([("http.method".into(), TraceValue::String("GET".into()))]),
            resource: BTreeMap::from([("service.name".into(), TraceValue::String("api".into()))]),
            scope: BTreeMap::from([("name".into(), TraceValue::String("loom".into()))]),
            events: vec![SpanEvent::new(start + 1, "event".into(), BTreeMap::new()).unwrap()],
            ..SpanDetails::default()
        })
        .unwrap()
    }

    #[test]
    fn spans_round_trip_and_query_is_bounded() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Traces, None, WorkspaceId::from_bytes([12; 16]))
            .unwrap();
        let first = span([2; 8], 10);
        let second = span([3; 8], 30);
        traces_put_span(&mut loom, ns, &first).unwrap();
        traces_put_span(&mut loom, ns, &second).unwrap();
        assert_eq!(
            traces_get_span(&loom, ns, &first.trace_id_hex(), &first.span_id_hex()).unwrap(),
            Some(first.clone())
        );
        let trace = traces_trace_spans(&loom, ns, &first.trace_id_hex(), 1, 4096).unwrap();
        assert_eq!(trace.spans, vec![first]);
        assert!(trace.partial);
        let query = traces_query(
            &loom,
            ns,
            &TraceQuery {
                from_start_time_ns: 20,
                to_start_time_ns: 40,
                max_spans: 4,
                max_output_bytes: 4096,
            },
        )
        .unwrap();
        assert_eq!(query.spans, vec![second]);
        assert!(!query.partial);
    }
}
