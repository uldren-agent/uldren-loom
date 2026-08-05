//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

fn open_security_session(
    path: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<generated_session::GeneratedSession> {
    generated_session::open_generated_session(
        path,
        store_passphrase,
        principal,
        principal_passphrase,
    )
}

#[pyfunction]
#[pyo3(signature = (path, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn audit_config_show_json(
    path: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .audit_config_show_json(&generated.session)
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, authority_principal, generation, detach_reason, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn identity_force_detach_authority_json(
    path: &str,
    authority_principal: &str,
    generation: u64,
    detach_reason: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    let authority_principal = WorkspaceId::parse(authority_principal).map_err(py_err)?;
    generated
        .client
        .identity_force_detach_authority_json(
            &generated.session,
            authority_principal,
            generation,
            detach_reason,
        )
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, source, become_authority, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn identity_replicate_authority_json(
    path: &str,
    source: &str,
    become_authority: bool,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .identity_replicate_authority_json(&generated.session, source, become_authority)
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, id, source, disabled, pull_on_start, interval_ms, jitter_ms, backoff_ms, publish_witness, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn identity_configure_authority_replication_json(
    path: &str,
    id: &str,
    source: &str,
    disabled: bool,
    pull_on_start: bool,
    interval_ms: Option<u64>,
    jitter_ms: u64,
    backoff_ms: u64,
    publish_witness: bool,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .identity_configure_authority_replication_json(
            &generated.session,
            id,
            source,
            disabled,
            pull_on_start,
            interval_ms,
            jitter_ms,
            backoff_ms,
            publish_witness,
        )
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, id, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn identity_remove_authority_replication_json(
    path: &str,
    id: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .identity_remove_authority_replication_json(&generated.session, id)
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, retention_days=None, legal_hold=None, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn audit_config_set_json(
    path: &str,
    retention_days: Option<u32>,
    legal_hold: Option<bool>,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .audit_config_set_json(&generated.session, retention_days, legal_hold)
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn audit_list_json(
    path: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .audit_list_json(&generated.session)
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, record, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn audit_view_json(
    path: &str,
    record: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .audit_view_json(&generated.session, record)
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn certificate_list_json(
    path: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .certificate_list_json(&generated.session)
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, name, cert_chain_pem, private_key_pem, trust_bundle_pem=None, force=false, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn certificate_import_json(
    path: &str,
    name: &str,
    cert_chain_pem: &[u8],
    private_key_pem: &[u8],
    trust_bundle_pem: Option<&[u8]>,
    force: bool,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .certificate_import_json(
            &generated.session,
            name,
            cert_chain_pem.to_vec(),
            private_key_pem.to_vec(),
            trust_bundle_pem.map(<[u8]>::to_vec),
            force,
        )
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, name, include_cert_chain, include_private_key, include_trust_bundle, force=false, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn certificate_export<'py>(
    py: Python<'py>,
    path: &str,
    name: &str,
    include_cert_chain: bool,
    include_private_key: bool,
    include_trust_bundle: bool,
    force: bool,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    let bytes = generated
        .client
        .certificate_export(
            &generated.session,
            name,
            include_cert_chain,
            include_private_key,
            include_trust_bundle,
            force,
        )
        .map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, name, dns_names, ip_addresses, cn=None, days=365, algorithm="p256", force=false, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn certificate_generate_self_signed_json(
    path: &str,
    name: &str,
    dns_names: Vec<String>,
    ip_addresses: Vec<String>,
    cn: Option<&str>,
    days: u32,
    algorithm: &str,
    force: bool,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .certificate_generate_self_signed_json(
            &generated.session,
            name,
            dns_names,
            ip_addresses,
            cn,
            days,
            algorithm,
            force,
        )
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, name, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn certificate_remove_json(
    path: &str,
    name: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .certificate_remove_json(&generated.session, name)
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, name, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn certificate_audit_json(
    path: &str,
    name: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .certificate_audit_json(&generated.session, name)
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn network_access_list_json(
    path: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .network_access_list_json(&generated.session)
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, name, description, default_action, rules_json, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn network_access_set_json(
    path: &str,
    name: &str,
    description: Option<&str>,
    default_action: &str,
    rules_json: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .network_access_set_json(
            &generated.session,
            name,
            description,
            default_action,
            rules_json,
        )
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, name, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn network_access_remove_json(
    path: &str,
    name: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .network_access_remove_json(&generated.session, name)
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, name, store_passphrase=None, principal=None, principal_passphrase=None))]
pub(crate) fn network_access_audit_json(
    path: &str,
    name: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = open_security_session(path, store_passphrase, principal, principal_passphrase)?;
    generated
        .client
        .network_access_audit_json(&generated.session, name)
        .map_err(py_err)
}
