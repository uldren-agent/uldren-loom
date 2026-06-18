//! Local-vs-remote `LoomClient` parity suite.
//!
//! A single deterministic operation sequence, expressed once against a small [`ParityDriver`] adapter that
//! both a local and a remote client can satisfy. Running the suite against each driver yields a
//! [`ParityReport`] of labeled, observable outputs; the two reports are byte-for-byte equal for the covered
//! families. The suite compares real observable values (returned bytes, digests, sequence numbers,
//! timestamped points), not just call success, and is deterministic - fixed workspace/collection names and
//! a fixed `timestamp_ms` for the content-addressed VCS commit - so the local and remote reports are
//! directly comparable.
//!
//! The operation sequence and assertions live here, driver-agnostic. [`LocalClientDriver`] drives an
//! in-process [`loom_client::LocalLoomClient`]; a remote socket driver in the loom-cli live-test path stands
//! up `loom serve remote` and implements this same [`ParityDriver`].
//!
//! Covered families: store version, KV, CAS, Queue, Document read + `query_json`, TimeSeries `latest`, a
//! digest-sensitive VCS timestamped commit, and a committed-table SQL read (`sql_open` -> `sql_exec` ->
//! `sql_commit` -> `sql_close`, then the read-only `sql_query_result`). The host-composite `document_query`
//! and the streaming/session SQL and Watch surfaces are covered elsewhere, not by this unary suite.

/// An ordered set of labeled, observable outputs captured while running the parity suite. Two reports from
/// two drivers are equal iff every family produced identical observable output. Scalars are encoded into
/// bytes deterministically so the comparison is a plain `==`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ParityReport {
    pub entries: Vec<(String, Vec<u8>)>,
}

impl ParityReport {
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, label: &str, bytes: Vec<u8>) {
        self.entries.push((label.to_string(), bytes));
    }

    fn text(&mut self, label: &str, value: &str) {
        self.push(label, value.as_bytes().to_vec());
    }

    /// A raw byte payload recorded verbatim (e.g. a canonical-CBOR/JSON result body).
    fn bytes(&mut self, label: &str, value: Vec<u8>) {
        self.push(label, value);
    }

    fn u64(&mut self, label: &str, value: u64) {
        self.push(label, value.to_le_bytes().to_vec());
    }

    /// Tagged encoding of an optional byte string: `[0]` for `None`, `[1, ..bytes]` for `Some`.
    fn opt_bytes(&mut self, label: &str, value: Option<Vec<u8>>) {
        let mut out = Vec::new();
        match value {
            None => out.push(0u8),
            Some(bytes) => {
                out.push(1u8);
                out.extend_from_slice(&bytes);
            }
        }
        self.push(label, out);
    }

    /// Tagged encoding of an optional timeseries point: `[0]` for `None`, `[1, ts_le(8), ..value]`.
    fn opt_point(&mut self, label: &str, value: Option<(i64, Vec<u8>)>) {
        let mut out = Vec::new();
        match value {
            None => out.push(0u8),
            Some((ts, bytes)) => {
                out.push(1u8);
                out.extend_from_slice(&ts.to_le_bytes());
                out.extend_from_slice(&bytes);
            }
        }
        self.push(label, out);
    }
}

/// The minimal operation surface the parity suite drives. Both a local `LoomClient` and a remote one can
/// satisfy it (the remote driver blocks on the async client, mirroring the CLI facade). Every op returns a
/// stringified error so drivers over different error types unify. The driver owns its session/connection.
pub trait ParityDriver {
    fn store_version(&self) -> Result<String, String>;

    fn kv_put(&self, ws: &str, collection: &str, key: &[u8], value: &[u8]) -> Result<(), String>;
    fn kv_get(&self, ws: &str, collection: &str, key: &[u8]) -> Result<Option<Vec<u8>>, String>;

    fn cas_put(&self, ws: &str, content: &[u8]) -> Result<String, String>;
    fn cas_get(&self, ws: &str, digest: &str) -> Result<Option<Vec<u8>>, String>;

    fn queue_append(&self, ws: &str, stream: &str, entry: &[u8]) -> Result<u64, String>;
    fn queue_get(&self, ws: &str, stream: &str, seq: u64) -> Result<Option<Vec<u8>>, String>;

    fn document_put_binary_bytes(
        &self,
        ws: &str,
        collection: &str,
        id: &str,
        doc: &[u8],
    ) -> Result<(), String>;
    fn document_get_binary_bytes(
        &self,
        ws: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>, String>;
    fn document_query_json(
        &self,
        ws: &str,
        collection: &str,
        query_json: &[u8],
    ) -> Result<Vec<u8>, String>;

    /// Document text/binary surface. `put_*` return the content digest string; `get_*` return the
    /// canonical-CBOR `[value, digest]` result (or `None`); `list_binary` returns the encoded collection.
    /// Deterministic given fixed content, so directly comparable local vs remote.
    fn document_put_text(
        &self,
        ws: &str,
        collection: &str,
        id: &str,
        text: &str,
    ) -> Result<String, String>;
    fn document_get_text(
        &self,
        ws: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>, String>;
    fn document_put_binary(
        &self,
        ws: &str,
        collection: &str,
        id: &str,
        bytes: &[u8],
    ) -> Result<String, String>;
    fn document_get_binary(
        &self,
        ws: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>, String>;
    fn document_list_binary(&self, ws: &str, collection: &str) -> Result<Vec<u8>, String>;

    /// PIM imports. A collection/book must exist first (engine contract), so the suite creates it, then
    /// `put_ics`/`put_vcard` import a fixed document and return the content-addressed etag string -
    /// deterministic, so directly comparable local vs remote. `create_*` take the encoded meta record.
    fn calendar_create_collection(
        &self,
        ws: &str,
        principal: &str,
        collection: &str,
        meta: &[u8],
    ) -> Result<(), String>;
    fn calendar_put_ics(
        &self,
        ws: &str,
        principal: &str,
        collection: &str,
        ics: &str,
    ) -> Result<String, String>;
    fn contacts_create_book(
        &self,
        ws: &str,
        principal: &str,
        book: &str,
        meta: &[u8],
    ) -> Result<(), String>;
    fn contacts_put_vcard(
        &self,
        ws: &str,
        principal: &str,
        book: &str,
        vcard: &str,
    ) -> Result<String, String>;

    /// Metrics: register a descriptor (encoded `MetricDescriptor`), read it back, append one encoded
    /// `MetricObservation`, then query the observation window. `get_descriptor` returns the encoded record
    /// (or `None`); `query` returns canonical CBOR `[observations, partial, stale]`. With fixed descriptor
    /// + observation (fixed timestamp/value) + fixed query bounds, all outputs are deterministic.
    fn metrics_put_descriptor(&self, ws: &str, descriptor: &[u8]) -> Result<(), String>;
    fn metrics_get_descriptor(&self, ws: &str, name: &str) -> Result<Option<Vec<u8>>, String>;
    fn metrics_put_observation(
        &self,
        ws: &str,
        descriptor_name: &str,
        observation: &[u8],
    ) -> Result<(), String>;
    #[allow(clippy::too_many_arguments)]
    fn metrics_query(
        &self,
        ws: &str,
        descriptor_name: &str,
        from_timestamp_ms: u64,
        to_timestamp_ms: u64,
        max_series: u32,
        max_groups: u32,
        max_samples: u32,
        max_output_bytes: u64,
        now_timestamp_ms: u64,
    ) -> Result<Vec<u8>, String>;

    /// Logs: store one encoded `LogRecord`, read it by returned content id, then query the fixed time
    /// window. The record is content-addressed and timestamp-fixed, so both get and query outputs are
    /// deterministic local vs remote.
    fn logs_put_record(&self, ws: &str, record: &[u8]) -> Result<String, String>;
    fn logs_get_record(&self, ws: &str, record_id: &str) -> Result<Option<Vec<u8>>, String>;
    fn logs_query(
        &self,
        ws: &str,
        from_time_unix_nano: u64,
        to_time_unix_nano: u64,
        max_records: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, String>;

    /// Traces: store one encoded span, read it by trace/span id, then query both trace-local and
    /// time-window views. The span is fixed and canonical, so all outputs are deterministic.
    fn traces_put_span(&self, ws: &str, span: &[u8]) -> Result<(), String>;
    fn traces_get_span(
        &self,
        ws: &str,
        trace_id: &str,
        span_id: &str,
    ) -> Result<Option<Vec<u8>>, String>;
    fn traces_trace_spans(
        &self,
        ws: &str,
        trace_id: &str,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, String>;
    fn traces_query(
        &self,
        ws: &str,
        from_start_time_ns: u64,
        to_start_time_ns: u64,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, String>;

    /// Search introspection. `index` seeds a fixed search document (setup); `source_digest` returns the
    /// content-addressed digest string of the indexed source; `status` returns canonical bytes of
    /// `[source_digest, DerivedArtifactStatus]` for the never-rebuilt derived artifact. Both reads are
    /// deterministic over the fixed indexed content (no run_id/timestamp for a never-rebuilt index).
    fn search_create(&self, ws: &str, name: &str, mapping: &[u8]) -> Result<(), String>;
    fn search_index(&self, ws: &str, name: &str, id: &[u8], doc: &[u8]) -> Result<(), String>;
    fn search_source_digest(&self, ws: &str, name: &str) -> Result<String, String>;
    fn search_status(&self, ws: &str, name: &str, engine_version: &str) -> Result<Vec<u8>, String>;

    fn ts_put(&self, ws: &str, collection: &str, ts: i64, value: &[u8]) -> Result<(), String>;
    fn ts_latest(&self, ws: &str, collection: &str) -> Result<Option<(i64, Vec<u8>)>, String>;

    fn vcs_commit(
        &self,
        ws: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<String, String>;

    /// The current HEAD branch name of `ws` (VersionControl `head_branch`). After a first commit this is
    /// the default branch, so it is deterministic and directly comparable local vs remote.
    fn vcs_head_branch(&self, ws: &str) -> Result<String, String>;

    /// Seed a committed SQL table through the full session lifecycle (`sql_open` -> `sql_exec`* ->
    /// `sql_commit` -> `sql_close`), then run a read-only `sql_query_result` and return its canonical result
    /// bytes. The commit uses a fixed `commit_ts_ms` so the durable snapshot is content-stable; the read is
    /// what the parity report compares. The commit author/message are fixed fixture constants inside each
    /// driver (they do not affect the compared read output). The suite drives only the minimum mutation
    /// lifecycle needed to seed a committed table for the read.
    fn sql_seed_and_query(
        &self,
        ws: &str,
        db: &str,
        setup: &[&str],
        commit_ts_ms: u64,
        query: &str,
    ) -> Result<Vec<u8>, String>;
}

/// Fixed fixture identity for the parity SQL commit (see [`ParityDriver::sql_seed_and_query`]). Shared so a
/// local and a remote driver commit identically (the compared read output does not depend on these, but
/// keeping them equal keeps the seeded VCS snapshot identical too).
pub const SQL_COMMIT_AUTHOR: &str = "parity-sql-author";
pub const SQL_COMMIT_MESSAGE: &str = "parity sql seed";

/// The canonical KV key envelope for a text key - KV keys are typed `Value`s in a CBOR envelope, not raw
/// bytes or a bare scalar, so this uses the engine's own `kv::key_to_cbor`.
fn kv_key(text: &str) -> Vec<u8> {
    loom_core::kv::key_to_cbor(&loom_core::Value::Text(text.to_string()))
}

/// Run the deterministic parity sequence against `driver`, returning the observable outputs. A local and a
/// remote driver run this identical sequence; their [`ParityReport`]s MUST be equal.
pub fn run_client_parity_suite<D: ParityDriver>(driver: &D) -> Result<ParityReport, String> {
    let mut r = ParityReport::new();

    // Each step attaches its label to any error so a failure names the family precisely.
    let ctx = |label: &str, e: String| format!("{label}: {e}");

    // Store version (a build property; must match).
    r.text(
        "store_version",
        &driver
            .store_version()
            .map_err(|e| ctx("store_version", e))?,
    );

    // KV: put then get present + absent. KV keys are typed (canonical-CBOR encoded); values are raw bytes.
    let k1 = kv_key("k1");
    let missing = kv_key("missing");
    driver
        .kv_put("parity_kv", "c", &k1, b"v1")
        .map_err(|e| ctx("kv_put", e))?;
    r.opt_bytes(
        "kv_get.present",
        driver
            .kv_get("parity_kv", "c", &k1)
            .map_err(|e| ctx("kv_get", e))?,
    );
    r.opt_bytes(
        "kv_get.absent",
        driver
            .kv_get("parity_kv", "c", &missing)
            .map_err(|e| ctx("kv_get.absent", e))?,
    );

    // CAS: put returns a content address; get by that address round-trips.
    let digest = driver
        .cas_put("parity_cas", b"cas parity payload")
        .map_err(|e| ctx("cas_put", e))?;
    r.text("cas_put.digest", &digest);
    r.opt_bytes(
        "cas_get.present",
        driver
            .cas_get("parity_cas", &digest)
            .map_err(|e| ctx("cas_get", e))?,
    );

    // Queue: append returns a sequence; get by that sequence round-trips.
    let seq = driver
        .queue_append("parity_queue", "s", b"queue-entry-1")
        .map_err(|e| ctx("queue_append", e))?;
    r.u64("queue_append.seq", seq);
    r.opt_bytes(
        "queue_get.present",
        driver
            .queue_get("parity_queue", "s", seq)
            .map_err(|e| ctx("queue_get", e))?,
    );

    // Document read: put then get, then a `query_json` over the collection. `Document::query_json` is a
    // single unary call at the client level; the query returns canonical JSON - matching ids, per-item
    // digests under the store algorithm, and (here) the documents themselves. It is deterministic given the
    // fixed collection + fixed doc bytes, so it is byte-identical local vs remote.
    driver
        .document_put_binary_bytes("parity_doc", "notes", "d1", br#"{"x":1}"#)
        .map_err(|e| ctx("document_put_binary", e))?;
    driver
        .document_put_binary_bytes("parity_doc", "notes", "d2", br#"{"x":2}"#)
        .map_err(|e| ctx("document_put_binary", e))?;
    r.opt_bytes(
        "document_get_binary.d1",
        driver
            .document_get_binary_bytes("parity_doc", "notes", "d1")
            .map_err(|e| ctx("document_get_binary", e))?,
    );
    r.bytes(
        "document_query.notes",
        driver
            .document_query_json("parity_doc", "notes", br#"{"include_document":true}"#)
            .map_err(|e| ctx("document_query", e))?,
    );

    // Document text/binary: put then get (canonical `[value, digest]` CBOR), and the encoded collection.
    // Content-addressed digests + canonical CBOR are deterministic, so identical local vs remote.
    r.text(
        "document_put_text.digest",
        &driver
            .document_put_text("parity_doc", "text", "t1", "doc text parity")
            .map_err(|e| ctx("document_put_text", e))?,
    );
    r.opt_bytes(
        "document_get_text.t1",
        driver
            .document_get_text("parity_doc", "text", "t1")
            .map_err(|e| ctx("document_get_text", e))?,
    );
    r.text(
        "document_put_binary.digest",
        &driver
            .document_put_binary("parity_doc", "bin", "b1", b"\x00\x01\x02binary-parity")
            .map_err(|e| ctx("document_put_binary", e))?,
    );
    r.opt_bytes(
        "document_get_binary.b1",
        driver
            .document_get_binary("parity_doc", "bin", "b1")
            .map_err(|e| ctx("document_get_binary", e))?,
    );
    r.bytes(
        "document_list_binary.bin",
        driver
            .document_list_binary("parity_doc", "bin")
            .map_err(|e| ctx("document_list_binary", e))?,
    );

    // PIM: create a calendar collection + address book, then import a fixed iCalendar / vCard. The returned
    // etag is content-addressed over the parsed entry/card, so it is deterministic and identical local vs
    // remote. The engine requires the collection/book to exist before the import.
    let cal_meta = loom_core::calendar::CollectionMeta {
        display_name: "Parity".to_string(),
        component_set: Vec::new(),
    }
    .encode();
    driver
        .calendar_create_collection("parity_cal", "alice", "work", &cal_meta)
        .map_err(|e| ctx("calendar_create_collection", e))?;
    r.text(
        "calendar_put_ics.etag",
        &driver
            .calendar_put_ics(
                "parity_cal",
                "alice",
                "work",
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:parity-evt\r\nSUMMARY:Parity\r\nDTSTART:20240115T100000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
            )
            .map_err(|e| ctx("calendar_put_ics", e))?,
    );
    let book_meta = loom_core::contacts::BookMeta {
        display_name: "Parity".to_string(),
    }
    .encode();
    driver
        .contacts_create_book("parity_con", "alice", "personal", &book_meta)
        .map_err(|e| ctx("contacts_create_book", e))?;
    r.text(
        "contacts_put_vcard.etag",
        &driver
            .contacts_put_vcard(
                "parity_con",
                "alice",
                "personal",
                "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:parity-card\r\nFN:Parity Person\r\nEMAIL:p@x.io\r\nEND:VCARD\r\n",
            )
            .map_err(|e| ctx("contacts_put_vcard", e))?,
    );

    // Metrics: register a descriptor, read it back, append one fixed observation, then query the window.
    // Fixed descriptor + observation (timestamp 1, value 1.0) + fixed query bounds -> deterministic, so
    // identical local vs remote.
    let descriptor = loom_core::MetricDescriptor::new(
        "requests".into(),
        String::new(),
        "1".into(),
        loom_core::MetricInstrumentKind::Counter,
        loom_core::MetricTemporality::Cumulative,
        vec!["method".into()],
        64,
        30_000,
    )
    .map_err(|e| ctx("metric_descriptor", e.to_string()))?;
    let descriptor_bytes = descriptor
        .encode()
        .map_err(|e| ctx("metric_descriptor_encode", e.to_string()))?;
    driver
        .metrics_put_descriptor("parity_metrics", &descriptor_bytes)
        .map_err(|e| ctx("metrics_put_descriptor", e))?;
    r.opt_bytes(
        "metrics_get_descriptor.requests",
        driver
            .metrics_get_descriptor("parity_metrics", "requests")
            .map_err(|e| ctx("metrics_get_descriptor", e))?,
    );
    let observation = loom_core::MetricObservation::new(
        descriptor
            .digest()
            .map_err(|e| ctx("metric_digest", e.to_string()))?,
        std::collections::BTreeMap::from([("method".to_string(), "GET".to_string())]),
        1,
        1.0,
    )
    .map_err(|e| ctx("metric_observation", e.to_string()))?;
    driver
        .metrics_put_observation(
            "parity_metrics",
            "requests",
            &observation
                .encode()
                .map_err(|e| ctx("metric_observation_encode", e.to_string()))?,
        )
        .map_err(|e| ctx("metrics_put_observation", e))?;
    r.bytes(
        "metrics_query.requests",
        driver
            .metrics_query("parity_metrics", "requests", 0, 10, 16, 16, 64, 65536, 100)
            .map_err(|e| ctx("metrics_query", e))?,
    );

    // Logs: fixed record content + time bounds produce stable record id, get result, and query bytes.
    let log_record = loom_core::LogRecord::new(
        10,
        Some(20),
        loom_core::LogSeverityNumber::new(13).map_err(|e| ctx("log_severity", e.to_string()))?,
        "WARN".into(),
        loom_core::LogValue::String("parity log".into()),
    )
    .map_err(|e| ctx("log_record", e.to_string()))?
    .with_context(
        std::collections::BTreeMap::from([(
            "component".to_string(),
            loom_core::LogValue::String("parity".to_string()),
        )]),
        std::collections::BTreeMap::from([(
            "service.name".to_string(),
            loom_core::LogValue::String("loom".to_string()),
        )]),
        std::collections::BTreeMap::new(),
        None,
    )
    .map_err(|e| ctx("log_record_context", e.to_string()))?;
    let log_record_bytes = log_record
        .encode()
        .map_err(|e| ctx("log_record_encode", e.to_string()))?;
    let log_record_id = driver
        .logs_put_record("parity_logs", &log_record_bytes)
        .map_err(|e| ctx("logs_put_record", e))?;
    r.text("logs_put_record.id", &log_record_id);
    r.opt_bytes(
        "logs_get_record.record",
        driver
            .logs_get_record("parity_logs", &log_record_id)
            .map_err(|e| ctx("logs_get_record", e))?,
    );
    r.bytes(
        "logs_query.window",
        driver
            .logs_query("parity_logs", 0, 100, 16, 65536)
            .map_err(|e| ctx("logs_query", e))?,
    );

    // Traces: fixed span content + query bounds produce stable get, trace-spans, and query bytes.
    let span = loom_core::SpanRecord::new(
        loom_core::SpanContext::new([1; 16], [2; 8], 1)
            .map_err(|e| ctx("span_context", e.to_string()))?,
        "GET /parity".into(),
        loom_core::SpanKind::Server,
        30,
        40,
    )
    .map_err(|e| ctx("span_record", e.to_string()))?
    .with_details(loom_core::SpanDetails {
        observed_time_ns: Some(50),
        status_code: loom_core::SpanStatusCode::Ok,
        attributes: std::collections::BTreeMap::from([(
            "http.method".to_string(),
            loom_core::TraceValue::String("GET".to_string()),
        )]),
        resource: std::collections::BTreeMap::from([(
            "service.name".to_string(),
            loom_core::TraceValue::String("loom".to_string()),
        )]),
        ..loom_core::SpanDetails::default()
    })
    .map_err(|e| ctx("span_record_details", e.to_string()))?;
    let span_bytes = span
        .encode()
        .map_err(|e| ctx("span_record_encode", e.to_string()))?;
    let trace_id = span.trace_id_hex();
    let span_id = span.span_id_hex();
    driver
        .traces_put_span("parity_traces", &span_bytes)
        .map_err(|e| ctx("traces_put_span", e))?;
    r.opt_bytes(
        "traces_get_span.span",
        driver
            .traces_get_span("parity_traces", &trace_id, &span_id)
            .map_err(|e| ctx("traces_get_span", e))?,
    );
    r.bytes(
        "traces_trace_spans.trace",
        driver
            .traces_trace_spans("parity_traces", &trace_id, 16, 65536)
            .map_err(|e| ctx("traces_trace_spans", e))?,
    );
    r.bytes(
        "traces_query.window",
        driver
            .traces_query("parity_traces", 0, 100, 16, 65536)
            .map_err(|e| ctx("traces_query", e))?,
    );

    // Search introspection: index a fixed document, then read the source digest and the derived-artifact
    // status. The digest is content-addressed over the fixed indexed source, and the never-rebuilt status
    // has no run_id/timestamp, so both are deterministic and identical local vs remote.
    let mut search_doc: std::collections::BTreeMap<String, loom_core::FieldValue> =
        std::collections::BTreeMap::new();
    search_doc.insert(
        "body".to_string(),
        loom_core::FieldValue::Text("parity search body".to_string()),
    );
    let search_doc_bytes = loom_core::search::search_document_cbor(&search_doc);
    let mut search_mapping = loom_core::Mapping::new();
    search_mapping.insert("body".to_string(), loom_core::FieldMapping::text());
    let search_mapping_bytes = loom_core::search::search_mapping_cbor(&search_mapping);
    driver
        .search_create("parity_search", "idx", &search_mapping_bytes)
        .map_err(|e| ctx("search_create", e))?;
    driver
        .search_index("parity_search", "idx", b"doc-1", &search_doc_bytes)
        .map_err(|e| ctx("search_index", e))?;
    r.text(
        "search_source_digest.idx",
        &driver
            .search_source_digest("parity_search", "idx")
            .map_err(|e| ctx("search_source_digest", e))?,
    );
    r.bytes(
        "search_status.idx",
        driver
            .search_status("parity_search", "idx", "tantivy-parity")
            .map_err(|e| ctx("search_status", e))?,
    );

    // TimeSeries: two puts, then latest carries the newest timestamped point.
    driver
        .ts_put("parity_ts", "series", 100, b"t100")
        .map_err(|e| ctx("ts_put", e))?;
    driver
        .ts_put("parity_ts", "series", 200, b"t200")
        .map_err(|e| ctx("ts_put", e))?;
    r.opt_point(
        "ts_latest",
        driver
            .ts_latest("parity_ts", "series")
            .map_err(|e| ctx("ts_latest", e))?,
    );

    // VCS: a digest-sensitive timestamped commit. Both drivers start from an empty store and commit the
    // same content (a seeded document) with the same author/message and a FIXED timestamp, so the
    // content-addressed digest must be identical local vs remote.
    driver
        .document_put_binary_bytes("parity_vcs", "notes", "d1", br#"{"committed":true}"#)
        .map_err(|e| ctx("document_put_binary.vcs", e))?;
    r.text(
        "vcs_commit.digest",
        &driver
            .vcs_commit("parity_vcs", "parity-author", "parity commit", 5000)
            .map_err(|e| ctx("vcs_commit", e))?,
    );

    // VCS head branch: after the commit above, `parity_vcs` has a HEAD on the default branch. Deterministic
    // (fixed workspace, first commit), so identical local vs remote.
    r.text(
        "vcs_head_branch",
        &driver
            .vcs_head_branch("parity_vcs")
            .map_err(|e| ctx("vcs_head_branch", e))?,
    );

    // SQL read: seed a committed table through the session lifecycle (open -> exec CREATE + INSERT -> commit
    // -> close), then read it back with the read-only unary `sql_query_result`. `sql_exec` persists each
    // statement and the fixed-timestamp `sql_commit` snapshots the working tree, so the `SELECT ... ORDER
    // BY` result is deterministic and byte-identical local vs remote. Only the read result is recorded.
    r.bytes(
        "sql_query.orders",
        driver
            .sql_seed_and_query(
                "parity_sql",
                "app",
                &[
                    "CREATE TABLE orders (id INTEGER PRIMARY KEY, item TEXT)",
                    "INSERT INTO orders (id, item) VALUES (1, 'alpha'), (2, 'beta')",
                ],
                6000,
                "SELECT id, item FROM orders ORDER BY id",
            )
            .map_err(|e| ctx("sql_query", e))?,
    );

    Ok(r)
}

/// In-process [`ParityDriver`] over a [`loom_client::LocalLoomClient`].
pub struct LocalClientDriver {
    client: loom_client::LocalLoomClient,
    session: loom_client::types::LoomSession,
}

impl LocalClientDriver {
    /// Create a fresh local store at `path` and open a session against it.
    pub fn create(path: impl Into<std::path::PathBuf>) -> Result<Self, String> {
        let client = loom_client::LocalLoomClient::new(path);
        client.create().map_err(|e| e.to_string())?;
        let session = client.open().map_err(|e| e.to_string())?;
        Ok(Self { client, session })
    }
}

impl ParityDriver for LocalClientDriver {
    fn store_version(&self) -> Result<String, String> {
        Ok(self.client.store_version())
    }

    fn kv_put(&self, ws: &str, collection: &str, key: &[u8], value: &[u8]) -> Result<(), String> {
        self.client
            .kv_put(&self.session, ws, collection, key, value)
            .map_err(|e| e.to_string())
    }

    fn kv_get(&self, ws: &str, collection: &str, key: &[u8]) -> Result<Option<Vec<u8>>, String> {
        self.client
            .kv_get(&self.session, ws, collection, key)
            .map_err(|e| e.to_string())
    }

    fn cas_put(&self, ws: &str, content: &[u8]) -> Result<String, String> {
        self.client
            .cas_put(&self.session, ws, content)
            .map(|d| d.to_string())
            .map_err(|e| e.to_string())
    }

    fn cas_get(&self, ws: &str, digest: &str) -> Result<Option<Vec<u8>>, String> {
        let digest = loom_core::Digest::parse(digest).map_err(|e| e.to_string())?;
        self.client
            .cas_get(&self.session, ws, &digest)
            .map_err(|e| e.to_string())
    }

    fn queue_append(&self, ws: &str, stream: &str, entry: &[u8]) -> Result<u64, String> {
        self.client
            .queue_append(&self.session, ws, stream, entry)
            .map_err(|e| e.to_string())
    }

    fn queue_get(&self, ws: &str, stream: &str, seq: u64) -> Result<Option<Vec<u8>>, String> {
        self.client
            .queue_get(&self.session, ws, stream, seq)
            .map_err(|e| e.to_string())
    }

    fn document_put_binary_bytes(
        &self,
        ws: &str,
        collection: &str,
        id: &str,
        doc: &[u8],
    ) -> Result<(), String> {
        self.client
            .document_put_binary(&self.session, ws, collection, id, doc, None)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn document_get_binary_bytes(
        &self,
        ws: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        self.client
            .document_get_binary(&self.session, ws, collection, id)
            .and_then(|value| {
                value
                    .map(|bytes| loom_wire::document::binary_result_from_cbor(&bytes).map(|v| v.0))
                    .transpose()
            })
            .map_err(|e| e.to_string())
    }

    fn document_query_json(
        &self,
        ws: &str,
        collection: &str,
        query_json: &[u8],
    ) -> Result<Vec<u8>, String> {
        self.client
            .document_query_json(&self.session, ws, collection, query_json)
            .map_err(|e| e.to_string())
    }

    fn document_put_text(
        &self,
        ws: &str,
        collection: &str,
        id: &str,
        text: &str,
    ) -> Result<String, String> {
        self.client
            .document_put_text(&self.session, ws, collection, id, text, None)
            .and_then(|bytes| loom_wire::document::put_result_from_cbor(&bytes).map(|v| v.0))
            .map_err(|e| e.to_string())
    }

    fn document_get_text(
        &self,
        ws: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        self.client
            .document_get_text(&self.session, ws, collection, id)
            .map_err(|e| e.to_string())
    }

    fn document_put_binary(
        &self,
        ws: &str,
        collection: &str,
        id: &str,
        bytes: &[u8],
    ) -> Result<String, String> {
        self.client
            .document_put_binary(&self.session, ws, collection, id, bytes, None)
            .and_then(|bytes| loom_wire::document::put_result_from_cbor(&bytes).map(|v| v.0))
            .map_err(|e| e.to_string())
    }

    fn document_get_binary(
        &self,
        ws: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        self.client
            .document_get_binary(&self.session, ws, collection, id)
            .map_err(|e| e.to_string())
    }

    fn document_list_binary(&self, ws: &str, collection: &str) -> Result<Vec<u8>, String> {
        self.client
            .document_list_binary(&self.session, ws, collection)
            .map_err(|e| e.to_string())
    }

    fn calendar_create_collection(
        &self,
        ws: &str,
        principal: &str,
        collection: &str,
        meta: &[u8],
    ) -> Result<(), String> {
        self.client
            .calendar_create_collection(&self.session, ws, principal, collection, meta)
            .map_err(|e| e.to_string())
    }

    fn calendar_put_ics(
        &self,
        ws: &str,
        principal: &str,
        collection: &str,
        ics: &str,
    ) -> Result<String, String> {
        self.client
            .calendar_put_ics(&self.session, ws, principal, collection, ics)
            .map(|d| d.to_string())
            .map_err(|e| e.to_string())
    }

    fn contacts_create_book(
        &self,
        ws: &str,
        principal: &str,
        book: &str,
        meta: &[u8],
    ) -> Result<(), String> {
        self.client
            .contacts_create_book(&self.session, ws, principal, book, meta)
            .map_err(|e| e.to_string())
    }

    fn contacts_put_vcard(
        &self,
        ws: &str,
        principal: &str,
        book: &str,
        vcard: &str,
    ) -> Result<String, String> {
        self.client
            .contacts_put_vcard(&self.session, ws, principal, book, vcard)
            .map(|d| d.to_string())
            .map_err(|e| e.to_string())
    }

    fn metrics_put_descriptor(&self, ws: &str, descriptor: &[u8]) -> Result<(), String> {
        self.client
            .metrics_put_descriptor(&self.session, ws, descriptor)
            .map_err(|e| e.to_string())
    }

    fn metrics_get_descriptor(&self, ws: &str, name: &str) -> Result<Option<Vec<u8>>, String> {
        self.client
            .metrics_get_descriptor(&self.session, ws, name)
            .map_err(|e| e.to_string())
    }

    fn metrics_put_observation(
        &self,
        ws: &str,
        descriptor_name: &str,
        observation: &[u8],
    ) -> Result<(), String> {
        self.client
            .metrics_put_observation(&self.session, ws, descriptor_name, observation)
            .map_err(|e| e.to_string())
    }

    fn metrics_query(
        &self,
        ws: &str,
        descriptor_name: &str,
        from_timestamp_ms: u64,
        to_timestamp_ms: u64,
        max_series: u32,
        max_groups: u32,
        max_samples: u32,
        max_output_bytes: u64,
        now_timestamp_ms: u64,
    ) -> Result<Vec<u8>, String> {
        self.client
            .metrics_query(
                &self.session,
                ws,
                descriptor_name,
                from_timestamp_ms,
                to_timestamp_ms,
                max_series,
                max_groups,
                max_samples,
                max_output_bytes,
                now_timestamp_ms,
            )
            .map_err(|e| e.to_string())
    }

    fn logs_put_record(&self, ws: &str, record: &[u8]) -> Result<String, String> {
        self.client
            .logs_put_record(&self.session, ws, record)
            .map_err(|e| e.to_string())
    }

    fn logs_get_record(&self, ws: &str, record_id: &str) -> Result<Option<Vec<u8>>, String> {
        self.client
            .logs_get_record(&self.session, ws, record_id)
            .map_err(|e| e.to_string())
    }

    fn logs_query(
        &self,
        ws: &str,
        from_time_unix_nano: u64,
        to_time_unix_nano: u64,
        max_records: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, String> {
        self.client
            .logs_query(
                &self.session,
                ws,
                from_time_unix_nano,
                to_time_unix_nano,
                max_records,
                max_output_bytes,
            )
            .map_err(|e| e.to_string())
    }

    fn traces_put_span(&self, ws: &str, span: &[u8]) -> Result<(), String> {
        self.client
            .traces_put_span(&self.session, ws, span)
            .map_err(|e| e.to_string())
    }

    fn traces_get_span(
        &self,
        ws: &str,
        trace_id: &str,
        span_id: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        self.client
            .traces_get_span(&self.session, ws, trace_id, span_id)
            .map_err(|e| e.to_string())
    }

    fn traces_trace_spans(
        &self,
        ws: &str,
        trace_id: &str,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, String> {
        self.client
            .traces_trace_spans(&self.session, ws, trace_id, max_spans, max_output_bytes)
            .map_err(|e| e.to_string())
    }

    fn traces_query(
        &self,
        ws: &str,
        from_start_time_ns: u64,
        to_start_time_ns: u64,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>, String> {
        self.client
            .traces_query(
                &self.session,
                ws,
                from_start_time_ns,
                to_start_time_ns,
                max_spans,
                max_output_bytes,
            )
            .map_err(|e| e.to_string())
    }

    fn search_create(&self, ws: &str, name: &str, mapping: &[u8]) -> Result<(), String> {
        self.client
            .search_create(&self.session, ws, name, mapping)
            .map_err(|e| e.to_string())
    }

    fn search_index(&self, ws: &str, name: &str, id: &[u8], doc: &[u8]) -> Result<(), String> {
        self.client
            .search_index(&self.session, ws, name, id, doc)
            .map_err(|e| e.to_string())
    }

    fn search_source_digest(&self, ws: &str, name: &str) -> Result<String, String> {
        self.client
            .search_source_digest(&self.session, ws, name)
            .map(|d| d.to_string())
            .map_err(|e| e.to_string())
    }

    fn search_status(&self, ws: &str, name: &str, engine_version: &str) -> Result<Vec<u8>, String> {
        self.client
            .search_status(&self.session, ws, name, engine_version)
            .map_err(|e| e.to_string())
    }

    fn ts_put(&self, ws: &str, collection: &str, ts: i64, value: &[u8]) -> Result<(), String> {
        self.client
            .ts_put(&self.session, ws, collection, ts, value)
            .map_err(|e| e.to_string())
    }

    fn ts_latest(&self, ws: &str, collection: &str) -> Result<Option<(i64, Vec<u8>)>, String> {
        self.client
            .ts_latest(&self.session, ws, collection)
            .map_err(|e| e.to_string())
    }

    fn vcs_commit(
        &self,
        ws: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<String, String> {
        self.client
            .commit(&self.session, ws, author, message, timestamp_ms)
            .map(|d| d.to_string())
            .map_err(|e| e.to_string())
    }

    fn vcs_head_branch(&self, ws: &str) -> Result<String, String> {
        self.client
            .vcs_head_branch(&self.session, ws)
            .map_err(|e| e.to_string())
    }

    fn sql_seed_and_query(
        &self,
        ws: &str,
        db: &str,
        setup: &[&str],
        commit_ts_ms: u64,
        query: &str,
    ) -> Result<Vec<u8>, String> {
        // The in-process `LocalLoomClient` SQL methods each open the store file independently, and `open()`
        // (this driver's held session) keeps an exclusive writer lock on that same file, so `sql_open` would
        // collide with "loom is open for writing"; the remote server, which does not hold an exclusive
        // writer lock per session, has no such collision. SQL is the terminal step of the parity suite, so
        // the local driver releases its held session first, runs the full SQL lifecycle, then reads through
        // a fresh short-lived session. This must remain the last op the local driver performs.
        self.client.close(&self.session);

        let sql_session = self.client.sql_open(ws, db).map_err(|e| e.to_string())?;
        for stmt in setup {
            self.client
                .sql_exec(&sql_session, stmt)
                .map_err(|e| e.to_string())?;
        }
        self.client
            .sql_commit(
                &sql_session,
                SQL_COMMIT_MESSAGE,
                SQL_COMMIT_AUTHOR,
                commit_ts_ms,
            )
            .map_err(|e| e.to_string())?;
        self.client.sql_close(&sql_session);

        // Read the committed table through a fresh store session (the read uses the store session, not
        // the SQL session).
        let read_session = self.client.open().map_err(|e| e.to_string())?;
        let result = self
            .client
            .sql_query_result(&read_session, ws, db, query)
            .map_err(|e| e.to_string());
        self.client.close(&read_session);
        result
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use super::*;

    fn temp_store_path(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        std::env::temp_dir().join(format!("loom-parity-{tag}-{pid}-{n}.loom"))
    }

    #[test]
    fn local_driver_runs_the_full_parity_suite() {
        let path = temp_store_path("local");
        let driver = LocalClientDriver::create(&path).expect("create local store");
        let report = run_client_parity_suite(&driver).expect("suite runs against the local driver");

        // Every family produced an observable output (a non-toy report), in order.
        let labels: Vec<&str> = report.entries.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "store_version",
                "kv_get.present",
                "kv_get.absent",
                "cas_put.digest",
                "cas_get.present",
                "queue_append.seq",
                "queue_get.present",
                "document_get_binary.d1",
                "document_query.notes",
                "document_put_text.digest",
                "document_get_text.t1",
                "document_put_binary.digest",
                "document_get_binary.b1",
                "document_list_binary.bin",
                "calendar_put_ics.etag",
                "contacts_put_vcard.etag",
                "metrics_get_descriptor.requests",
                "metrics_query.requests",
                "logs_put_record.id",
                "logs_get_record.record",
                "logs_query.window",
                "traces_get_span.span",
                "traces_trace_spans.trace",
                "traces_query.window",
                "search_source_digest.idx",
                "search_status.idx",
                "ts_latest",
                "vcs_commit.digest",
                "vcs_head_branch",
                "sql_query.orders",
            ]
        );

        let get = |label: &str| -> &[u8] {
            &report
                .entries
                .iter()
                .find(|(l, _)| l == label)
                .expect("label present")
                .1
        };

        // Concrete observable values (not just success): KV round-trips, absent is None-tagged, CAS digest
        // is algo:hex and round-trips, queue seq is 1, ts_latest carries ts=200/value, commit is algo:hex.
        assert_eq!(get("kv_get.present"), b"\x01v1");
        assert_eq!(get("kv_get.absent"), b"\x00");
        assert!(
            std::str::from_utf8(get("cas_put.digest"))
                .unwrap()
                .contains(':')
        );
        assert_eq!(get("cas_get.present"), b"\x01cas parity payload");
        assert_eq!(get("queue_append.seq"), 0u64.to_le_bytes());
        assert_eq!(get("queue_get.present"), b"\x01queue-entry-1");
        // ts_latest: tag 1, ts=200 little-endian, then value "t200".
        let mut expected_ts = vec![1u8];
        expected_ts.extend_from_slice(&200i64.to_le_bytes());
        expected_ts.extend_from_slice(b"t200");
        assert_eq!(get("ts_latest"), expected_ts.as_slice());
        assert!(
            std::str::from_utf8(get("vcs_commit.digest"))
                .unwrap()
                .contains(':')
        );
        // document_query returns canonical JSON naming both seeded ids (d1, d2).
        let dq = std::str::from_utf8(get("document_query.notes")).unwrap();
        assert!(
            dq.contains("d1") && dq.contains("d2"),
            "query names both docs: {dq}"
        );
        // sql_query returns a non-empty canonical result over the committed table (two seeded rows).
        assert!(
            !get("sql_query.orders").is_empty(),
            "sql query returned a result"
        );

        // Running the suite again on a second fresh store yields an identical report - the determinism the
        // local vs remote comparison relies on (fixed names + fixed commit timestamp).
        let path2 = temp_store_path("local2");
        let driver2 = LocalClientDriver::create(&path2).expect("create second local store");
        let report2 = run_client_parity_suite(&driver2).expect("suite runs again");
        assert_eq!(
            report, report2,
            "the suite is deterministic across fresh stores"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&path2);
    }
}
