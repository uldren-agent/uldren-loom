//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::StudioMaintenance as GeneratedStudioMaintenance;

#[pyfunction]
#[pyo3(signature = (path, workspace, profile, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn studio_reindex_json(
    path: &str,
    workspace: &str,
    profile: &str,
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
        <loom_client::LocalLoomClient as GeneratedStudioMaintenance>::studio_reindex_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            profile.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, profile, dry_run, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn studio_revisions_rebuild_json(
    path: &str,
    workspace: &str,
    profile: &str,
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
        <loom_client::LocalLoomClient as GeneratedStudioMaintenance>::studio_revisions_rebuild_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            profile.to_string(),
            dry_run,
        ),
    )
    .map_err(py_err)
}
