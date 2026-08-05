//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use loom_core::WorkspaceId;

use super::*;

fn open_security_session(
    path: &str,
    store_passphrase: Option<&str>,
    principal: Option<&str>,
    principal_passphrase: Option<&str>,
) -> napi::Result<generated_session::GeneratedSession> {
    generated_session::open_generated_session(
        path,
        store_passphrase,
        principal,
        principal_passphrase,
    )
}

#[napi]
pub fn audit_config_show_json(
    path: String,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .audit_config_show_json(&generated.session)
        .map_err(reason)
}

#[napi]
pub fn identity_force_detach_authority_json(
    path: String,
    authority_principal: String,
    generation: i64,
    detach_reason: String,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    let authority_principal = WorkspaceId::parse(&authority_principal).map_err(reason)?;
    let generation = u64::try_from(generation)
        .map_err(|_| napi::Error::from_reason("generation must be non-negative"))?;
    generated
        .client
        .identity_force_detach_authority_json(
            &generated.session,
            authority_principal,
            generation,
            &detach_reason,
        )
        .map_err(reason)
}

#[napi]
pub fn identity_replicate_authority_json(
    path: String,
    source: String,
    become_authority: bool,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .identity_replicate_authority_json(&generated.session, &source, become_authority)
        .map_err(reason)
}

#[napi]
pub fn identity_configure_authority_replication_json(
    path: String,
    id: String,
    source: String,
    disabled: bool,
    pull_on_start: bool,
    interval_ms: Option<i64>,
    jitter_ms: i64,
    backoff_ms: i64,
    publish_witness: bool,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    let interval_ms = interval_ms
        .map(|value| {
            u64::try_from(value)
                .map_err(|_| napi::Error::from_reason("interval_ms must be non-negative"))
        })
        .transpose()?;
    let jitter_ms = u64::try_from(jitter_ms)
        .map_err(|_| napi::Error::from_reason("jitter_ms must be non-negative"))?;
    let backoff_ms = u64::try_from(backoff_ms)
        .map_err(|_| napi::Error::from_reason("backoff_ms must be non-negative"))?;
    generated
        .client
        .identity_configure_authority_replication_json(
            &generated.session,
            &id,
            &source,
            disabled,
            pull_on_start,
            interval_ms,
            jitter_ms,
            backoff_ms,
            publish_witness,
        )
        .map_err(reason)
}

#[napi]
pub fn identity_remove_authority_replication_json(
    path: String,
    id: String,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .identity_remove_authority_replication_json(&generated.session, &id)
        .map_err(reason)
}

#[napi]
pub fn audit_config_set_json(
    path: String,
    retention_days: Option<u32>,
    legal_hold: Option<bool>,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .audit_config_set_json(&generated.session, retention_days, legal_hold)
        .map_err(reason)
}

#[napi]
pub fn audit_list_json(
    path: String,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .audit_list_json(&generated.session)
        .map_err(reason)
}

#[napi]
pub fn audit_view_json(
    path: String,
    record: String,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .audit_view_json(&generated.session, &record)
        .map_err(reason)
}

#[napi]
pub fn certificate_list_json(
    path: String,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .certificate_list_json(&generated.session)
        .map_err(reason)
}

#[napi]
pub fn certificate_import_json(
    path: String,
    name: String,
    cert_chain_pem: Uint8Array,
    private_key_pem: Uint8Array,
    trust_bundle_pem: Option<Uint8Array>,
    force: bool,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .certificate_import_json(
            &generated.session,
            &name,
            cert_chain_pem.to_vec(),
            private_key_pem.to_vec(),
            trust_bundle_pem.map(|bytes| bytes.to_vec()),
            force,
        )
        .map_err(reason)
}

#[napi]
pub fn certificate_export(
    path: String,
    name: String,
    include_cert_chain: bool,
    include_private_key: bool,
    include_trust_bundle: bool,
    force: bool,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .certificate_export(
            &generated.session,
            &name,
            include_cert_chain,
            include_private_key,
            include_trust_bundle,
            force,
        )
        .map(Uint8Array::from)
        .map_err(reason)
}

#[napi]
pub fn certificate_generate_self_signed_json(
    path: String,
    name: String,
    dns_names: Vec<String>,
    ip_addresses: Vec<String>,
    cn: Option<String>,
    days: u32,
    algorithm: String,
    force: bool,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .certificate_generate_self_signed_json(
            &generated.session,
            &name,
            dns_names,
            ip_addresses,
            cn.as_deref(),
            days,
            &algorithm,
            force,
        )
        .map_err(reason)
}

#[napi]
pub fn certificate_remove_json(
    path: String,
    name: String,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .certificate_remove_json(&generated.session, &name)
        .map_err(reason)
}

#[napi]
pub fn certificate_audit_json(
    path: String,
    name: String,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .certificate_audit_json(&generated.session, &name)
        .map_err(reason)
}

#[napi]
pub fn network_access_list_json(
    path: String,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .network_access_list_json(&generated.session)
        .map_err(reason)
}

#[napi]
pub fn network_access_set_json(
    path: String,
    name: String,
    description: Option<String>,
    default_action: String,
    rules_json: String,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .network_access_set_json(
            &generated.session,
            &name,
            description.as_deref(),
            &default_action,
            &rules_json,
        )
        .map_err(reason)
}

#[napi]
pub fn network_access_remove_json(
    path: String,
    name: String,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .network_access_remove_json(&generated.session, &name)
        .map_err(reason)
}

#[napi]
pub fn network_access_audit_json(
    path: String,
    name: String,
    store_passphrase: Option<String>,
    principal: Option<String>,
    principal_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = open_security_session(
        &path,
        store_passphrase.as_deref(),
        principal.as_deref(),
        principal_passphrase.as_deref(),
    )?;
    generated
        .client
        .network_access_audit_json(&generated.session, &name)
        .map_err(reason)
}
