//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::Audit as GeneratedAudit;
use loom_client::generated_api::StoreAdmin as GeneratedStoreAdmin;

#[pyfunction]
#[pyo3(signature = (path, through_seq, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn audit_compact<'py>(
    py: Python<'py>,
    path: &str,
    through_seq: u64,
    store_passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = py.allow_threads(|| -> PyResult<Vec<u8>> {
        let generated = generated_session::open_generated_session(
            path,
            store_passphrase,
            auth_principal,
            auth_passphrase,
        )?;
        block_on(
            <loom_client::LocalLoomClient as GeneratedAudit>::audit_compact(
                &generated.client,
                generated.session.clone(),
                through_seq,
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, bundle, dry_run, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn store_bundle_import<'py>(
    py: Python<'py>,
    path: &str,
    bundle: &[u8],
    dry_run: bool,
    store_passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = py.allow_threads(|| -> PyResult<Vec<u8>> {
        let generated = generated_session::open_generated_session(
            path,
            store_passphrase,
            auth_principal,
            auth_passphrase,
        )?;
        block_on(
            <loom_client::LocalLoomClient as GeneratedStoreAdmin>::store_bundle_import(
                &generated.client,
                generated.session.clone(),
                bundle.to_vec(),
                dry_run,
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, request, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn store_maintenance_status<'py>(
    py: Python<'py>,
    path: &str,
    request: &[u8],
    store_passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = py.allow_threads(|| -> PyResult<Vec<u8>> {
        let generated = generated_session::open_generated_session(
            path,
            store_passphrase,
            auth_principal,
            auth_passphrase,
        )?;
        block_on(
            <loom_client::LocalLoomClient as GeneratedStoreAdmin>::store_maintenance_status(
                &generated.client,
                generated.session.clone(),
                request.to_vec(),
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, update, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn store_maintenance_policy_set<'py>(
    py: Python<'py>,
    path: &str,
    update: &[u8],
    store_passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = py.allow_threads(|| -> PyResult<Vec<u8>> {
        let generated = generated_session::open_generated_session(
            path,
            store_passphrase,
            auth_principal,
            auth_passphrase,
        )?;
        block_on(
            <loom_client::LocalLoomClient as GeneratedStoreAdmin>::store_maintenance_policy_set(
                &generated.client,
                generated.session.clone(),
                update.to_vec(),
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, request, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn store_maintenance_run<'py>(
    py: Python<'py>,
    path: &str,
    request: &[u8],
    store_passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let bytes = py.allow_threads(|| -> PyResult<Vec<u8>> {
        let generated = generated_session::open_generated_session(
            path,
            store_passphrase,
            auth_principal,
            auth_passphrase,
        )?;
        block_on(
            <loom_client::LocalLoomClient as GeneratedStoreAdmin>::store_maintenance_run(
                &generated.client,
                generated.session.clone(),
                request.to_vec(),
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}
