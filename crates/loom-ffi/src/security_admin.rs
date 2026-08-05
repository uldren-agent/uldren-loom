//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use rustls::pki_types::pem::PemObject as _;

fn json_opt(out: &mut String, value: Option<&str>) {
    match value {
        Some(value) => out.push_str(&json_string(value)),
        None => out.push_str("null"),
    }
}

fn audit_json(record: &loom_store::AuditRecord) -> String {
    let mut out = String::new();
    out.push_str("{\"seq\":");
    out.push_str(&record.seq.to_string());
    out.push_str(",\"hash\":");
    out.push_str(&json_string(&record.hash.to_string()));
    out.push_str(",\"principal\":");
    match record.principal {
        Some(principal) => out.push_str(&json_string(&principal.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"action\":");
    out.push_str(&json_string(&record.action));
    out.push_str(",\"target\":");
    json_opt(&mut out, record.target.as_deref());
    out.push_str(",\"prev_hash\":");
    match record.prev_hash {
        Some(hash) => out.push_str(&json_string(&hash.to_string())),
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

fn certificate_json(record: &loom_store::CertificateBundleRecord, seq: u64) -> String {
    let mut out = String::new();
    out.push_str("{\"seq\":");
    out.push_str(&seq.to_string());
    out.push_str(",\"name\":");
    out.push_str(&json_string(&record.name));
    out.push_str(",\"server_cert_chain_digest\":");
    out.push_str(&json_string(&record.server_cert_chain_digest.to_string()));
    out.push_str(",\"private_key_digest\":");
    out.push_str(&json_string(&record.private_key_digest.to_string()));
    out.push_str(",\"unencrypted_private_key_override\":");
    out.push_str(if record.unencrypted_private_key_override {
        "true"
    } else {
        "false"
    });
    out.push('}');
    out
}

fn validate_certificate_material(
    server_cert_chain_pem: &[u8],
    private_key_pem: &[u8],
    trust_bundle_pem: Option<&[u8]>,
) -> LoomResult<()> {
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

fn certificate_algorithm(name: &str) -> LoomResult<&'static rcgen::SignatureAlgorithm> {
    match name {
        "p256" | "ecdsa-p256" | "ecdsa-p256-sha256" => Ok(&rcgen::PKCS_ECDSA_P256_SHA256),
        "p384" | "ecdsa-p384" | "ecdsa-p384-sha384" => Ok(&rcgen::PKCS_ECDSA_P384_SHA384),
        "ed25519" => Ok(&rcgen::PKCS_ED25519),
        _ => Err(LoomError::invalid(
            "unsupported certificate algorithm; use p256, p384, or ed25519",
        )),
    }
}

fn certificate_san_names(
    dns_names_json: &str,
    ip_addresses_json: &str,
    cn: Option<&str>,
) -> LoomResult<Vec<String>> {
    let mut names = Vec::new();
    for value in [dns_names_json, ip_addresses_json] {
        let parsed: serde_json::Value = serde_json::from_str(value)
            .map_err(|err| LoomError::invalid(format!("certificate SAN JSON: {err}")))?;
        let array = parsed
            .as_array()
            .ok_or_else(|| LoomError::invalid("certificate SAN JSON must be an array"))?;
        for item in array {
            let text = item
                .as_str()
                .ok_or_else(|| LoomError::invalid("certificate SAN entry must be a string"))?;
            names.push(text.to_string());
        }
    }
    if names.is_empty() {
        let cn = cn.ok_or_else(|| {
            LoomError::invalid("provide at least one dns name, IP address, or common name")
        })?;
        names.push(cn.to_string());
    }
    Ok(names)
}

fn network_rules_from_json(rules_json: &str) -> LoomResult<Vec<loom_store::NetworkAccessRule>> {
    let value: serde_json::Value = serde_json::from_str(rules_json)
        .map_err(|err| LoomError::invalid(format!("network access rules JSON: {err}")))?;
    let rules = value
        .as_array()
        .ok_or_else(|| LoomError::invalid("network access rules JSON must be an array"))?;
    rules
        .iter()
        .enumerate()
        .map(|(idx, value)| {
            let object = value
                .as_object()
                .ok_or_else(|| LoomError::invalid(format!("rule {idx} must be an object")))?;
            let id = object
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("rule-{}", idx + 1));
            let action = object
                .get("action")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| LoomError::invalid(format!("rule {idx} missing action")))
                .and_then(loom_store::NetworkAccessAction::parse)?;
            let source_cidr = object
                .get("source_cidr")
                .and_then(serde_json::Value::as_str)
                .map(loom_store::NetworkAccessCidr::parse)
                .transpose()?;
            Ok(loom_store::NetworkAccessRule {
                id,
                action,
                source_cidr,
                trusted_proxy_cidr: None,
                require_mtls: object
                    .get("require_mtls")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                client_cert_subject: None,
                client_cert_san: None,
                client_cert_issuer: None,
                description: object
                    .get("description")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

fn policy_json(
    store: &FileStore,
    policy: &loom_store::NetworkAccessPolicyRecord,
    seq: u64,
) -> LoomResult<String> {
    let digest = store.network_access_policy_digest(policy)?;
    let mut out = String::new();
    out.push_str("{\"seq\":");
    out.push_str(&seq.to_string());
    out.push_str(",\"name\":");
    out.push_str(&json_string(&policy.name));
    out.push_str(",\"digest\":");
    out.push_str(&json_string(&digest.to_string()));
    out.push_str(",\"default_action\":");
    out.push_str(&json_string(policy.default_action.as_str()));
    out.push('}');
    Ok(out)
}

unsafe fn optional_bytes<'a>(ptr: *const c_uchar, len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { core::slice::from_raw_parts(ptr, len) })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_audit_config_show_json(
    handle: *mut LoomSession,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_audit_config_show_json");
    let result = (|| -> LoomResult<String> {
        let loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let seq = loom
            .store()
            .audit_append(Some(actor), "audit.config.show", None)?;
        let config = loom.store().audit_config()?;
        Ok(format!(
            "{{\"seq\":{seq},\"retention_days\":{},\"legal_hold\":{}}}",
            config.retention_days,
            if config.legal_hold { "true" } else { "false" }
        ))
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_audit_config_set_json(
    handle: *mut LoomSession,
    retention_days: u32,
    has_retention_days: i32,
    legal_hold: i32,
    has_legal_hold: i32,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_audit_config_set_json");
    let result = (|| -> LoomResult<String> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let mut config = loom.store().audit_config()?;
        if has_retention_days != 0 {
            config.retention_days = retention_days;
        }
        if has_legal_hold != 0 {
            config.legal_hold = legal_hold != 0;
        }
        let seq = loom.store().save_audit_config_audited(
            config,
            Some(actor),
            "audit.config.set",
            Some("audit.config"),
        )?;
        save_loom(&mut loom)?;
        Ok(format!(
            "{{\"seq\":{seq},\"config\":{{\"retention_days\":{},\"legal_hold\":{}}}}}",
            config.retention_days,
            if config.legal_hold { "true" } else { "false" }
        ))
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_audit_list_json(
    handle: *mut LoomSession,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_audit_list_json");
    let result = (|| -> LoomResult<String> {
        let loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        loom.store().audit_append(Some(actor), "audit.list", None)?;
        let records = loom.store().audit_records()?;
        let body = records.iter().map(audit_json).collect::<Vec<_>>().join(",");
        Ok(format!("{{\"records\":[{body}]}}"))
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_audit_view_json(
    handle: *mut LoomSession,
    record: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_audit_view_json");
    let record = arg_str!(record, "loom_audit_view_json");
    let result = (|| -> LoomResult<String> {
        let loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let records = loom.store().audit_records()?;
        let found = if let Ok(seq) = record.parse::<u64>() {
            records.iter().find(|entry| entry.seq == seq)
        } else {
            let digest = Digest::parse(record)?;
            records.iter().find(|entry| entry.hash == digest)
        }
        .ok_or_else(|| LoomError::not_found("audit record not found"))?;
        loom.store()
            .audit_append(Some(actor), "audit.view", Some(record))?;
        Ok(audit_json(found))
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_certificate_list_json(
    handle: *mut LoomSession,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_certificate_list_json");
    let result = (|| -> LoomResult<String> {
        let loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let seq = loom.store().audit_append(
            Some(actor),
            "certificate.bundle.list",
            Some("certificates"),
        )?;
        let body = loom
            .store()
            .certificate_bundles()?
            .iter()
            .map(|record| certificate_json(record, seq))
            .collect::<Vec<_>>()
            .join(",");
        Ok(format!("{{\"seq\":{seq},\"certificates\":[{body}]}}"))
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_certificate_import_json(
    handle: *mut LoomSession,
    name: *const c_char,
    cert_chain_pem: *const c_uchar,
    cert_chain_len: usize,
    private_key_pem: *const c_uchar,
    private_key_len: usize,
    trust_bundle_pem: *const c_uchar,
    trust_bundle_len: usize,
    force: i32,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_certificate_import_json");
    let name = arg_str!(name, "loom_certificate_import_json");
    let cert_chain = unsafe { byte_slice(cert_chain_pem, cert_chain_len) };
    let private_key = unsafe { byte_slice(private_key_pem, private_key_len) };
    let trust = unsafe { optional_bytes(trust_bundle_pem, trust_bundle_len) };
    let result = (|| -> LoomResult<String> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        validate_certificate_material(cert_chain, private_key, trust)?;
        let mut record = loom.store().certificate_bundle_record(
            name,
            cert_chain.to_vec(),
            private_key.to_vec(),
            trust.map(<[u8]>::to_vec),
        )?;
        let action = if force != 0 {
            "certificate.bundle.import.force"
        } else {
            "certificate.bundle.import"
        };
        let seq = loom.store().save_certificate_bundle_audited(
            &record,
            Some(actor),
            action,
            Some(&format!("name={name}")),
            force != 0,
        )?;
        save_loom(&mut loom)?;
        record.updated_audit_seq = Some(seq);
        Ok(certificate_json(&record, seq))
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_certificate_export(
    handle: *mut LoomSession,
    name: *const c_char,
    include_cert_chain: i32,
    include_private_key: i32,
    include_trust_bundle: i32,
    force: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_certificate_export");
    let name = arg_str!(name, "loom_certificate_export");
    let result = (|| -> LoomResult<Vec<u8>> {
        if include_private_key != 0 && force == 0 {
            return Err(LoomError::new(
                Code::PermissionDenied,
                "exporting private keys requires force",
            ));
        }
        let loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let record = loom
            .store()
            .certificate_bundle(name)?
            .ok_or_else(|| LoomError::not_found("certificate bundle not found"))?;
        let seq = loom.store().audit_append(
            Some(actor),
            if include_private_key != 0 {
                "certificate.bundle.export_private_key"
            } else {
                "certificate.bundle.export"
            },
            Some(&format!("name={name}")),
        )?;
        let trust =
            if include_trust_bundle != 0 {
                Some(record.trust_bundle_pem.clone().ok_or_else(|| {
                    LoomError::not_found("certificate bundle has no trust bundle")
                })?)
            } else {
                None
            };
        let value = CborValue::Array(vec![
            CborValue::Uint(seq),
            CborValue::Text(name.to_string()),
            if include_cert_chain != 0 {
                CborValue::Bytes(record.server_cert_chain_pem)
            } else {
                CborValue::Null
            },
            if include_private_key != 0 {
                CborValue::Bytes(record.private_key_pem)
            } else {
                CborValue::Null
            },
            trust.map_or(CborValue::Null, CborValue::Bytes),
        ]);
        cbor_encode(&value).map_err(|err| LoomError::new(Code::CorruptObject, err.to_string()))
    })();
    match result {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_certificate_generate_self_signed_json(
    handle: *mut LoomSession,
    name: *const c_char,
    dns_names_json: *const c_char,
    ip_addresses_json: *const c_char,
    cn: *const c_char,
    days: u32,
    algorithm: *const c_char,
    force: i32,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_certificate_generate_self_signed_json");
    let name = arg_str!(name, "loom_certificate_generate_self_signed_json");
    let dns = arg_str!(dns_names_json, "loom_certificate_generate_self_signed_json");
    let ips = arg_str!(
        ip_addresses_json,
        "loom_certificate_generate_self_signed_json"
    );
    let algorithm = arg_str!(algorithm, "loom_certificate_generate_self_signed_json");
    let cn = unsafe { cstr(cn) };
    let result = (|| -> LoomResult<String> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        if days == 0 {
            return Err(LoomError::invalid("days must be greater than zero"));
        }
        let san_names = certificate_san_names(dns, ips, cn)?;
        let mut params = rcgen::CertificateParams::new(san_names)
            .map_err(|err| LoomError::invalid(format!("certificate parameters: {err}")))?;
        let now = time::OffsetDateTime::now_utc();
        params.not_before = now;
        params.not_after = now
            .checked_add(time::Duration::days(i64::from(days)))
            .ok_or_else(|| LoomError::invalid("days is too large"))?;
        let key_pair = rcgen::KeyPair::generate_for(certificate_algorithm(algorithm)?)
            .map_err(|err| LoomError::new(Code::Internal, format!("generate key pair: {err}")))?;
        let cert = params.self_signed(&key_pair).map_err(|err| {
            LoomError::new(Code::Internal, format!("generate certificate: {err}"))
        })?;
        let cert_chain = cert.pem().into_bytes();
        let private_key = key_pair.serialize_pem().into_bytes();
        validate_certificate_material(&cert_chain, &private_key, None)?;
        let mut record =
            loom.store()
                .certificate_bundle_record(name, cert_chain, private_key, None)?;
        let action = if force != 0 {
            "certificate.bundle.generate_self_signed.force"
        } else {
            "certificate.bundle.generate_self_signed"
        };
        let seq = loom.store().save_certificate_bundle_audited(
            &record,
            Some(actor),
            action,
            Some(&format!("name={name}")),
            force != 0,
        )?;
        save_loom(&mut loom)?;
        record.updated_audit_seq = Some(seq);
        Ok(certificate_json(&record, seq))
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_certificate_remove_json(
    handle: *mut LoomSession,
    name: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_certificate_remove_json");
    let name = arg_str!(name, "loom_certificate_remove_json");
    let result = (|| -> LoomResult<String> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let references = loom
            .store()
            .served_listeners()?
            .into_iter()
            .filter(|record| record.tls.certificate_bundle_ref.as_deref() == Some(name))
            .map(|record| record.id)
            .collect::<Vec<_>>();
        if !references.is_empty() {
            loom.store().audit_append(
                Some(actor),
                "certificate.bundle.remove.denied",
                Some(&format!(
                    "name={name};served_listener_count={}",
                    references.len()
                )),
            )?;
            return Err(LoomError::new(
                Code::PermissionDenied,
                "certificate bundle is referenced by served listeners",
            ));
        }
        let seq = loom.store().remove_certificate_bundle_audited(
            name,
            Some(actor),
            "certificate.bundle.remove",
            Some(&format!("name={name}")),
        )?;
        save_loom(&mut loom)?;
        Ok(format!("{{\"seq\":{seq},\"name\":{}}}", json_string(name)))
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_certificate_audit_json(
    handle: *mut LoomSession,
    name: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_certificate_audit_json");
    let name = arg_str!(name, "loom_certificate_audit_json");
    let result = (|| -> LoomResult<String> {
        let loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let record = loom
            .store()
            .certificate_bundle(name)?
            .ok_or_else(|| LoomError::not_found("certificate bundle not found"))?;
        let seq = loom.store().audit_append(
            Some(actor),
            "certificate.bundle.audit",
            Some(&format!("name={name}")),
        )?;
        Ok(certificate_json(&record, seq))
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_network_access_list_json(
    handle: *mut LoomSession,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_network_access_list_json");
    let result = (|| -> LoomResult<String> {
        let loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let seq = loom.store().audit_append(
            Some(actor),
            "network-access.policy.list",
            Some("network-access"),
        )?;
        let body = loom
            .store()
            .network_access_policies()?
            .iter()
            .map(|policy| policy_json(loom.store(), policy, seq))
            .collect::<LoomResult<Vec<_>>>()?
            .join(",");
        Ok(format!("{{\"seq\":{seq},\"policies\":[{body}]}}"))
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_network_access_set_json(
    handle: *mut LoomSession,
    name: *const c_char,
    description: *const c_char,
    default_action: *const c_char,
    rules_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_network_access_set_json");
    let name = arg_str!(name, "loom_network_access_set_json");
    let default_action = arg_str!(default_action, "loom_network_access_set_json");
    let rules_json = arg_str!(rules_json, "loom_network_access_set_json");
    let description = unsafe { cstr(description) };
    let result = (|| -> LoomResult<String> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let default_action = loom_store::NetworkAccessAction::parse(default_action)?;
        let rules = network_rules_from_json(rules_json)?;
        let mut policy = FileStore::network_access_policy_record(
            name,
            description.map(str::to_string),
            default_action,
            rules,
        )?;
        let seq = loom.store().save_network_access_policy_audited(
            &policy,
            Some(actor),
            "network-access.policy.set",
            Some(&format!("name={name}")),
        )?;
        save_loom(&mut loom)?;
        policy.updated_audit_seq = Some(seq);
        policy_json(loom.store(), &policy, seq)
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_network_access_remove_json(
    handle: *mut LoomSession,
    name: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_network_access_remove_json");
    let name = arg_str!(name, "loom_network_access_remove_json");
    let result = (|| -> LoomResult<String> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let references = loom
            .store()
            .served_listeners()?
            .into_iter()
            .filter(|record| record.network_access_policy_ref.as_deref() == Some(name))
            .map(|record| record.id)
            .collect::<Vec<_>>();
        if !references.is_empty() {
            loom.store().audit_append(
                Some(actor),
                "network-access.policy.remove.denied",
                Some(&format!(
                    "name={name};served_listener_count={}",
                    references.len()
                )),
            )?;
            return Err(LoomError::new(
                Code::PermissionDenied,
                "network access policy is referenced by served listeners",
            ));
        }
        let seq = loom.store().remove_network_access_policy_audited(
            name,
            Some(actor),
            "network-access.policy.remove",
            Some(&format!("name={name}")),
        )?;
        save_loom(&mut loom)?;
        Ok(format!("{{\"seq\":{seq},\"name\":{}}}", json_string(name)))
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_network_access_audit_json(
    handle: *mut LoomSession,
    name: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_network_access_audit_json");
    let name = arg_str!(name, "loom_network_access_audit_json");
    let result = (|| -> LoomResult<String> {
        let loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let policy = loom
            .store()
            .network_access_policy(name)?
            .ok_or_else(|| LoomError::not_found("network access policy not found"))?;
        let seq = loom.store().audit_append(
            Some(actor),
            "network-access.policy.audit",
            Some(&format!("name={name}")),
        )?;
        policy_json(loom.store(), &policy, seq)
    })();
    match result {
        Ok(value) => unsafe { ok_str(out, &value) },
        Err(error) => fail(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        LoomSession, cstr, loom_close, loom_create, loom_last_error, loom_open, loom_string_free,
        to_c_string,
    };
    use std::ffi::{CStr, CString};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn cs(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    fn temp_loom() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "loom-security-admin-optional-{}-{}.loom",
            std::process::id(),
            NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn generated_certificate_material() -> (Vec<u8>, Vec<u8>) {
        let params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let cert = params.self_signed(&key_pair).unwrap();
        (
            cert.pem().into_bytes(),
            key_pair.serialize_pem().into_bytes(),
        )
    }

    unsafe fn ok_out(status: i32, out: *mut c_char) -> String {
        assert_eq!(status, 0, "last error: {:?}", last_error());
        assert!(!out.is_null());
        let s = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
        unsafe { loom_string_free(out) };
        s
    }

    fn last_error() -> Option<(i32, String)> {
        let mut code = 0;
        let mut message = core::ptr::null_mut();
        let mut len = 0usize;
        unsafe { loom_last_error(&mut code, &mut message, &mut len) };
        if message.is_null() {
            return None;
        }
        let text = unsafe { CStr::from_ptr(message) }
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(len, text.len());
        unsafe { loom_string_free(message) };
        Some((code, text))
    }

    #[test]
    fn c_abi_preserves_optional_absent_and_empty_values() {
        let dir = temp_loom();
        let path = cs(dir.to_str().unwrap());
        let dflt = cs("default");
        assert_eq!(
            unsafe {
                loom_create(
                    path.as_ptr(),
                    dflt.as_ptr(),
                    core::ptr::null(),
                    core::ptr::null(),
                    0,
                )
            },
            0
        );

        let mut handle: *mut LoomSession = core::ptr::null_mut();
        assert_eq!(unsafe { loom_open(path.as_ptr(), &mut handle) }, 0);

        let (cert, key) = generated_certificate_material();
        let cert_name = cs("no-trust");
        let mut cert_out = core::ptr::null_mut();
        let absent_trust_json = unsafe {
            ok_out(
                loom_certificate_import_json(
                    handle,
                    cert_name.as_ptr(),
                    cert.as_ptr(),
                    cert.len(),
                    key.as_ptr(),
                    key.len(),
                    core::ptr::null(),
                    0,
                    1,
                    &mut cert_out,
                ),
                cert_out,
            )
        };
        assert!(absent_trust_json.contains("\"name\":\"no-trust\""));
        {
            let store = FileStore::open(&dir).unwrap();
            let bundle = store
                .certificate_bundle("no-trust")
                .unwrap()
                .expect("no-trust bundle");
            assert!(bundle.trust_bundle_pem.is_none());
        }

        let empty_trust = [];
        let empty_cert_name = cs("empty-trust");
        let mut empty_out = core::ptr::null_mut();
        let empty_trust_status = unsafe {
            loom_certificate_import_json(
                handle,
                empty_cert_name.as_ptr(),
                cert.as_ptr(),
                cert.len(),
                key.as_ptr(),
                key.len(),
                empty_trust.as_ptr(),
                0,
                1,
                &mut empty_out,
            )
        };
        assert_eq!(empty_trust_status, Code::InvalidArgument.as_i32());
        assert!(empty_out.is_null());

        let null_cn = unsafe { cstr(core::ptr::null()) };
        assert_eq!(null_cn, None);
        let empty_cn = cs("");
        let present_empty_cn = unsafe { cstr(empty_cn.as_ptr()) };
        assert_eq!(present_empty_cn, Some(""));

        let rules = cs("[]");
        let allow = cs("allow");
        let absent_desc_name = cs("absent-desc");
        let mut policy_out = core::ptr::null_mut();
        unsafe {
            ok_out(
                loom_network_access_set_json(
                    handle,
                    absent_desc_name.as_ptr(),
                    core::ptr::null(),
                    allow.as_ptr(),
                    rules.as_ptr(),
                    &mut policy_out,
                ),
                policy_out,
            )
        };
        {
            let store = FileStore::open(&dir).unwrap();
            let policy = store
                .network_access_policy("absent-desc")
                .unwrap()
                .expect("absent-desc policy");
            assert_eq!(policy.description, None);
        }

        let empty_desc_name = cs("empty-desc");
        let empty_desc = cs("");
        let mut empty_policy_out = core::ptr::null_mut();
        unsafe {
            ok_out(
                loom_network_access_set_json(
                    handle,
                    empty_desc_name.as_ptr(),
                    empty_desc.as_ptr(),
                    allow.as_ptr(),
                    rules.as_ptr(),
                    &mut empty_policy_out,
                ),
                empty_policy_out,
            )
        };
        {
            let store = FileStore::open(&dir).unwrap();
            let policy = store
                .network_access_policy("empty-desc")
                .unwrap()
                .expect("empty-desc policy");
            assert_eq!(policy.description.as_deref(), Some(""));
        }

        unsafe { loom_close(handle) };
        let _ = std::fs::remove_file(dir);
    }

    #[test]
    fn optional_empty_bytes_are_present() {
        let empty = [];
        let value = unsafe { optional_bytes(empty.as_ptr(), 0) };
        assert_eq!(value, Some(&[][..]));
        let missing = unsafe { optional_bytes(core::ptr::null(), 0) };
        assert_eq!(missing, None);

        let empty_c = to_c_string("");
        let present = unsafe { cstr(empty_c) };
        assert_eq!(present, Some(""));
        unsafe { loom_string_free(empty_c) };
    }
}
