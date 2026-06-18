//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[wasm_bindgen]
impl LoomSql {
    /// Append `payload` to ledger `collection` of `workspace` (created with the `ledger` facet if absent);
    /// returns the entry sequence.
    pub fn ledger_append(
        &mut self,
        workspace: String,
        collection: String,
        payload: Vec<u8>,
    ) -> Result<u64, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_ledger_ns(&mut self.loom, &workspace)?;
        let seq = ledger_append(&mut self.loom, ns, &collection, payload).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(seq)
    }

    /// The payload at `seq` in ledger `collection`, or null if absent.
    pub fn ledger_get(
        &self,
        workspace: String,
        collection: String,
        seq: u64,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(ledger_get(&self.loom, ns, &collection, seq)
            .map_err(le)?
            .map(|b| Uint8Array::from(b.as_slice())))
    }

    /// The 32-byte head chain hash of ledger `collection`, or null when absent or empty.
    pub fn ledger_head(
        &self,
        workspace: String,
        collection: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(ledger_head(&self.loom, ns, &collection)
            .map_err(le)?
            .map(|d| Uint8Array::from(d.bytes().as_slice())))
    }

    /// The number of entries in ledger `collection` (0 when absent).
    pub fn ledger_len(&self, workspace: String, collection: String) -> Result<u64, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        ledger_len(&self.loom, ns, &collection).map_err(le)
    }

    /// Recompute and verify ledger `collection`'s hash chain; an altered payload or broken link errors.
    pub fn ledger_verify(&self, workspace: String, collection: String) -> Result<(), JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        ledger_verify(&self.loom, ns, &collection).map_err(le)
    }
}
