//! Persistent native log records.

use crate::acl::AclRight;
use crate::error::{Code, Result};
use crate::provider::ObjectStore;
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};
use loom_logs::LogRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogQuery {
    pub from_time_unix_nano: u64,
    pub to_time_unix_nano: u64,
    pub max_records: u32,
    pub max_output_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogQueryResult {
    pub records: Vec<LogRecord>,
    pub partial: bool,
}

fn record_id_path_component(record_id: &str) -> Result<&str> {
    if record_id.len() != 64
        || record_id
            .bytes()
            .any(|byte| !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase())
    {
        return Err(crate::LoomError::invalid("log record id is invalid"));
    }
    Ok(record_id)
}

fn record_path(record_id: &str) -> Result<String> {
    Ok(facet_path(
        FacetKind::Logs,
        &format!("records/{}", record_id_path_component(record_id)?),
    ))
}

fn records_root() -> String {
    facet_path(FacetKind::Logs, "records")
}

pub fn logs_put_record<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    record: &LogRecord,
) -> Result<String> {
    record.validate()?;
    let record_id = record.record_id()?;
    let path = record_path(&record_id)?;
    loom.authorize_facet_path(ns, FacetKind::Logs, &path, AclRight::Write)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Logs), true)?;
    loom.create_directory_reserved(ns, &records_root(), true)?;
    loom.write_file_reserved(ns, &path, &record.encode()?, 0o100644)?;
    Ok(record_id)
}

pub fn logs_get_record<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    record_id: &str,
) -> Result<Option<LogRecord>> {
    let path = record_path(record_id)?;
    loom.authorize_facet_path(ns, FacetKind::Logs, &path, AclRight::Read)?;
    match loom.read_file_reserved(ns, &path) {
        Ok(bytes) => LogRecord::decode(&bytes).map(Some),
        Err(error) if error.code == Code::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn logs_query<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    query: &LogQuery,
) -> Result<LogQueryResult> {
    if query.from_time_unix_nano >= query.to_time_unix_nano
        || query.max_records == 0
        || query.max_output_bytes == 0
    {
        return Err(crate::LoomError::invalid("log query bounds are invalid"));
    }
    let root = records_root();
    loom.authorize_facet_path(ns, FacetKind::Logs, &root, AclRight::Read)?;
    let entries = match loom.list_directory(ns, &root) {
        Ok(entries) => entries,
        Err(error) if error.code == Code::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    let mut records = Vec::new();
    for entry in entries {
        let path = record_path(&entry.name)?;
        let record = LogRecord::decode(&loom.read_file_reserved(ns, &path)?)?;
        if record.timestamp_ns < query.from_time_unix_nano
            || record.timestamp_ns >= query.to_time_unix_nano
        {
            continue;
        }
        records.push(record);
    }
    records.sort_by_key(|record| {
        (
            record.timestamp_ns,
            record.record_id().unwrap_or_else(|_| String::new()),
        )
    });
    apply_bounds(records, query.max_records, query.max_output_bytes)
}

fn apply_bounds(
    records: Vec<LogRecord>,
    max_records: u32,
    max_output_bytes: u64,
) -> Result<LogQueryResult> {
    let mut bounded = Vec::new();
    let mut output_bytes = 0_u64;
    let mut partial = false;
    for record in records {
        let encoded_len = record.encode()?.len() as u64;
        if output_bytes.saturating_add(encoded_len) > max_output_bytes
            || bounded.len() >= max_records as usize
        {
            partial = true;
            break;
        }
        output_bytes = output_bytes.saturating_add(encoded_len);
        bounded.push(record);
    }
    Ok(LogQueryResult {
        records: bounded,
        partial,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::memory::MemoryStore;
    use crate::workspace::FacetKind;
    use loom_logs::{LogSeverityNumber, LogValue};
    use std::collections::BTreeMap;

    fn record(time_unix_nano: u64) -> LogRecord {
        LogRecord::new(
            time_unix_nano,
            Some(time_unix_nano + 10),
            LogSeverityNumber::new(13).unwrap(),
            "WARN".into(),
            LogValue::String("cache miss".into()),
        )
        .unwrap()
        .with_context(
            BTreeMap::from([("cache.hit".into(), LogValue::Bool(false))]),
            BTreeMap::from([("service.name".into(), LogValue::String("api".into()))]),
            BTreeMap::from([("name".into(), LogValue::String("loom".into()))]),
            None,
        )
        .unwrap()
    }

    #[test]
    fn records_round_trip_and_query_is_bounded() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Logs, None, WorkspaceId::from_bytes([13; 16]))
            .unwrap();
        let first = record(10);
        let second = record(30);
        let first_id = logs_put_record(&mut loom, ns, &first).unwrap();
        logs_put_record(&mut loom, ns, &second).unwrap();
        assert_eq!(
            logs_get_record(&loom, ns, &first_id).unwrap(),
            Some(first.clone())
        );
        let query = logs_query(
            &loom,
            ns,
            &LogQuery {
                from_time_unix_nano: 0,
                to_time_unix_nano: 40,
                max_records: 1,
                max_output_bytes: 4096,
            },
        )
        .unwrap();
        assert_eq!(query.records, vec![first]);
        assert!(query.partial);
    }
}
