//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[wasm_bindgen]
impl LoomSql {
    /// Append `entry` to `stream` in `workspace` (UUID or name, created with the queue facet if
    /// absent); returns the assigned zero-based sequence.
    pub fn queue_append(
        &mut self,
        workspace: String,
        stream: String,
        entry: Vec<u8>,
    ) -> Result<BigInt, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        validate_stream_name(&stream)?;
        let ns = ensure_queue_ns(&mut self.loom, &workspace)?;
        let seq = self.loom.stream_append(ns, &stream, &entry).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(BigInt::from(seq as u64))
    }

    /// Fetch the entry at `seq` in `stream`, or null if out of range.
    pub fn queue_get(
        &self,
        workspace: String,
        stream: String,
        seq: u64,
    ) -> Result<Option<Uint8Array>, JsError> {
        validate_stream_name(&stream)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let seq = usize::try_from(seq).map_err(|_| JsError::new("seq out of range"))?;
        let found = self.loom.stream_get(ns, &stream, seq).map_err(le)?;
        Ok(found.map(|b| Uint8Array::from(b.as_slice())))
    }

    /// The half-open range `[lo, hi)` of `stream` as a JS array of `Uint8Array`, oldest first.
    pub fn queue_range(
        &self,
        workspace: String,
        stream: String,
        lo: u64,
        hi: u64,
    ) -> Result<JsValue, JsError> {
        validate_stream_name(&stream)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let lo = usize::try_from(lo).map_err(|_| JsError::new("lo out of range"))?;
        let hi = usize::try_from(hi).map_err(|_| JsError::new("hi out of range"))?;
        let entries = self.loom.stream_range(ns, &stream, lo, hi).map_err(le)?;
        let arr = Array::new();
        for e in &entries {
            arr.push(&Uint8Array::from(e.as_slice()));
        }
        Ok(arr.into())
    }

    /// The number of entries in `stream`.
    pub fn queue_len(&self, workspace: String, stream: String) -> Result<BigInt, JsError> {
        validate_stream_name(&stream)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(BigInt::from(
            self.loom.stream_len(ns, &stream).map_err(le)? as u64
        ))
    }

    /// The named consumer's next sequence for `stream`; 0 when none is stored.
    pub fn queue_consumer_position(
        &self,
        workspace: String,
        stream: String,
        consumer_id: String,
    ) -> Result<BigInt, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(BigInt::from(
            self.loom
                .consumer_position(ns, &stream, &consumer_id)
                .map_err(le)?,
        ))
    }

    /// Up to `max` entries from the consumer's stored next sequence as a JS array of `Uint8Array`;
    /// does not advance the consumer.
    pub fn queue_consumer_read(
        &self,
        workspace: String,
        stream: String,
        consumer_id: String,
        max: u32,
    ) -> Result<JsValue, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let entries = self
            .loom
            .consumer_read(ns, &stream, &consumer_id, max as usize)
            .map_err(le)?;
        let arr = Array::new();
        for e in &entries {
            arr.push(&Uint8Array::from(e.as_slice()));
        }
        Ok(arr.into())
    }

    /// Advance the named consumer's next sequence for `stream` to `next_seq` (monotonic).
    pub fn queue_consumer_advance(
        &mut self,
        workspace: String,
        stream: String,
        consumer_id: String,
        next_seq: u64,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        self.loom
            .consumer_advance(ns, &stream, &consumer_id, next_seq)
            .map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Set the named consumer's next sequence for `stream` to `next_seq` (may move backward).
    pub fn queue_consumer_reset(
        &mut self,
        workspace: String,
        stream: String,
        consumer_id: String,
        next_seq: u64,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        self.loom
            .consumer_reset(ns, &stream, &consumer_id, next_seq)
            .map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }
}
