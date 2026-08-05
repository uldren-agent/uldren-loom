//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::Exec as GeneratedExec;

#[pyfunction]
#[pyo3(signature = (path, request, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn apply_cbor<'py>(
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
        block_on(<loom_client::LocalLoomClient as GeneratedExec>::apply_cbor(
            &generated.client,
            generated.session.clone(),
            request.to_vec(),
        ))
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}
