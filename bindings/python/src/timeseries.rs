//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Put `value` at timestamp `ts` in series `collection` (created with the `time-series` facet if absent).
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, ts, value, passphrase=None))]
pub(crate) fn ts_put(
    path: &str,
    workspace: &str,
    collection: &str,
    ts: i64,
    value: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_ts_ns(&mut loom, workspace)?;
    loom_core::ts_put(&mut loom, ns, collection, ts, value.to_vec()).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// The point at timestamp `ts` in series `collection`, or `None` if absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, ts, passphrase=None))]
pub(crate) fn ts_get<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    collection: &str,
    ts: i64,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::ts_get(&loom, ns, collection, ts)
        .map_err(py_err)?
        .map(|b| PyBytes::new(py, &b)))
}
/// The points of series `collection` with `from <= ts < to` (half-open) as CBOR `[ts, value]` pairs.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, from_ts, to_ts, passphrase=None))]
pub(crate) fn ts_range<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    collection: &str,
    from_ts: i64,
    to_ts: i64,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = loom_core::ts_range(&loom, ns, collection, from_ts, to_ts)
        .map_err(py_err)?
        .encode();
    Ok(PyBytes::new(py, &bytes))
}
/// The most recent point of series `collection` as a one-point CBOR array, or `None` if absent/empty.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, passphrase=None))]
pub(crate) fn ts_latest<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    collection: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::ts_latest(&loom, ns, collection)
        .map_err(py_err)?
        .map(|(ts, v)| {
            let mut s = loom_core::Series::new();
            s.put(ts, v);
            PyBytes::new(py, &s.encode())
        }))
}
