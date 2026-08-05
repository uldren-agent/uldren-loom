//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use futures::executor::block_on;
use loom_client::generated_api::Meetings as GeneratedMeetings;
use loom_interchange_io::{meetings_source_payload_path, validate_meetings_source_payload_leaf};

#[pyfunction]
#[pyo3(signature = (path, workspace, input_profile, snapshot, dry_run, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn meetings_import_snapshot(
    path: &str,
    workspace: &str,
    input_profile: &str,
    snapshot: &[u8],
    dry_run: bool,
    store_passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(
        path,
        store_passphrase,
        auth_principal,
        auth_passphrase,
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedMeetings>::meetings_import_snapshot(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            input_profile.to_string(),
            snapshot.to_vec(),
            dry_run,
        ),
    )
    .map_err(py_err)
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
