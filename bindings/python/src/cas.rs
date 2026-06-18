//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Store `content` in the `cas` facet of a workspace (by UUID or name, created if absent); returns the
/// content address (`"algo:hex"`). Idempotent: identical bytes yield the same address.
#[pyfunction]
#[pyo3(signature = (path, workspace, content, passphrase=None))]
pub(crate) fn cas_put(
    path: &str,
    workspace: &str,
    content: &[u8],
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_cas_ns(&mut loom, workspace)?;
    let digest = loom_core::cas_put(&mut loom, ns, content).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(digest.to_string())
}
/// Fetch the blob addressed by `digest` from a workspace, or `None` if absent. An invalid digest raises
/// `INVALID_ARGUMENT`; a content/digest mismatch raises `INTEGRITY_FAILURE`.
#[pyfunction]
#[pyo3(signature = (path, workspace, digest, passphrase=None))]
pub(crate) fn cas_get<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    digest: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let digest = Digest::parse(digest).map_err(py_err)?;
    Ok(loom_core::cas_get(&loom, ns, &digest)
        .map_err(py_err)?
        .map(|bytes| PyBytes::new(py, &bytes)))
}
/// Whether a blob addressed by `digest` is present in a workspace. An invalid digest raises
/// `INVALID_ARGUMENT`.
#[pyfunction]
#[pyo3(signature = (path, workspace, digest, passphrase=None))]
pub(crate) fn cas_has(
    path: &str,
    workspace: &str,
    digest: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let digest = Digest::parse(digest).map_err(py_err)?;
    loom_core::cas_has(&loom, ns, &digest).map_err(py_err)
}
/// Drop the blob addressed by `digest` from a workspace's working tree, making it unreachable going
/// forward; returns whether it was present. CAS stays immutable; bytes are reclaimed by GC once
/// unreferenced, and an earlier commit that held the blob still restores it.
#[pyfunction]
#[pyo3(signature = (path, workspace, digest, passphrase=None))]
pub(crate) fn cas_delete(
    path: &str,
    workspace: &str,
    digest: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let digest = Digest::parse(digest).map_err(py_err)?;
    let present = loom_core::cas_delete(&mut loom, ns, &digest).map_err(py_err)?;
    if present {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(present)
}
/// List the content addresses (`"algo:hex"`) reachable in a workspace's `cas` facet as a JSON string
/// array, sorted. Enumeration is within the workspace, not a global index.
#[pyfunction]
#[pyo3(signature = (path, workspace, passphrase=None))]
pub(crate) fn cas_list_json(
    path: &str,
    workspace: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let list = loom_core::cas_list(&loom, ns).map_err(py_err)?;
    let mut out = String::from("[");
    for (i, d) in list.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&d.to_string());
        out.push('"');
    }
    out.push(']');
    Ok(out)
}
