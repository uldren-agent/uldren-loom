//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::tabular::cell_value;
use loom_core::{DataframeBatch, DataframePlan};

fn ensure_dataframe_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
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
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Dataframe)
        .map_err(reason)?;
    Ok(ns)
}

fn dataframe_batch_cbor(batch: DataframeBatch) -> napi::Result<Vec<u8>> {
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
    .map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))
}

fn digest_list_cbor(digests: Vec<Digest>) -> napi::Result<Vec<u8>> {
    let values = digests
        .into_iter()
        .map(|digest| CborValue::Text(digest.to_string()))
        .collect::<Vec<_>>();
    cbor_encode(&CborValue::Array(values))
        .map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))
}

/// Create dataframe frame `name` from canonical DataframePlan CBOR.
#[napi]
pub fn dataframe_create(
    loom_path: String,
    workspace: String,
    name: String,
    plan: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let plan = DataframePlan::decode(&plan).map_err(reason)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_dataframe_ns(&mut loom, &workspace)?;
    loom_core::dataframe_create(&mut loom, ns, &name, &plan).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}

/// Execute dataframe frame `name` and return canonical CBOR `[columns, rows]`.
#[napi]
pub fn dataframe_collect(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let bytes =
        dataframe_batch_cbor(loom_core::dataframe_collect(&loom, ns, &name).map_err(reason)?)?;
    Ok(bytes.into())
}

/// Execute dataframe frame `name` and return at most `rows` rows as canonical CBOR `[columns, rows]`.
#[napi]
pub fn dataframe_preview(
    loom_path: String,
    workspace: String,
    name: String,
    rows: BigInt,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let rows = bigint_to_u64(rows, "rows")?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let bytes = dataframe_batch_cbor(
        loom_core::dataframe_preview(&loom, ns, &name, rows).map_err(reason)?,
    )?;
    Ok(bytes.into())
}

/// Materialize dataframe frame `name`; returns a CAS digest when the materialization target emits one.
#[napi]
pub fn dataframe_materialize(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<Option<String>> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let digest = loom_core::dataframe_materialize(&mut loom, ns, &name).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(digest.map(|digest| digest.to_string()))
}

/// Canonical dataframe plan digest as `algo:hex`.
#[napi]
pub fn dataframe_plan_digest(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::dataframe_plan_digest(&loom, ns, &name)
        .map_err(reason)?
        .to_string())
}

/// Source digests pinned in the dataframe plan as canonical CBOR array of `algo:hex` strings.
#[napi]
pub fn dataframe_source_digests(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let bytes =
        digest_list_cbor(loom_core::dataframe_source_digests(&loom, ns, &name).map_err(reason)?)?;
    Ok(bytes.into())
}
