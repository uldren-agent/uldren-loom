//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::InterchangeProfiles as GeneratedInterchangeProfiles;

#[pyfunction]
#[pyo3(signature = (path, workspace, source_scope, csv_payload, database, table, schema, primary_key, mode, commit, author, message, dry_run, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn import_table_csv<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    source_scope: &str,
    csv_payload: &[u8],
    database: &str,
    table: &str,
    schema: &str,
    primary_key: &str,
    mode: &str,
    commit: bool,
    author: Option<&str>,
    message: Option<&str>,
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
            <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_table_csv(
                &generated.client,
                generated.session.clone(),
                workspace.to_string(),
                source_scope.to_string(),
                csv_payload.to_vec(),
                database.to_string(),
                table.to_string(),
                schema.to_string(),
                primary_key.to_string(),
                mode.to_string(),
                commit,
                author.map(str::to_string),
                message.map(str::to_string),
                dry_run,
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, profile, source_scope, snapshot_payload, field_policy, dry_run, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn import_redmine<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    profile: &str,
    source_scope: &str,
    snapshot_payload: &[u8],
    field_policy: &str,
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
            <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_redmine(
                &generated.client,
                generated.session.clone(),
                workspace.to_string(),
                profile.to_string(),
                source_scope.to_string(),
                snapshot_payload.to_vec(),
                field_policy.to_string(),
                dry_run,
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, profile, source_scope, snapshot_payload, field_policy, dry_run, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn import_asana<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    profile: &str,
    source_scope: &str,
    snapshot_payload: &[u8],
    field_policy: &str,
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
            <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_asana(
                &generated.client,
                generated.session.clone(),
                workspace.to_string(),
                profile.to_string(),
                source_scope.to_string(),
                snapshot_payload.to_vec(),
                field_policy.to_string(),
                dry_run,
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, profile, source_scope, snapshot_payload, field_policy, dry_run, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn import_jira<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    profile: &str,
    source_scope: &str,
    snapshot_payload: &[u8],
    field_policy: &str,
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
            <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_jira(
                &generated.client,
                generated.session.clone(),
                workspace.to_string(),
                profile.to_string(),
                source_scope.to_string(),
                snapshot_payload.to_vec(),
                field_policy.to_string(),
                dry_run,
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, profile, source_scope, snapshot_payload, default_space, dry_run, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn import_confluence<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    profile: &str,
    source_scope: &str,
    snapshot_payload: &[u8],
    default_space: &str,
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
            <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_confluence(
                &generated.client,
                generated.session.clone(),
                workspace.to_string(),
                profile.to_string(),
                source_scope.to_string(),
                snapshot_payload.to_vec(),
                default_space.to_string(),
                dry_run,
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, profile, source_scope, snapshot_payload, dry_run, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn import_slack<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    profile: &str,
    source_scope: &str,
    snapshot_payload: &[u8],
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
            <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_slack(
                &generated.client,
                generated.session.clone(),
                workspace.to_string(),
                profile.to_string(),
                source_scope.to_string(),
                snapshot_payload.to_vec(),
                dry_run,
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, profile, source_scope, archive_payload, dry_run, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn import_drive<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    profile: &str,
    source_scope: &str,
    archive_payload: &[u8],
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
            <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_drive(
                &generated.client,
                generated.session.clone(),
                workspace.to_string(),
                profile.to_string(),
                source_scope.to_string(),
                archive_payload.to_vec(),
                dry_run,
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, profile, source_scope, archive_payload, space, dry_run, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn import_markdown<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    profile: &str,
    source_scope: &str,
    archive_payload: &[u8],
    space: &str,
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
            <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_markdown(
                &generated.client,
                generated.session.clone(),
                workspace.to_string(),
                profile.to_string(),
                source_scope.to_string(),
                archive_payload.to_vec(),
                space.to_string(),
                dry_run,
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, profile, source_scope, snapshot_payload, default_space, dry_run, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn import_notion<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    profile: &str,
    source_scope: &str,
    snapshot_payload: &[u8],
    default_space: &str,
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
            <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_notion(
                &generated.client,
                generated.session.clone(),
                workspace.to_string(),
                profile.to_string(),
                source_scope.to_string(),
                snapshot_payload.to_vec(),
                default_space.to_string(),
                dry_run,
            ),
        )
        .map_err(py_err)
    })?;
    Ok(PyBytes::new(py, &bytes))
}
