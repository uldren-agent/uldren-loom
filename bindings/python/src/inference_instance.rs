//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::InferenceInstance as GeneratedInferenceInstance;

#[pyfunction]
#[pyo3(signature = (path, workspace, name, model, kind, runtime, preset, settings_json, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn inference_instance_create_json(
    path: &str,
    workspace: &str,
    name: &str,
    model: &str,
    kind: &str,
    runtime: &str,
    preset: Option<&str>,
    settings_json: &str,
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
        <loom_client::LocalLoomClient as GeneratedInferenceInstance>::inference_instance_create_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            name.to_string(),
            model.to_string(),
            kind.to_string(),
            runtime.to_string(),
            preset.map(str::to_string),
            settings_json.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, name, preset, settings_json, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn inference_instance_update_json(
    path: &str,
    workspace: &str,
    name: &str,
    preset: Option<&str>,
    settings_json: &str,
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
        <loom_client::LocalLoomClient as GeneratedInferenceInstance>::inference_instance_update_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            name.to_string(),
            preset.map(str::to_string),
            settings_json.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, name, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn inference_instance_delete_json(
    path: &str,
    workspace: &str,
    name: &str,
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
        <loom_client::LocalLoomClient as GeneratedInferenceInstance>::inference_instance_delete_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            name.to_string(),
        ),
    )
    .map_err(py_err)
}
