use loom_core::{
    FacetKind, LogQuery, Loom, MemoryStore, Result, WorkspaceId, logs_get_record, logs_put_record,
    logs_query,
};
use loom_logs::{LogRecord, LogSeverityNumber, LogTraceContext, LogValue};
use std::collections::BTreeMap;

pub struct LogCanonicalVector {
    pub name: &'static str,
    pub record: LogRecord,
    pub expect_record_id: &'static str,
    pub expect_record_canonical: &'static str,
}

pub struct LogNegativeVector {
    pub name: &'static str,
    pub canonical: &'static str,
}

pub fn log_canonical_vectors() -> Result<Vec<LogCanonicalVector>> {
    let record = LogRecord::new(
        1_725_000_000_000_000_000,
        Some(1_725_000_000_010_000_000),
        LogSeverityNumber::new(13)?,
        "WARN".into(),
        LogValue::String("cache miss".into()),
    )?
    .with_context(
        BTreeMap::from([
            ("cache.hit".into(), LogValue::Bool(false)),
            ("latency.ms".into(), LogValue::Float(12.5)),
        ]),
        BTreeMap::from([("service.name".into(), LogValue::String("api".into()))]),
        BTreeMap::from([
            ("name".into(), LogValue::String("loom".into())),
            ("version".into(), LogValue::String("0.1.0".into())),
        ]),
        Some(LogTraceContext::new(
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            [17, 18, 19, 20, 21, 22, 23, 24],
            1,
        )?),
    )?;
    Ok(vec![LogCanonicalVector {
        name: "structured-warning-with-trace",
        record,
        expect_record_id: "3c9952e0bb89d3e9fdb4892b314f1ac7fc05ea9059a6f68787f4ab8e4858d8c0",
        expect_record_canonical: "8a736c6f6f6d2e6c6f67732e7265636f72642e76311b17f06e5c4d8c80001b17f06e5c4e2516800d645741524e8266737472696e676a6361636865206d697373a26963616368652e6869748264626f6f6cf46a6c6174656e63792e6d738265666c6f6174fb4029000000000000a16c736572766963652e6e616d658266737472696e6763617069a2646e616d658266737472696e67646c6f6f6d6776657273696f6e8266737472696e6765302e312e3083500102030405060708090a0b0c0d0e0f1048111213141516171801",
    }])
}

pub const LOG_NEGATIVE_VECTORS: &[LogNegativeVector] = &[
    LogNegativeVector {
        name: "wrong-schema",
        canonical: "816178",
    },
    LogNegativeVector {
        name: "zero-timestamp",
        canonical: "8a736c6f6f6d2e6c6f67732e7265636f72642e763100f60d645741524e8266737472696e676a6361636865206d697373a0a0a0f6",
    },
    LogNegativeVector {
        name: "invalid-severity",
        canonical: "8a736c6f6f6d2e6c6f67732e7265636f72642e763101f600645741524e8266737472696e676a6361636865206d697373a0a0a0f6",
    },
    LogNegativeVector {
        name: "zero-trace-id",
        canonical: "8a736c6f6f6d2e6c6f67732e7265636f72642e763101f60d645741524e8266737472696e676a6361636865206d697373a0a0a083500000000000000000000000000000000048010101010101010101",
    },
];

pub fn run_logs_behavior() -> Result<()> {
    for vector in log_canonical_vectors()? {
        let encoded = vector.record.encode()?;
        assert_eq!(
            hex::encode(&encoded),
            vector.expect_record_canonical,
            "log record canonical bytes mismatch for '{}'",
            vector.name
        );
        assert_eq!(
            vector.record.record_id()?,
            vector.expect_record_id,
            "log record identity mismatch for '{}'",
            vector.name
        );
        assert_eq!(
            LogRecord::decode(&encoded)?,
            vector.record,
            "log record canonical round-trip mismatch for '{}'",
            vector.name
        );
    }
    for vector in LOG_NEGATIVE_VECTORS {
        let bytes = hex::decode(vector.canonical)
            .map_err(|err| loom_core::LoomError::invalid(format!("invalid log hex: {err}")))?;
        assert!(
            LogRecord::decode(&bytes).is_err(),
            "invalid log vector '{}' unexpectedly decoded",
            vector.name
        );
    }
    let mut loom = Loom::new(MemoryStore::new());
    let ns =
        loom.registry_mut()
            .create(FacetKind::Logs, None, WorkspaceId::from_bytes([0x6c; 16]))?;
    let vectors = log_canonical_vectors()?;
    let record = vectors[0].record.clone();
    let record_id = logs_put_record(&mut loom, ns, &record)?;
    assert_eq!(
        logs_get_record(&loom, ns, &record_id)?,
        Some(record.clone())
    );
    let query = logs_query(
        &loom,
        ns,
        &LogQuery {
            from_time_unix_nano: record.timestamp_ns.saturating_sub(1),
            to_time_unix_nano: record.timestamp_ns.saturating_add(1),
            max_records: 1,
            max_output_bytes: record.encode()?.len() as u64,
        },
    )?;
    assert_eq!(query.records, vec![record]);
    assert!(!query.partial);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_behavior_passes() {
        run_logs_behavior().expect("logs behavior must pass");
    }
}
