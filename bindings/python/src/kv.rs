//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Put `value` at the typed `key` (Loom Canonical CBOR cell) in map `collection` of `workspace` (UUID or name,
/// created with the `kv` facet if absent). A later put at the same key replaces the value.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, key, value, passphrase=None))]
pub(crate) fn kv_put(
    path: &str,
    workspace: &str,
    collection: &str,
    key: &[u8],
    value: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_kv_ns(&mut loom, workspace)?;
    let key = loom_core::key_from_cbor(key).map_err(py_err)?;
    loom_core::kv_put(&mut loom, ns, collection, key, value.to_vec()).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Fetch the value at typed `key` in map `collection` of `workspace`, or `None` if the key or map is absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, key, passphrase=None))]
pub(crate) fn kv_get<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    collection: &str,
    key: &[u8],
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let key = loom_core::key_from_cbor(key).map_err(py_err)?;
    Ok(loom_core::kv_get(&loom, ns, collection, &key)
        .map_err(py_err)?
        .map(|bytes| PyBytes::new(py, &bytes)))
}
/// Remove the typed `key` from map `collection` of `workspace`; returns whether it was present.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, key, passphrase=None))]
pub(crate) fn kv_delete(
    path: &str,
    workspace: &str,
    collection: &str,
    key: &[u8],
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let key = loom_core::key_from_cbor(key).map_err(py_err)?;
    let present = loom_core::kv_delete(&mut loom, ns, collection, &key).map_err(py_err)?;
    if present {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(present)
}
/// List map `collection` of `workspace` as the Loom Canonical CBOR array of `[key, value]` pairs in key order
/// (an absent map is the empty array).
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, passphrase=None))]
pub(crate) fn kv_list<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    collection: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = loom_core::kv_list(&loom, ns, collection)
        .map_err(py_err)?
        .encode();
    Ok(PyBytes::new(py, &bytes))
}
/// The entries of map `collection` with `lo <= key < hi` (half-open, key order) as the Loom Canonical CBOR
/// array of `[key, value]` pairs. `lo`/`hi` are typed-cell CBOR keys.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, lo, hi, passphrase=None))]
pub(crate) fn kv_range<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    collection: &str,
    lo: &[u8],
    hi: &[u8],
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let lo = loom_core::key_from_cbor(lo).map_err(py_err)?;
    let hi = loom_core::key_from_cbor(hi).map_err(py_err)?;
    let bytes = loom_core::kv_range(&loom, ns, collection, &lo, &hi)
        .map_err(py_err)?
        .encode();
    Ok(PyBytes::new(py, &bytes))
}
