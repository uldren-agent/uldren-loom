//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::ServeConfig as GeneratedServeConfig;

#[pyfunction]
#[pyo3(signature = (path, request_json, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn serve_listener_configure_json(
    path: &str,
    request_json: &str,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_listener_configure_json(
            &generated.client,
            generated.session.clone(),
            request_json.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn serve_listener_list_json(
    path: &str,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_listener_list_json(
            &generated.client,
            generated.session.clone(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, listener_id, enabled, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn serve_listener_set_enabled_json(
    path: &str,
    listener_id: &str,
    enabled: bool,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_listener_set_enabled_json(
            &generated.client,
            generated.session.clone(),
            listener_id.to_string(),
            enabled,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, listener_id, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn serve_listener_remove_json(
    path: &str,
    listener_id: &str,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_listener_remove_json(
            &generated.client,
            generated.session.clone(),
            listener_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, listener_id, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn serve_web_route_list_json(
    path: &str,
    listener_id: &str,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_web_route_list_json(
            &generated.client,
            generated.session.clone(),
            listener_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, request_json, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn serve_web_route_set_json(
    path: &str,
    request_json: &str,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_web_route_set_json(
            &generated.client,
            generated.session.clone(),
            request_json.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, listener_id, route_id, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn serve_web_route_remove_json(
    path: &str,
    listener_id: &str,
    route_id: &str,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_web_route_remove_json(
            &generated.client,
            generated.session.clone(),
            listener_id.to_string(),
            route_id.to_string(),
        ),
    )
    .map_err(py_err)
}
