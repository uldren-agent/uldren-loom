//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[wasm_bindgen]
impl LoomSql {
    /// Put `content` into workspace `workspace`'s CAS facet (UUID or name, created with the `cas`
    /// facet if absent); returns the content address (`"algo:hex"`). Idempotent.
    pub fn cas_put(&mut self, workspace: String, content: Vec<u8>) -> Result<String, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_cas_ns(&mut self.loom, &workspace)?;
        let digest = cas_put(&mut self.loom, ns, &content).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(digest.to_string())
    }

    /// Fetch the CAS blob addressed by `digest` from `workspace`, or null if absent.
    pub fn cas_get(
        &self,
        workspace: String,
        digest: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let digest = Digest::parse(&digest).map_err(le)?;
        let found = cas_get(&self.loom, ns, &digest).map_err(le)?;
        Ok(found.map(|b| Uint8Array::from(b.as_slice())))
    }

    /// Whether a CAS blob addressed by `digest` is present in `workspace`.
    pub fn cas_has(&self, workspace: String, digest: String) -> Result<bool, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let digest = Digest::parse(&digest).map_err(le)?;
        cas_has(&self.loom, ns, &digest).map_err(le)
    }

    /// Drop the blob addressed by `digest` from `workspace`'s working tree (unreachable going
    /// forward); returns whether it was present. CAS stays immutable: bytes are GC-reclaimed once
    /// unreferenced, and an earlier commit that held the blob still restores it.
    pub fn cas_delete(&mut self, workspace: String, digest: String) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let digest = Digest::parse(&digest).map_err(le)?;
        let present = cas_delete(&mut self.loom, ns, &digest).map_err(le)?;
        if present {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(present)
    }

    /// The content addresses reachable in `workspace`'s CAS facet, as a JS array of strings, sorted.
    pub fn cas_list(&self, workspace: String) -> Result<JsValue, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let arr = Array::new();
        for d in cas_list(&self.loom, ns).map_err(le)? {
            arr.push(&JsValue::from_str(&d.to_string()));
        }
        Ok(arr.into())
    }
}
