//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::tabular::cell_value;
use loom_core::{DataframeBatch, DataframePlan};

fn ensure_dataframe_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Dataframe)
        .map_err(py_err)?;
    Ok(ns)
}

fn dataframe_batch_cbor(batch: DataframeBatch) -> PyResult<Vec<u8>> {
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
    .map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))
}

fn digest_list_cbor(digests: Vec<Digest>) -> PyResult<Vec<u8>> {
    let values = digests
        .into_iter()
        .map(|digest| CborValue::Text(digest.to_string()))
        .collect::<Vec<_>>();
    cbor_encode(&CborValue::Array(values))
        .map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))
}

/// Create dataframe frame `name` from canonical DataframePlan CBOR.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, plan, passphrase=None))]
pub(crate) fn dataframe_create(
    path: &str,
    workspace: &str,
    name: &str,
    plan: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let plan = DataframePlan::decode(plan).map_err(py_err)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_dataframe_ns(&mut loom, workspace)?;
    loom_core::dataframe_create(&mut loom, ns, name, &plan).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}

/// Execute dataframe frame `name` and return canonical CBOR `[columns, rows]`.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn dataframe_collect<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes =
        dataframe_batch_cbor(loom_core::dataframe_collect(&loom, ns, name).map_err(py_err)?)?;
    Ok(PyBytes::new(py, &bytes))
}

/// Execute dataframe frame `name` and return at most `rows` rows as canonical CBOR `[columns, rows]`.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, rows, passphrase=None))]
pub(crate) fn dataframe_preview<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    rows: u64,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes =
        dataframe_batch_cbor(loom_core::dataframe_preview(&loom, ns, name, rows).map_err(py_err)?)?;
    Ok(PyBytes::new(py, &bytes))
}

/// Materialize dataframe frame `name`; returns a CAS digest when the materialization target emits one.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn dataframe_materialize(
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<String>> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let digest = loom_core::dataframe_materialize(&mut loom, ns, name).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(digest.map(|digest| digest.to_string()))
}

/// Canonical dataframe plan digest as `algo:hex`.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn dataframe_plan_digest(
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::dataframe_plan_digest(&loom, ns, name)
        .map_err(py_err)?
        .to_string())
}

/// Source digests pinned in the dataframe plan as canonical CBOR array of `algo:hex` strings.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn dataframe_source_digests<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes =
        digest_list_cbor(loom_core::dataframe_source_digests(&loom, ns, name).map_err(py_err)?)?;
    Ok(PyBytes::new(py, &bytes))
}
