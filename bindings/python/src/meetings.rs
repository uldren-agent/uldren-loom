//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_interchange_io::{
    import_meetings_bytes, import_report_json, meetings_source_payload_path,
    parse_meetings_input_profile, validate_meetings_source_payload_leaf,
};

#[pyfunction]
#[pyo3(signature = (path, workspace, input_profile, snapshot, dry_run, passphrase=None))]
pub(crate) fn meetings_import_snapshot(
    path: &str,
    workspace: &str,
    input_profile: &str,
    snapshot: &[u8],
    dry_run: bool,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    let profile = parse_meetings_input_profile(input_profile).map_err(py_err)?;
    let result = import_meetings_bytes(&mut loom, workspace_id, profile, snapshot, dry_run)
        .map_err(py_err)?;
    import_report_json(&result.report).map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, source_id, leaf, passphrase=None))]
pub(crate) fn meetings_source_read<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    source_id: &str,
    leaf: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    validate_meetings_source_payload_leaf(leaf).map_err(py_err)?;
    let profile_id = workspace_id.to_string();
    let path = meetings_source_payload_path(&profile_id, source_id, leaf);
    let bytes = loom
        .read_file_reserved(workspace_id, &path)
        .map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}
