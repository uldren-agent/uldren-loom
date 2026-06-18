//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[pyfunction]
#[pyo3(signature = (path, name=None, facet=None, passphrase=None))]
pub(crate) fn workspace_create(
    path: &str,
    name: Option<&str>,
    facet: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let id = random_workspace_id()?;
    let name = name.filter(|value| !value.is_empty());
    let ns = match facet.filter(|value| !value.is_empty()) {
        Some(facet) => loom
            .registry_mut()
            .create(FacetKind::parse(facet).map_err(py_err)?, name, id)
            .map_err(py_err)?,
        None => loom
            .registry_mut()
            .create_workspace(name, id)
            .map_err(py_err)?,
    };
    save_loom(&mut loom).map_err(py_err)?;
    Ok(ns.to_string())
}
#[pyfunction]
#[pyo3(signature = (path, passphrase=None))]
pub(crate) fn workspace_list_json(path: &str, passphrase: Option<&str>) -> PyResult<String> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    Ok(workspace_list_json_inner(&loom))
}
#[pyfunction]
#[pyo3(signature = (path, workspace, new_name, passphrase=None))]
pub(crate) fn workspace_rename(
    path: &str,
    workspace: &str,
    new_name: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom.registry_mut().rename(ns, new_name).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)
}
#[pyfunction]
#[pyo3(signature = (path, workspace, passphrase=None))]
pub(crate) fn workspace_delete(
    path: &str,
    workspace: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom.registry_mut().delete(ns).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)
}
