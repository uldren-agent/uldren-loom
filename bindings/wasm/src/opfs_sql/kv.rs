//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[wasm_bindgen]
impl LoomSql {
    /// Put `value` at the typed `key` (Loom Canonical CBOR cell) in map `name` of `workspace` (UUID
    /// or name, created with the `kv` facet if absent). A later put at the same key replaces it.
    pub fn kv_put(
        &mut self,
        workspace: String,
        collection: String,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_kv_ns(&mut self.loom, &workspace)?;
        let key = key_from_cbor(&key).map_err(le)?;
        kv_put(&mut self.loom, ns, &collection, key, value).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Fetch the value at typed `key` in map `collection` of `workspace`, or null if the key/map is absent.
    pub fn kv_get(
        &self,
        workspace: String,
        collection: String,
        key: Vec<u8>,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let key = key_from_cbor(&key).map_err(le)?;
        Ok(kv_get(&self.loom, ns, &collection, &key)
            .map_err(le)?
            .map(|b| Uint8Array::from(b.as_slice())))
    }

    /// Remove the typed `key` from map `collection` of `workspace`; returns whether it was present.
    pub fn kv_delete(
        &mut self,
        workspace: String,
        collection: String,
        key: Vec<u8>,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let key = key_from_cbor(&key).map_err(le)?;
        let present = kv_delete(&mut self.loom, ns, &collection, &key).map_err(le)?;
        if present {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(present)
    }

    /// List map `collection` of `workspace` as the Loom Canonical CBOR array of `[key, value]` pairs in
    /// key order (an absent map is the empty array).
    pub fn kv_list(&self, workspace: String, collection: String) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(kv_list(&self.loom, ns, &collection).map_err(le)?.encode())
    }

    /// The entries of map `collection` with `lo <= key < hi` (half-open, key order) as the Loom Canonical
    /// CBOR array of `[key, value]` pairs. `lo`/`hi` are typed-cell CBOR keys.
    pub fn kv_range(
        &self,
        workspace: String,
        collection: String,
        lo: Vec<u8>,
        hi: Vec<u8>,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let lo = key_from_cbor(&lo).map_err(le)?;
        let hi = key_from_cbor(&hi).map_err(le)?;
        Ok(kv_range(&self.loom, ns, &collection, &lo, &hi)
            .map_err(le)?
            .encode())
    }
}
