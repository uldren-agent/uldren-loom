//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::Sql as GeneratedSql;

#[pyfunction]
#[pyo3(signature = (path, workspace, db, sql, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn sql_exec_result<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    db: &str,
    sql: &str,
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
            <loom_client::LocalLoomClient as GeneratedSql>::sql_exec_result(
                &generated.client,
                generated.session.clone(),
                workspace.to_string(),
                db.to_string(),
                sql.to_string(),
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}
