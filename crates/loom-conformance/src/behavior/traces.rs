use loom_core::{
    FacetKind, Loom, MemoryStore, Result, TraceQuery, WorkspaceId, traces_get_span,
    traces_put_span, traces_query, traces_trace_spans,
};
use loom_traces::{
    SpanContext, SpanDetails, SpanEvent, SpanKind, SpanLink, SpanRecord, SpanStatusCode, TraceValue,
};
use std::collections::BTreeMap;

pub struct TraceCanonicalVector {
    pub name: &'static str,
    pub span: SpanRecord,
    pub expect_record_id: &'static str,
    pub expect_record_canonical: &'static str,
}

pub struct TraceNegativeVector {
    pub name: &'static str,
    pub canonical: &'static str,
}

pub fn trace_canonical_vectors() -> Result<Vec<TraceCanonicalVector>> {
    let span = sample_span()?;
    Ok(vec![TraceCanonicalVector {
        name: "server-span-with-event-and-link",
        span,
        expect_record_id: "2aa77044066227e32ae911318ac87807f19e4576bf7363acb053a325ddb6ca1f",
        expect_record_canonical: "8f736c6f6f6d2e7472616365732e7370616e2e76318350101112131415161718191a1b1c1d1e1f482021222324252627014850515253545556576d474554202f76312f6974656d73667365727665721b17f06e5c4d8c80001b17f06e5c4e2516801b17f06e5c4e3458c0626f6b60a26b687474702e6d6574686f648266737472696e676347455470687474702e7374617475735f636f64658263696e7418c8a26c736572766963652e6e616d658266737472696e676b636174616c6f672d6170696f736572766963652e76657273696f6e8266737472696e6765312e322e33a2646e616d658266737472696e676b6c6f6f6d2e6e61746976656776657273696f6e8266737472696e6765302e312e3081831b17f06e5c4dd8cb406864622e7175657279a264726f77738263696e74036964622e73797374656d8266737472696e676673716c69746581828350303132333435363738393a3b3c3d3e3f48404142434445464700a1696c696e6b2e6b696e648266737472696e676c666f6c6c6f77735f66726f6d",
    }])
}

pub const TRACE_NEGATIVE_VECTORS: &[TraceNegativeVector] = &[
    TraceNegativeVector {
        name: "wrong-schema",
        canonical: "816178",
    },
    TraceNegativeVector {
        name: "zero-trace-id",
        canonical: "8f736c6f6f6d2e7472616365732e7370616e2e7631835000000000000000000000000000000000482021222324252627014850515253545556576d474554202f76312f6974656d73667365727665721b17f06e5c4d8c80001b17f06e5c4e2516801b17f06e5c4e3458c0626f6b60a26b687474702e6d6574686f648266737472696e676347455470687474702e7374617475735f636f64658263696e7418c8a26c736572766963652e6e616d658266737472696e676b636174616c6f672d6170696f736572766963652e76657273696f6e8266737472696e6765312e322e33a2646e616d658266737472696e676b6c6f6f6d2e6e61746976656776657273696f6e8266737472696e6765302e312e3081831b17f06e5c4dd8cb406864622e7175657279a264726f77738263696e74036964622e73797374656d8266737472696e676673716c69746581828350303132333435363738393a3b3c3d3e3f48404142434445464700a1696c696e6b2e6b696e648266737472696e676c666f6c6c6f77735f66726f6d",
    },
    TraceNegativeVector {
        name: "parent-is-self",
        canonical: "8f736c6f6f6d2e7472616365732e7370616e2e76318350101112131415161718191a1b1c1d1e1f482021222324252627014820212223242526276d474554202f76312f6974656d73667365727665721b17f06e5c4d8c80001b17f06e5c4e2516801b17f06e5c4e3458c0626f6b60a26b687474702e6d6574686f648266737472696e676347455470687474702e7374617475735f636f64658263696e7418c8a26c736572766963652e6e616d658266737472696e676b636174616c6f672d6170696f736572766963652e76657273696f6e8266737472696e6765312e322e33a2646e616d658266737472696e676b6c6f6f6d2e6e61746976656776657273696f6e8266737472696e6765302e312e3081831b17f06e5c4dd8cb406864622e7175657279a264726f77738263696e74036964622e73797374656d8266737472696e676673716c69746581828350303132333435363738393a3b3c3d3e3f48404142434445464700a1696c696e6b2e6b696e648266737472696e676c666f6c6c6f77735f66726f6d",
    },
];

pub fn run_traces_behavior() -> Result<()> {
    for vector in trace_canonical_vectors()? {
        let encoded = vector.span.encode()?;
        assert_eq!(
            hex::encode(&encoded),
            vector.expect_record_canonical,
            "trace span canonical bytes mismatch for '{}'",
            vector.name
        );
        assert_eq!(
            vector.span.record_id()?,
            vector.expect_record_id,
            "trace span identity mismatch for '{}'",
            vector.name
        );
        assert_eq!(
            SpanRecord::decode(&encoded)?,
            vector.span,
            "trace span canonical round-trip mismatch for '{}'",
            vector.name
        );
    }
    for vector in TRACE_NEGATIVE_VECTORS {
        let bytes = hex::decode(vector.canonical)
            .map_err(|err| loom_core::LoomError::invalid(format!("invalid trace hex: {err}")))?;
        assert!(
            SpanRecord::decode(&bytes).is_err(),
            "invalid trace vector '{}' unexpectedly decoded",
            vector.name
        );
    }
    let mut loom = Loom::new(MemoryStore::new());
    let ns =
        loom.registry_mut()
            .create(FacetKind::Traces, None, WorkspaceId::from_bytes([0x74; 16]))?;
    let first = sample_span()?;
    let second = SpanRecord::new(
        SpanContext::new(first.context.trace_id, [0x60; 8], 1)?,
        "GET /v1/items/1".into(),
        SpanKind::Client,
        first.end_time_ns + 1,
        first.end_time_ns + 10,
    )?;
    traces_put_span(&mut loom, ns, &first)?;
    traces_put_span(&mut loom, ns, &second)?;
    assert_eq!(
        traces_get_span(&loom, ns, &first.trace_id_hex(), &first.span_id_hex())?,
        Some(first.clone())
    );
    let bounded_trace = traces_trace_spans(&loom, ns, &first.trace_id_hex(), 1, 4096)?;
    assert_eq!(bounded_trace.spans, vec![first.clone()]);
    assert!(bounded_trace.partial);
    let bounded_query = traces_query(
        &loom,
        ns,
        &TraceQuery {
            from_start_time_ns: first.start_time_ns,
            to_start_time_ns: second.end_time_ns + 1,
            max_spans: 8,
            max_output_bytes: first.encode()?.len() as u64,
        },
    )?;
    assert_eq!(bounded_query.spans, vec![first]);
    assert!(bounded_query.partial);
    Ok(())
}

fn sample_span() -> Result<SpanRecord> {
    let context = SpanContext::new(
        [
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
            0x1e, 0x1f,
        ],
        [0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27],
        1,
    )?;
    let event = SpanEvent::new(
        1_725_000_000_005_000_000,
        "db.query".into(),
        BTreeMap::from([
            ("db.system".into(), TraceValue::String("sqlite".into())),
            ("rows".into(), TraceValue::Int(3)),
        ]),
    )?;
    let link = SpanLink::new(
        SpanContext::new(
            [
                0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d,
                0x3e, 0x3f,
            ],
            [0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47],
            0,
        )?,
        BTreeMap::from([(
            "link.kind".into(),
            TraceValue::String("follows_from".into()),
        )]),
    )?;
    SpanRecord::new(
        context,
        "GET /v1/items".into(),
        SpanKind::Server,
        1_725_000_000_000_000_000,
        1_725_000_000_010_000_000,
    )?
    .with_details(SpanDetails {
        parent_span_id: Some([0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57]),
        observed_time_ns: Some(1_725_000_000_011_000_000),
        status_code: SpanStatusCode::Ok,
        attributes: BTreeMap::from([
            ("http.method".into(), TraceValue::String("GET".into())),
            ("http.status_code".into(), TraceValue::Int(200)),
        ]),
        resource: BTreeMap::from([
            (
                "service.name".into(),
                TraceValue::String("catalog-api".into()),
            ),
            ("service.version".into(), TraceValue::String("1.2.3".into())),
        ]),
        scope: BTreeMap::from([
            ("name".into(), TraceValue::String("loom.native".into())),
            ("version".into(), TraceValue::String("0.1.0".into())),
        ]),
        events: vec![event],
        links: vec![link],
        ..SpanDetails::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_behavior_passes() {
        run_traces_behavior().expect("traces behavior must pass");
    }
}
