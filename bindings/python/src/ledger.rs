//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Append `payload` to ledger `collection` (created with the `ledger` facet if absent); returns the sequence.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, payload, passphrase=None))]
pub(crate) fn ledger_append(
    path: &str,
    workspace: &str,
    collection: &str,
    payload: &[u8],
    passphrase: Option<&str>,
) -> PyResult<u64> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_ledger_ns(&mut loom, workspace)?;
    let seq =
        loom_core::ledger_append(&mut loom, ns, collection, payload.to_vec()).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(seq)
}
/// The payload at `seq` in ledger `collection`, or `None` if absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, seq, passphrase=None))]
pub(crate) fn ledger_get<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    collection: &str,
    seq: u64,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::ledger_get(&loom, ns, collection, seq)
        .map_err(py_err)?
        .map(|b| PyBytes::new(py, &b)))
}
/// The 32-byte head chain hash of ledger `collection`, or `None` when absent or empty.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, passphrase=None))]
pub(crate) fn ledger_head<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    collection: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::ledger_head(&loom, ns, collection)
        .map_err(py_err)?
        .map(|d| PyBytes::new(py, &d.bytes().to_vec())))
}
/// The number of entries in ledger `collection` (0 when absent).
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, passphrase=None))]
pub(crate) fn ledger_len(
    path: &str,
    workspace: &str,
    collection: &str,
    passphrase: Option<&str>,
) -> PyResult<u64> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_core::ledger_len(&loom, ns, collection).map_err(py_err)
}
/// Recompute and verify ledger `collection`'s hash chain; an altered payload or broken link raises.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, passphrase=None))]
pub(crate) fn ledger_verify(
    path: &str,
    workspace: &str,
    collection: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_core::ledger_verify(&loom, ns, collection).map_err(py_err)
}
