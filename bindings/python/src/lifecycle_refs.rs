//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::{Lifecycle as GeneratedLifecycle, Refs as GeneratedRefs};

#[pyfunction]
#[pyo3(signature = (path, workspace, kind, version, completion_predicate_digest, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn lifecycle_define_standard_json(
    path: &str,
    workspace: &str,
    kind: &str,
    version: &str,
    completion_predicate_digest: &str,
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
        <loom_client::LocalLoomClient as GeneratedLifecycle>::lifecycle_define_standard_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            kind.to_string(),
            version.to_string(),
            completion_predicate_digest.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, definition, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn lifecycle_define_json(
    path: &str,
    workspace: &str,
    definition: &[u8],
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
        <loom_client::LocalLoomClient as GeneratedLifecycle>::lifecycle_define_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            definition.to_vec(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, instance_id, definition_id, subject_refs, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn lifecycle_instantiate_json(
    path: &str,
    workspace: &str,
    instance_id: &str,
    definition_id: &str,
    subject_refs: Vec<String>,
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
        <loom_client::LocalLoomClient as GeneratedLifecycle>::lifecycle_instantiate_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            instance_id.to_string(),
            definition_id.to_string(),
            subject_refs,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, instance_id, transition_id, to_stage_id, actor_principal_id, gate_evaluations_json, snapshot_digest=None, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn lifecycle_transition_json(
    path: &str,
    workspace: &str,
    instance_id: &str,
    transition_id: &str,
    to_stage_id: &str,
    actor_principal_id: Option<&str>,
    gate_evaluations_json: &str,
    snapshot_digest: Option<&str>,
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
        <loom_client::LocalLoomClient as GeneratedLifecycle>::lifecycle_transition_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            instance_id.to_string(),
            transition_id.to_string(),
            to_stage_id.to_string(),
            actor_principal_id.map(str::to_string),
            gate_evaluations_json.to_string(),
            snapshot_digest.map(str::to_string),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, max, store_passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn refs_reconcile_json(
    path: &str,
    workspace: &str,
    max: u64,
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
        <loom_client::LocalLoomClient as GeneratedRefs>::refs_reconcile_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            max,
        ),
    )
    .map_err(py_err)
}
