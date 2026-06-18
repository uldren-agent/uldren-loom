//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::tabular::cell_value;
use loom_core::{DataframeBatch, DataframePlan};

fn ensure_dataframe_ns(
    loom: &mut Loom<FileStore>,
    workspace: &str,
) -> Result<WorkspaceId, JsError> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Dataframe,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Dataframe)
        .map_err(le)?;
    Ok(ns)
}

fn dataframe_batch_cbor(batch: DataframeBatch) -> Result<Vec<u8>, JsError> {
    let columns = batch
        .columns
        .into_iter()
        .map(|column| {
            CborValue::Array(vec![
                CborValue::Text(column.name),
                CborValue::Uint(u64::from(column.column_type.tag())),
                CborValue::Bool(column.nullable),
            ])
        })
        .collect::<Vec<_>>();
    let rows = batch
        .rows
        .into_iter()
        .map(|row| CborValue::Array(row.iter().map(cell_value).collect()))
        .collect::<Vec<_>>();
    cbor_encode(&CborValue::Array(vec![
        CborValue::Array(columns),
        CborValue::Array(rows),
    ]))
    .map_err(|e| JsError::new(&format!("cbor: {e}")))
}

fn digest_list_cbor(digests: Vec<Digest>) -> Result<Vec<u8>, JsError> {
    let values = digests
        .into_iter()
        .map(|digest| CborValue::Text(digest.to_string()))
        .collect::<Vec<_>>();
    cbor_encode(&CborValue::Array(values)).map_err(|e| JsError::new(&format!("cbor: {e}")))
}

#[wasm_bindgen]
impl LoomSql {
    /// Create dataframe frame `name` from canonical DataframePlan CBOR.
    pub fn dataframe_create(
        &mut self,
        workspace: String,
        name: String,
        plan: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let plan = DataframePlan::decode(&plan).map_err(le)?;
        let ns = ensure_dataframe_ns(&mut self.loom, &workspace)?;
        loom_core::dataframe_create(&mut self.loom, ns, &name, &plan).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Execute dataframe frame `name` and return canonical CBOR `[columns, rows]`.
    pub fn dataframe_collect(&self, workspace: String, name: String) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        dataframe_batch_cbor(loom_core::dataframe_collect(&self.loom, ns, &name).map_err(le)?)
    }

    /// Execute dataframe frame `name` and return at most `rows` rows as canonical CBOR `[columns, rows]`.
    pub fn dataframe_preview(
        &self,
        workspace: String,
        name: String,
        rows: u64,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        dataframe_batch_cbor(loom_core::dataframe_preview(&self.loom, ns, &name, rows).map_err(le)?)
    }

    /// Materialize dataframe frame `name`; returns a CAS digest when the materialization target emits one.
    pub fn dataframe_materialize(
        &mut self,
        workspace: String,
        name: String,
    ) -> Result<Option<String>, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let digest = loom_core::dataframe_materialize(&mut self.loom, ns, &name).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(digest.map(|digest| digest.to_string()))
    }

    /// Canonical dataframe plan digest as `algo:hex`.
    pub fn dataframe_plan_digest(
        &self,
        workspace: String,
        name: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(loom_core::dataframe_plan_digest(&self.loom, ns, &name)
            .map_err(le)?
            .to_string())
    }

    /// Source digests pinned in the dataframe plan as canonical CBOR array of `algo:hex` strings.
    pub fn dataframe_source_digests(
        &self,
        workspace: String,
        name: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        digest_list_cbor(loom_core::dataframe_source_digests(&self.loom, ns, &name).map_err(le)?)
    }
}
