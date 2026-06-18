//! The local (`Lo`) methods of the generated `RemoteLoomClient` surface.
//!
//! `Diagnostics` (JSON renders + `last_error`), `Store.blob_digest`, `Tasks.iter_free`/`task_free`, and
//! the whole `ResultViews` family never round trip: they run on a buffer the client already holds or on
//! purely local bookkeeping. The generated per-interface impls call the `lo_*` helpers here, which decode
//! through the shared engine-free decoder (`loom-result`) so the remote client and the in-process client
//! present identical local semantics. A decoded [`ResultView`] is kept in a client-local registry keyed
//! by a minted handle id; `result_close` drops it (freeing a handle is local).
//!
//! Licensed under BUSL-1.1.

use crate::client::RemoteLoomClient;
use crate::transport::Transport;
use crate::wire::blob_digest;
use loom_codec::Value;
use loom_remote_protocol::api_types::{Digest, HandleId, ResultView, RowIter, Task};
use loom_result::result_view::{ResultPayload, decode};
use loom_result::view;
use loom_types::tabular::cell_from;
use loom_types::{Code, LoomError};
use std::sync::atomic::Ordering;

impl<T: Transport> RemoteLoomClient<T> {
    // ---- Diagnostics (transport `Lo`) -----------------------------------------------------------

    /// `Diagnostics.result_to_json`: render a result buffer to debug JSON via the shared decoder.
    pub(crate) fn lo_result_to_json(&self, result: Vec<u8>) -> Result<String, LoomError> {
        self.record(loom_result::result_to_json(&result))
    }

    /// `Diagnostics.result_to_bridge_json`: render a result buffer to bridge JSON via the shared decoder.
    pub(crate) fn lo_result_to_bridge_json(&self, result: Vec<u8>) -> Result<String, LoomError> {
        self.record(loom_result::to_bridge_json(&result))
    }

    /// `Diagnostics.last_error`: the most recent recorded decode failure as canonical CBOR
    /// `[code_i32, message_or_null, details_or_null]` (the IDL `LastError`). No error yet is
    /// `[0, null, null]`.
    pub(crate) fn lo_last_error(&self) -> Result<Vec<u8>, LoomError> {
        Ok(self.last_error.lock().expect("last error lock").clone())
    }

    /// Record a decode failure into `last_error` and pass the result through unchanged.
    fn record<V>(&self, result: Result<V, LoomError>) -> Result<V, LoomError> {
        if let Err(err) = &result {
            self.store_error(err);
        }
        result
    }

    /// Record `err` into `last_error` and return it (for direct error paths).
    fn recorded(&self, err: LoomError) -> LoomError {
        self.store_error(&err);
        err
    }

    fn store_error(&self, err: &LoomError) {
        *self.last_error.lock().expect("last error lock") =
            encode_last_error(err.code.as_i32(), Some(&err.message), err.details_cbor());
    }

    // ---- Store.blob_digest (transport `Lo`) -----------------------------------------------------

    /// `Store.blob_digest`: the content address of `data`, computed locally (matches the engine).
    pub(crate) fn lo_blob_digest(&self, data: Vec<u8>) -> Result<Digest, LoomError> {
        Ok(Digest(blob_digest(&data)))
    }

    // ---- Tasks handle release (transport `Lo`) --------------------------------------------------

    /// `Tasks.iter_free`: releasing a remote iterator handle is local; the server reclaims by generation.
    pub(crate) fn lo_iter_free(&self, _iter: RowIter) -> Result<(), LoomError> {
        Ok(())
    }

    /// `Tasks.task_free`: releasing a remote task handle is local; the server reclaims by generation.
    pub(crate) fn lo_task_free(&self, _task: Task) -> Result<(), LoomError> {
        Ok(())
    }

    // ---- ResultViews (transport `Lo`) -----------------------------------------------------------

    /// `ResultViews.result_open`: decode a full result buffer and register it, returning a view handle.
    pub(crate) fn lo_result_open(&self, result: Vec<u8>) -> Result<ResultView, LoomError> {
        let payload = self.record(decode(&result))?;
        Ok(self.register_view(payload))
    }

    /// `ResultViews.row_open`: wrap a single streamed row (a canonical CBOR cell array) as item 0, row 0.
    pub(crate) fn lo_row_open(&self, row: Vec<u8>) -> Result<ResultView, LoomError> {
        let decoded = self.record(
            loom_codec::decode(&row)
                .map_err(|err| LoomError::new(Code::CorruptObject, format!("row cbor: {err}"))),
        )?;
        let Value::Array(cells) = decoded else {
            return Err(self.recorded(LoomError::new(
                Code::CorruptObject,
                "row is not a cell array",
            )));
        };
        let cells = self.record(
            cells
                .into_iter()
                .map(cell_from)
                .collect::<Result<Vec<_>, LoomError>>(),
        )?;
        let payload = ResultPayload::Reader(loom_result::result_view::Reader::Rows {
            columns: Vec::new(),
            rows: vec![cells],
        });
        Ok(self.register_view(payload))
    }

    /// `ResultViews.result_close`: drop the registered view (a local handle free).
    pub(crate) fn lo_result_close(&self, view: ResultView) -> Result<(), LoomError> {
        self.views.lock().expect("view lock").remove(&view.0.id);
        Ok(())
    }

    fn register_view(&self, payload: ResultPayload) -> ResultView {
        let id = self
            .next_view
            .fetch_add(1, Ordering::Relaxed)
            .to_be_bytes()
            .to_vec();
        self.views
            .lock()
            .expect("view lock")
            .insert(id.clone(), payload);
        ResultView(HandleId {
            kind: "result_view".to_string(),
            id,
            generation: 1,
            owner_session: self.session_id().unwrap_or_default(),
        })
    }

    /// Run `f` against the decoded payload behind `view`, or fail `NOT_FOUND` for an unknown handle.
    fn with_view<R>(
        &self,
        view: &ResultView,
        f: impl FnOnce(&ResultPayload) -> Result<R, LoomError>,
    ) -> Result<R, LoomError> {
        let views = self.views.lock().expect("view lock");
        let payload = views
            .get(&view.0.id)
            .ok_or_else(|| LoomError::new(Code::NotFound, "unknown or closed result view"))?;
        f(payload)
    }

    pub(crate) fn lo_result_len(&self, view: ResultView) -> Result<u64, LoomError> {
        self.with_view(&view, |p| Ok(view::len(p)))
    }

    pub(crate) fn lo_result_is_statements(
        &self,
        view: ResultView,
    ) -> Result<Option<bool>, LoomError> {
        self.with_view(&view, |p| Ok(view::is_statements(p)))
    }

    pub(crate) fn lo_result_item_kind(
        &self,
        view: ResultView,
        item: u64,
    ) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_view(&view, |p| Ok(view::item_kind(p, item)))
    }

    pub(crate) fn lo_result_column_count(
        &self,
        view: ResultView,
        item: u64,
    ) -> Result<u64, LoomError> {
        self.with_view(&view, |p| view::column_count(p, item))
    }

    pub(crate) fn lo_result_column_name(
        &self,
        view: ResultView,
        item: u64,
        col: u64,
    ) -> Result<String, LoomError> {
        self.with_view(&view, |p| view::column_name(p, item, col))
    }

    pub(crate) fn lo_result_column_type(
        &self,
        view: ResultView,
        item: u64,
        col: u64,
    ) -> Result<String, LoomError> {
        self.with_view(&view, |p| view::column_type(p, item, col))
    }

    pub(crate) fn lo_result_row_count(
        &self,
        view: ResultView,
        item: u64,
    ) -> Result<u64, LoomError> {
        self.with_view(&view, |p| view::row_count(p, item))
    }

    pub(crate) fn lo_result_row_len(
        &self,
        view: ResultView,
        item: u64,
        row: u64,
    ) -> Result<u64, LoomError> {
        self.with_view(&view, |p| view::row_len(p, item, row))
    }

    pub(crate) fn lo_result_cell(
        &self,
        view: ResultView,
        item: u64,
        row: u64,
        col: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_view(&view, |p| view::cell(p, item, row, col))
    }

    pub(crate) fn lo_result_row_commit(
        &self,
        view: ResultView,
        item: u64,
        row: u64,
    ) -> Result<String, LoomError> {
        self.with_view(&view, |p| view::row_commit(p, item, row))
    }

    pub(crate) fn lo_result_count(&self, view: ResultView, item: u64) -> Result<u64, LoomError> {
        self.with_view(&view, |p| view::count(p, item))
    }

    pub(crate) fn lo_result_string_count(
        &self,
        view: ResultView,
        item: u64,
    ) -> Result<u64, LoomError> {
        self.with_view(&view, |p| view::string_count(p, item))
    }

    pub(crate) fn lo_result_string(
        &self,
        view: ResultView,
        item: u64,
        i: u64,
    ) -> Result<String, LoomError> {
        self.with_view(&view, |p| view::string(p, item, i))
    }

    pub(crate) fn lo_result_variable_kind(
        &self,
        view: ResultView,
        item: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_view(&view, |p| view::variable_kind(p, item))
    }

    pub(crate) fn lo_result_merge_outcome(
        &self,
        view: ResultView,
        item: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_view(&view, |p| view::merge_outcome(p, item))
    }

    pub(crate) fn lo_result_diff_count(
        &self,
        view: ResultView,
        item: u64,
    ) -> Result<u64, LoomError> {
        self.with_view(&view, |p| view::diff_count(p, item))
    }

    pub(crate) fn lo_result_diff_change(
        &self,
        view: ResultView,
        item: u64,
        entry: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_view(&view, |p| view::diff_change(p, item, entry))
    }

    pub(crate) fn lo_result_diff_len(
        &self,
        view: ResultView,
        item: u64,
        entry: u64,
        side: Vec<u8>,
    ) -> Result<u64, LoomError> {
        self.with_view(&view, |p| view::diff_len(p, item, entry, &side))
    }

    pub(crate) fn lo_result_diff_cell(
        &self,
        view: ResultView,
        item: u64,
        entry: u64,
        side: Vec<u8>,
        col: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_view(&view, |p| view::diff_cell(p, item, entry, &side, col))
    }

    pub(crate) fn lo_result_map_len(
        &self,
        view: ResultView,
        item: u64,
        row: u64,
    ) -> Result<u64, LoomError> {
        self.with_view(&view, |p| view::map_len(p, item, row))
    }

    pub(crate) fn lo_result_map_entry(
        &self,
        view: ResultView,
        item: u64,
        row: u64,
        idx: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_view(&view, |p| view::map_entry(p, item, row, idx))
    }
}

/// Encode a `LastError` as canonical CBOR `[code_i32, message_or_null, details_or_null]`.
fn encode_last_error(code: i32, message: Option<&str>, details: Option<Vec<u8>>) -> Vec<u8> {
    let message = match message {
        Some(text) => Value::Text(text.to_string()),
        None => Value::Null,
    };
    let details = details.map(Value::Bytes).unwrap_or(Value::Null);
    loom_codec::encode(&Value::Array(vec![
        Value::int(i64::from(code)),
        message,
        details,
    ]))
    .expect("last error always encodes to canonical CBOR")
}
