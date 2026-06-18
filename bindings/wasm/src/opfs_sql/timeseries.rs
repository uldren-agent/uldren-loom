//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[wasm_bindgen]
impl LoomSql {
    /// Put `value` at timestamp `ts` in series `collection` of `workspace` (created with the
    /// `time-series` facet if absent).
    pub fn ts_put(
        &mut self,
        workspace: String,
        collection: String,
        ts: i64,
        value: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_ts_ns(&mut self.loom, &workspace)?;
        ts_put(&mut self.loom, ns, &collection, ts, value).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// The point at timestamp `ts` in series `collection`, or null if absent.
    pub fn ts_get(
        &self,
        workspace: String,
        collection: String,
        ts: i64,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(ts_get(&self.loom, ns, &collection, ts)
            .map_err(le)?
            .map(|b| Uint8Array::from(b.as_slice())))
    }

    /// The points of series `collection` with `from <= ts < to` (half-open) as the Loom Canonical CBOR
    /// array of `[ts, value]` pairs.
    pub fn ts_range(
        &self,
        workspace: String,
        collection: String,
        from: i64,
        to: i64,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(ts_range(&self.loom, ns, &collection, from, to)
            .map_err(le)?
            .encode())
    }

    /// The most recent point of series `collection` as a one-point CBOR array, or null if absent/empty.
    pub fn ts_latest(
        &self,
        workspace: String,
        collection: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(ts_latest(&self.loom, ns, &collection)
            .map_err(le)?
            .map(|(ts, v)| {
                let mut s = Series::new();
                s.put(ts, v);
                Uint8Array::from(s.encode().as_slice())
            }))
    }
}
