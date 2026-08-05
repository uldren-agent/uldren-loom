//! Shared security-administration service for local Loom stores.
//!
//! Licensed under BUSL-1.1.

use loom_core::Loom;
use loom_core::digest::Digest;
use loom_store::FileStore;
use loom_types::{Code, LoomError};
#[cfg(not(target_arch = "wasm32"))]
use rustls::pki_types::pem::PemObject as _;
use std::collections::BTreeMap;

fn json_quote(value: &str) -> String {
    serde_json::to_string(value).expect("json string encode")
}

fn push_json_option(out: &mut String, value: Option<&str>) {
    match value {
        Some(value) => out.push_str(&json_quote(value)),
        None => out.push_str("null"),
    }
}

fn audit_config_json(config: loom_store::AuditConfig) -> String {
    format!(
        "{{\"retention_days\":{},\"legal_hold\":{}}}",
        config.retention_days, config.legal_hold
    )
}

fn audit_record_json(record: &loom_store::AuditRecord) -> String {
    let principal = record
        .principal
        .map(|principal| json_quote(&principal.to_string()))
        .unwrap_or_else(|| "null".to_string());
    let target = record
        .target
        .as_deref()
        .map(json_quote)
        .unwrap_or_else(|| "null".to_string());
    let prev_hash = record
        .prev_hash
        .map(|hash| json_quote(&hash.to_string()))
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"seq\":{},\"hash\":{},\"principal\":{},\"action\":{},\"target\":{},\"prev_hash\":{}}}",
        record.seq,
        json_quote(&record.hash.to_string()),
        principal,
        json_quote(&record.action),
        target,
        prev_hash
    )
}

fn find_audit_record<'a>(
    records: &'a [loom_store::AuditRecord],
    record: &str,
) -> Result<&'a loom_store::AuditRecord, LoomError> {
    if let Ok(seq) = record.parse::<u64>() {
        return records
            .iter()
            .find(|entry| entry.seq == seq)
            .ok_or_else(|| LoomError::not_found(format!("audit record not found: {record}")));
    }
    let digest = Digest::parse(record)?;
    records
        .iter()
        .find(|entry| entry.hash == digest)
        .ok_or_else(|| LoomError::not_found(format!("audit record not found: {record}")))
}

fn optional_bytes_value(value: Option<Vec<u8>>) -> loom_codec::Value {
    value.map_or(loom_codec::Value::Null, loom_codec::Value::Bytes)
}

fn certificate_bundle_target(name: &str) -> String {
    format!("name={name}")
}

fn certificate_bundle_json(
    record: &loom_store::CertificateBundleRecord,
    seq: u64,
    references: &[String],
) -> String {
    format!(
        "{{\"seq\":{},{}",
        seq,
        &certificate_bundle_record_json(record, references)[1..]
    )
}

fn certificate_bundle_record_json(
    record: &loom_store::CertificateBundleRecord,
    references: &[String],
) -> String {
    let trust_digest = record
        .trust_bundle_digest
        .map(|digest| json_quote(&digest.to_string()))
        .unwrap_or_else(|| "null".to_string());
    let mut out = String::new();
    out.push('{');
    out.push_str("\"name\":");
    out.push_str(&json_quote(&record.name));
    out.push_str(",\"schema_version\":");
    out.push_str(&record.schema_version.to_string());
    out.push_str(",\"profile\":");
    out.push_str(&json_quote(&record.profile));
    out.push_str(",\"health\":{\"status\":\"ok\",\"reasons\":[]}");
    out.push_str(",\"server_certificates\":1");
    out.push_str(",\"server_cert_chain_digest\":");
    out.push_str(&json_quote(&record.server_cert_chain_digest.to_string()));
    out.push_str(",\"private_key_digest\":");
    out.push_str(&json_quote(&record.private_key_digest.to_string()));
    out.push_str(",\"trust_bundle_certificates\":");
    out.push_str(if record.trust_bundle_pem.is_some() {
        "1"
    } else {
        "null"
    });
    out.push_str(",\"trust_bundle_digest\":");
    out.push_str(&trust_digest);
    out.push_str(",\"reference_count\":");
    out.push_str(&references.len().to_string());
    out.push_str(",\"served_listener_references\":[");
    for (idx, reference) in references.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_quote(reference));
    }
    out.push_str("],\"created_audit_seq\":");
    push_json_u64_option(&mut out, record.created_audit_seq);
    out.push_str(",\"updated_audit_seq\":");
    push_json_u64_option(&mut out, record.updated_audit_seq);
    out.push_str(",\"unencrypted_private_key_override\":");
    out.push_str(if record.unencrypted_private_key_override {
        "true"
    } else {
        "false"
    });
    out.push('}');
    out
}

fn push_json_u64_option(out: &mut String, value: Option<u64>) {
    match value {
        Some(value) => out.push_str(&value.to_string()),
        None => out.push_str("null"),
    }
}

fn certificate_bundle_served_listener_reference_map(
    store: &FileStore,
) -> Result<BTreeMap<String, Vec<String>>, LoomError> {
    let mut references = BTreeMap::<String, Vec<String>>::new();
    for record in store.served_listeners()? {
        if let Some(name) = record.tls.certificate_bundle_ref.as_deref() {
            references
                .entry(name.to_string())
                .or_default()
                .push(record.id);
        }
    }
    Ok(references)
}

fn certificate_references_for<'a>(
    references: &'a BTreeMap<String, Vec<String>>,
    name: &str,
) -> &'a [String] {
    references.get(name).map(Vec::as_slice).unwrap_or(&[])
}

fn certificate_bundle_served_listener_references(
    store: &FileStore,
    name: &str,
) -> Result<Vec<String>, LoomError> {
    Ok(certificate_bundle_served_listener_reference_map(store)?
        .remove(name)
        .unwrap_or_default())
}

fn certificate_denied_remove_target(name: &str, references: &[String]) -> String {
    denied_remove_target(&certificate_bundle_target(name), references)
}

fn denied_remove_target(prefix: &str, references: &[String]) -> String {
    let mut target = prefix.to_string();
    target.push_str(";served_listener_count=");
    target.push_str(&references.len().to_string());
    target.push_str(";served_listeners=");
    for (idx, reference) in references.iter().enumerate() {
        if idx > 0 {
            target.push(',');
        }
        if target.len() + reference.len() > 900 {
            target.push_str(";truncated=true");
            break;
        }
        target.push_str(reference);
    }
    target
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_certificate_material(
    server_cert_chain_pem: &[u8],
    private_key_pem: &[u8],
    trust_bundle_pem: Option<&[u8]>,
) -> Result<(), LoomError> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let certs: Vec<_> = rustls::pki_types::CertificateDer::pem_slice_iter(server_cert_chain_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| LoomError::invalid(format!("invalid certificate chain PEM: {err}")))?;
    if certs.is_empty() {
        return Err(LoomError::invalid(
            "invalid certificate chain: no CERTIFICATE PEM block found",
        ));
    }
    if let Some(trust) = trust_bundle_pem {
        let trust_certs: Vec<_> = rustls::pki_types::CertificateDer::pem_slice_iter(trust)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| LoomError::invalid(format!("invalid trust bundle PEM: {err}")))?;
        if trust_certs.is_empty() {
            return Err(LoomError::invalid(
                "invalid trust bundle: no CERTIFICATE PEM block found",
            ));
        }
    }
    let private_key = rustls::pki_types::PrivateKeyDer::from_pem_slice(private_key_pem)
        .map_err(|err| LoomError::invalid(format!("invalid private key PEM: {err}")))?;
    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, private_key)
        .map(|_| ())
        .map_err(|err| {
            LoomError::invalid(format!(
                "certificate chain and private key do not match: {err}"
            ))
        })
}

#[cfg(target_arch = "wasm32")]
fn validate_certificate_material(
    server_cert_chain_pem: &[u8],
    private_key_pem: &[u8],
    trust_bundle_pem: Option<&[u8]>,
) -> Result<(), LoomError> {
    if !server_cert_chain_pem
        .windows(b"-----BEGIN CERTIFICATE-----".len())
        .any(|window| window == b"-----BEGIN CERTIFICATE-----")
    {
        return Err(LoomError::invalid(
            "invalid certificate chain: no CERTIFICATE PEM block found",
        ));
    }
    if !private_key_pem
        .windows(b"-----BEGIN ".len())
        .any(|window| window == b"-----BEGIN ")
    {
        return Err(LoomError::invalid("invalid private key PEM"));
    }
    if let Some(trust) = trust_bundle_pem {
        if !trust
            .windows(b"-----BEGIN CERTIFICATE-----".len())
            .any(|window| window == b"-----BEGIN CERTIFICATE-----")
        {
            return Err(LoomError::invalid(
                "invalid trust bundle: no CERTIFICATE PEM block found",
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn certificate_san_names(
    dns_names: &[String],
    ip_addresses: &[String],
    cn: Option<&str>,
) -> Result<Vec<String>, LoomError> {
    let mut names = Vec::new();
    names.extend(dns_names.iter().cloned());
    names.extend(ip_addresses.iter().cloned());
    if names.is_empty() {
        let cn = cn.ok_or_else(|| {
            LoomError::invalid("provide at least one dns name, IP address, or common name")
        })?;
        names.push(cn.to_string());
    }
    Ok(names)
}

#[cfg(not(target_arch = "wasm32"))]
fn certificate_algorithm(name: &str) -> Result<&'static rcgen::SignatureAlgorithm, LoomError> {
    match name {
        "p256" | "ecdsa-p256" | "ecdsa-p256-sha256" => Ok(&rcgen::PKCS_ECDSA_P256_SHA256),
        "p384" | "ecdsa-p384" | "ecdsa-p384-sha384" => Ok(&rcgen::PKCS_ECDSA_P384_SHA384),
        "ed25519" => Ok(&rcgen::PKCS_ED25519),
        _ => Err(LoomError::invalid(
            "unsupported certificate algorithm; use p256, p384, or ed25519",
        )),
    }
}

fn network_access_policy_target(name: &str) -> String {
    format!("name={name}")
}

fn network_access_served_listener_reference_map(
    store: &FileStore,
) -> Result<BTreeMap<String, Vec<String>>, LoomError> {
    let mut references = BTreeMap::<String, Vec<String>>::new();
    for record in store.served_listeners()? {
        if let Some(name) = record.network_access_policy_ref.as_deref() {
            references
                .entry(name.to_string())
                .or_default()
                .push(record.id);
        }
    }
    Ok(references)
}

fn network_access_references_for<'a>(
    references: &'a BTreeMap<String, Vec<String>>,
    name: &str,
) -> &'a [String] {
    references.get(name).map(Vec::as_slice).unwrap_or(&[])
}

fn network_access_served_listener_references(
    store: &FileStore,
    name: &str,
) -> Result<Vec<String>, LoomError> {
    Ok(network_access_served_listener_reference_map(store)?
        .remove(name)
        .unwrap_or_default())
}

fn network_access_denied_remove_target(name: &str, references: &[String]) -> String {
    denied_remove_target(&network_access_policy_target(name), references)
}

fn network_access_policy_json(
    store: &FileStore,
    policy: &loom_store::NetworkAccessPolicyRecord,
    seq: u64,
    references: &[String],
) -> Result<String, LoomError> {
    Ok(format!(
        "{{\"seq\":{},{}",
        seq,
        &network_access_policy_record_json(store, policy, references)?[1..]
    ))
}

fn network_access_policy_record_json(
    store: &FileStore,
    policy: &loom_store::NetworkAccessPolicyRecord,
    references: &[String],
) -> Result<String, LoomError> {
    let digest = store.network_access_policy_digest(policy)?;
    let mut out = String::new();
    out.push('{');
    out.push_str("\"name\":");
    out.push_str(&json_quote(&policy.name));
    out.push_str(",\"schema_version\":");
    out.push_str(&policy.schema_version.to_string());
    out.push_str(",\"digest\":");
    out.push_str(&json_quote(&digest.to_string()));
    out.push_str(",\"description\":");
    push_json_option(&mut out, policy.description.as_deref());
    out.push_str(",\"default_action\":");
    out.push_str(&json_quote(policy.default_action.as_str()));
    out.push_str(",\"created_audit_seq\":");
    push_json_u64_option(&mut out, policy.created_audit_seq);
    out.push_str(",\"updated_audit_seq\":");
    push_json_u64_option(&mut out, policy.updated_audit_seq);
    out.push_str(",\"references\":[");
    for (idx, reference) in references.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_quote(reference));
    }
    out.push_str("],\"rules\":[");
    for (idx, rule) in policy.rules.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&network_access_rule_json(rule));
    }
    out.push_str("]}");
    Ok(out)
}

fn network_access_rule_json(rule: &loom_store::NetworkAccessRule) -> String {
    let source_cidr = rule.source_cidr.map(|cidr| cidr.to_string());
    let trusted_proxy_cidr = rule.trusted_proxy_cidr.map(|cidr| cidr.to_string());
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_quote(&rule.id));
    out.push_str(",\"action\":");
    out.push_str(&json_quote(rule.action.as_str()));
    out.push_str(",\"source_cidr\":");
    push_json_option(&mut out, source_cidr.as_deref());
    out.push_str(",\"trusted_proxy_cidr\":");
    push_json_option(&mut out, trusted_proxy_cidr.as_deref());
    out.push_str(",\"require_mtls\":");
    out.push_str(if rule.require_mtls { "true" } else { "false" });
    out.push_str(",\"client_cert_subject\":");
    push_json_option(&mut out, rule.client_cert_subject.as_deref());
    out.push_str(",\"client_cert_san\":");
    push_json_option(&mut out, rule.client_cert_san.as_deref());
    out.push_str(",\"client_cert_issuer\":");
    push_json_option(&mut out, rule.client_cert_issuer.as_deref());
    out.push_str(",\"description\":");
    push_json_option(&mut out, rule.description.as_deref());
    out.push('}');
    out
}

fn network_access_rules_from_json(
    rules_json: &str,
) -> Result<Vec<loom_store::NetworkAccessRule>, LoomError> {
    let value: serde_json::Value = serde_json::from_str(rules_json)
        .map_err(|err| LoomError::invalid(format!("network access rules JSON: {err}")))?;
    let rules = value
        .as_array()
        .ok_or_else(|| LoomError::invalid("network access rules JSON must be an array"))?;
    rules
        .iter()
        .enumerate()
        .map(|(idx, value)| network_access_rule_from_json(idx, value))
        .collect()
}

fn network_access_rule_from_json(
    idx: usize,
    value: &serde_json::Value,
) -> Result<loom_store::NetworkAccessRule, LoomError> {
    let object = value
        .as_object()
        .ok_or_else(|| LoomError::invalid(format!("rule {idx} must be an object")))?;
    let id = json_string_field(object, "id")?.unwrap_or_else(|| format!("rule-{}", idx + 1));
    let action = json_string_field(object, "action")?
        .ok_or_else(|| LoomError::invalid(format!("rule {idx} missing action")))
        .and_then(|value| loom_store::NetworkAccessAction::parse(&value))?;
    let source_cidr = json_string_field(object, "source_cidr")?
        .map(|value| loom_store::NetworkAccessCidr::parse(&value))
        .transpose()?;
    let trusted_proxy_cidr = json_string_field(object, "trusted_proxy_cidr")?
        .map(|value| loom_store::NetworkAccessCidr::parse(&value))
        .transpose()?;
    Ok(loom_store::NetworkAccessRule {
        id,
        action,
        source_cidr,
        trusted_proxy_cidr,
        require_mtls: json_bool_field(object, "require_mtls")?.unwrap_or(false),
        client_cert_subject: json_string_field(object, "client_cert_subject")?,
        client_cert_san: json_string_field(object, "client_cert_san")?,
        client_cert_issuer: json_string_field(object, "client_cert_issuer")?,
        description: json_string_field(object, "description")?,
    })
}

fn json_string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<String>, LoomError> {
    match object.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(LoomError::invalid(format!(
            "rule field {key:?} must be a string or null"
        ))),
    }
}

fn json_bool_field(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<bool>, LoomError> {
    match object.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(LoomError::invalid(format!(
            "rule field {key:?} must be a boolean or null"
        ))),
    }
}

pub struct SecurityAdminService;

impl SecurityAdminService {
    pub fn audit_config_show_json(loom: &mut Loom<FileStore>) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let config = loom.store().audit_config()?;
        loom.store()
            .audit_append(actor, "audit.config.show", None)?;
        Ok(audit_config_json(config))
    }

    pub fn audit_config_set_json(
        loom: &mut Loom<FileStore>,
        retention_days: Option<u32>,
        legal_hold: Option<bool>,
    ) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let mut config = loom.store().audit_config()?;
        if let Some(value) = retention_days {
            config.retention_days = value;
        }
        if let Some(value) = legal_hold {
            config.legal_hold = value;
        }
        let target = format!(
            "retention_days={};legal_hold={}",
            config.retention_days, config.legal_hold
        );
        let seq = loom.store().save_audit_config_audited(
            config,
            actor,
            "audit.config.set",
            Some(&target),
        )?;
        Ok(format!(
            "{{\"seq\":{},\"config\":{}}}",
            seq,
            audit_config_json(config)
        ))
    }

    pub fn audit_list_json(loom: &mut Loom<FileStore>) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let records = loom.store().audit_records()?;
        let seq = loom.store().audit_append(actor, "audit.list", None)?;
        let records_json = records
            .iter()
            .map(audit_record_json)
            .collect::<Vec<_>>()
            .join(",");
        Ok(format!("{{\"seq\":{seq},\"records\":[{records_json}]}}"))
    }

    pub fn audit_view_json(loom: &mut Loom<FileStore>, record: &str) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let records = loom.store().audit_records()?;
        let found = find_audit_record(&records, record)?;
        let target = format!("seq={}", found.seq);
        let seq = loom
            .store()
            .audit_append(actor, "audit.view", Some(&target))?;
        Ok(format!(
            "{{\"seq\":{seq},\"record\":{}}}",
            audit_record_json(found)
        ))
    }

    pub fn certificate_list_json(loom: &mut Loom<FileStore>) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let bundles = loom.store().certificate_bundles()?;
        let references = certificate_bundle_served_listener_reference_map(loom.store())?;
        let seq =
            loom.store()
                .audit_append(actor, "certificate.bundle.list", Some("certificates"))?;
        let certificates = bundles
            .iter()
            .map(|bundle| {
                certificate_bundle_record_json(
                    bundle,
                    certificate_references_for(&references, &bundle.name),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        Ok(format!(
            "{{\"seq\":{seq},\"certificates\":[{certificates}]}}"
        ))
    }

    pub fn certificate_import_json(
        loom: &mut Loom<FileStore>,
        name: &str,
        cert_chain_pem: Vec<u8>,
        private_key_pem: Vec<u8>,
        trust_bundle_pem: Option<Vec<u8>>,
        force: bool,
    ) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        validate_certificate_material(
            &cert_chain_pem,
            &private_key_pem,
            trust_bundle_pem.as_deref(),
        )?;
        let mut record = loom.store().certificate_bundle_record(
            name,
            cert_chain_pem,
            private_key_pem,
            trust_bundle_pem,
        )?;
        let action = if force {
            "certificate.bundle.import.force"
        } else {
            "certificate.bundle.import"
        };
        let target = certificate_bundle_target(name);
        let seq = loom.store().save_certificate_bundle_audited(
            &record,
            actor,
            action,
            Some(&target),
            force,
        )?;
        record.created_audit_seq = record.created_audit_seq.or(Some(seq));
        record.updated_audit_seq = Some(seq);
        record.unencrypted_private_key_override = !loom.store().is_encrypted() && force;
        Ok(certificate_bundle_json(&record, seq, &[]))
    }

    pub fn certificate_export(
        loom: &mut Loom<FileStore>,
        name: &str,
        include_cert_chain: bool,
        include_private_key: bool,
        include_trust_bundle: bool,
        force: bool,
    ) -> Result<Vec<u8>, LoomError> {
        if !include_cert_chain && !include_private_key && !include_trust_bundle {
            return Err(LoomError::invalid(
                "select at least one certificate export payload",
            ));
        }
        if include_private_key && !force {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "exporting private keys requires force",
            ));
        }
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let record = loom
            .store()
            .certificate_bundle(name)?
            .ok_or_else(|| LoomError::not_found("certificate bundle not found"))?;
        let target = certificate_bundle_target(name);
        let action = if include_private_key {
            "certificate.bundle.export_private_key"
        } else {
            "certificate.bundle.export"
        };
        let seq = loom.store().audit_append(actor, action, Some(&target))?;
        let trust =
            if include_trust_bundle {
                Some(record.trust_bundle_pem.clone().ok_or_else(|| {
                    LoomError::not_found("certificate bundle has no trust bundle")
                })?)
            } else {
                None
            };
        let value = loom_codec::Value::Array(vec![
            loom_codec::Value::Uint(seq),
            loom_codec::Value::Text(name.to_string()),
            optional_bytes_value(include_cert_chain.then_some(record.server_cert_chain_pem)),
            optional_bytes_value(include_private_key.then_some(record.private_key_pem)),
            optional_bytes_value(trust),
        ]);
        loom_codec::encode(&value).map_err(|err| {
            LoomError::new(
                Code::CorruptObject,
                format!("certificate export encode: {err}"),
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn certificate_generate_self_signed_json(
        loom: &mut Loom<FileStore>,
        name: &str,
        dns_names: Vec<String>,
        ip_addresses: Vec<String>,
        cn: Option<&str>,
        days: u32,
        algorithm: &str,
        force: bool,
    ) -> Result<String, LoomError> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (name, dns_names, ip_addresses, cn, days, algorithm, force);
            loom.authorize_global_admin()?;
            return Err(LoomError::new(
                Code::Unsupported,
                "self-signed certificate generation is unavailable in WASM",
            ));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            loom.authorize_global_admin()?;
            let actor = loom.effective_principal()?;
            if days == 0 {
                return Err(LoomError::invalid("days must be greater than zero"));
            }
            let san_names = certificate_san_names(&dns_names, &ip_addresses, cn)?;
            let mut params = rcgen::CertificateParams::new(san_names)
                .map_err(|err| LoomError::invalid(format!("certificate parameters: {err}")))?;
            if let Some(cn) = cn {
                let mut dn = rcgen::DistinguishedName::new();
                dn.push(rcgen::DnType::CommonName, cn);
                params.distinguished_name = dn;
            }
            let now = time::OffsetDateTime::now_utc();
            params.not_before = now;
            params.not_after = now
                .checked_add(time::Duration::days(i64::from(days)))
                .ok_or_else(|| LoomError::invalid("days is too large"))?;
            let key_pair = rcgen::KeyPair::generate_for(certificate_algorithm(algorithm)?)
                .map_err(|err| {
                    LoomError::new(Code::Internal, format!("generate key pair: {err}"))
                })?;
            let cert = params.self_signed(&key_pair).map_err(|err| {
                LoomError::new(Code::Internal, format!("generate certificate: {err}"))
            })?;
            let cert_chain_pem = cert.pem().into_bytes();
            let private_key_pem = key_pair.serialize_pem().into_bytes();
            validate_certificate_material(&cert_chain_pem, &private_key_pem, None)?;
            let mut record = loom.store().certificate_bundle_record(
                name,
                cert_chain_pem,
                private_key_pem,
                None,
            )?;
            let action = if force {
                "certificate.bundle.generate_self_signed.force"
            } else {
                "certificate.bundle.generate_self_signed"
            };
            let target = certificate_bundle_target(name);
            let seq = loom.store().save_certificate_bundle_audited(
                &record,
                actor,
                action,
                Some(&target),
                force,
            )?;
            record.created_audit_seq = record.created_audit_seq.or(Some(seq));
            record.updated_audit_seq = Some(seq);
            record.unencrypted_private_key_override = !loom.store().is_encrypted() && force;
            Ok(certificate_bundle_json(&record, seq, &[]))
        }
    }

    pub fn certificate_remove_json(
        loom: &mut Loom<FileStore>,
        name: &str,
    ) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let references = certificate_bundle_served_listener_references(loom.store(), name)?;
        let target = certificate_bundle_target(name);
        if !references.is_empty() {
            let denied_target = certificate_denied_remove_target(name, &references);
            loom.store().audit_append(
                actor,
                "certificate.bundle.remove.denied",
                Some(&denied_target),
            )?;
            return Err(LoomError::new(
                Code::PermissionDenied,
                format!(
                    "certificate bundle {name:?} is referenced by served listeners: {}",
                    references.join(", ")
                ),
            ));
        }
        let seq = loom.store().remove_certificate_bundle_audited(
            name,
            actor,
            "certificate.bundle.remove",
            Some(&target),
        )?;
        Ok(format!("{{\"seq\":{seq},\"name\":{}}}", json_quote(name)))
    }

    pub fn certificate_audit_json(
        loom: &mut Loom<FileStore>,
        name: &str,
    ) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let record = loom
            .store()
            .certificate_bundle(name)?
            .ok_or_else(|| LoomError::not_found("certificate bundle not found"))?;
        let references = certificate_bundle_served_listener_references(loom.store(), name)?;
        let target = certificate_bundle_target(name);
        let seq = loom
            .store()
            .audit_append(actor, "certificate.bundle.audit", Some(&target))?;
        Ok(certificate_bundle_json(&record, seq, &references))
    }

    pub fn network_access_list_json(loom: &mut Loom<FileStore>) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let policies = loom.store().network_access_policies()?;
        let references = network_access_served_listener_reference_map(loom.store())?;
        let seq = loom.store().audit_append(
            actor,
            "network-access.policy.list",
            Some("network-access"),
        )?;
        let policies = policies
            .iter()
            .map(|policy| {
                network_access_policy_record_json(
                    loom.store(),
                    policy,
                    network_access_references_for(&references, &policy.name),
                )
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(",");
        Ok(format!("{{\"seq\":{seq},\"policies\":[{policies}]}}"))
    }

    pub fn network_access_set_json(
        loom: &mut Loom<FileStore>,
        name: &str,
        description: Option<&str>,
        default_action: &str,
        rules_json: &str,
    ) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let default_action = loom_store::NetworkAccessAction::parse(default_action)?;
        let rules = network_access_rules_from_json(rules_json)?;
        let mut policy = FileStore::network_access_policy_record(
            name,
            description.map(str::to_string),
            default_action,
            rules,
        )?;
        let target = network_access_policy_target(name);
        let seq = loom.store().save_network_access_policy_audited(
            &policy,
            actor,
            "network-access.policy.set",
            Some(&target),
        )?;
        policy.created_audit_seq = policy.created_audit_seq.or(Some(seq));
        policy.updated_audit_seq = Some(seq);
        network_access_policy_json(loom.store(), &policy, seq, &[])
    }

    pub fn network_access_remove_json(
        loom: &mut Loom<FileStore>,
        name: &str,
    ) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let references = network_access_served_listener_references(loom.store(), name)?;
        let target = network_access_policy_target(name);
        if !references.is_empty() {
            let denied_target = network_access_denied_remove_target(name, &references);
            loom.store().audit_append(
                actor,
                "network-access.policy.remove.denied",
                Some(&denied_target),
            )?;
            return Err(LoomError::new(
                Code::PermissionDenied,
                format!(
                    "network access policy {name:?} is referenced by served listeners: {}",
                    references.join(", ")
                ),
            ));
        }
        let seq = loom.store().remove_network_access_policy_audited(
            name,
            actor,
            "network-access.policy.remove",
            Some(&target),
        )?;
        Ok(format!("{{\"seq\":{seq},\"name\":{}}}", json_quote(name)))
    }

    pub fn network_access_audit_json(
        loom: &mut Loom<FileStore>,
        name: &str,
    ) -> Result<String, LoomError> {
        loom.authorize_global_admin()?;
        let actor = loom.effective_principal()?;
        let policy = loom
            .store()
            .network_access_policy(name)?
            .ok_or_else(|| LoomError::not_found("network access policy not found"))?;
        let references = network_access_served_listener_references(loom.store(), name)?;
        let target = network_access_policy_target(name);
        let seq = loom
            .store()
            .audit_append(actor, "network-access.policy.audit", Some(&target))?;
        network_access_policy_json(loom.store(), &policy, seq, &references)
    }
}
