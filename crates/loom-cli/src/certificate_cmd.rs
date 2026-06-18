//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use std::io::Write as _;

use ::time::{Duration, OffsetDateTime};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, KeyPair, PKCS_ECDSA_P256_SHA256,
    PKCS_ECDSA_P384_SHA384, PKCS_ED25519, SignatureAlgorithm,
};
use rustls::pki_types::pem::PemObject as _;
use x509_parser::pem::Pem;
use x509_parser::prelude::*;

use super::*;

const EXPIRING_SOON_DAYS: i64 = 30;

pub(crate) fn run_certificate(action: CertificateCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        CertificateCmd::List { store } => run_certificate_list(&store, keys),
        CertificateCmd::Import {
            store,
            name,
            cert_chain,
            private_key,
            trust_bundle,
            force,
        } => run_certificate_import(
            &store,
            &name,
            &cert_chain,
            &private_key,
            trust_bundle.as_deref(),
            force,
            keys,
        ),
        CertificateCmd::Export {
            store,
            name,
            cert_chain,
            private_key,
            trust_bundle,
            force,
        } => run_certificate_export(
            &store,
            &name,
            cert_chain.as_deref(),
            private_key.as_deref(),
            trust_bundle.as_deref(),
            force,
            keys,
        ),
        CertificateCmd::Generate { action } => match action {
            CertificateGenerateCmd::SelfSigned {
                store,
                name,
                dns_names,
                ip_addresses,
                cn,
                days,
                algorithm,
                force,
            } => run_certificate_generate_self_signed(
                &store,
                CertificateSelfSignedRequest {
                    name: &name,
                    dns_names: &dns_names,
                    ip_addresses: &ip_addresses,
                    cn: cn.as_deref(),
                    days,
                    algorithm: &algorithm,
                    force,
                },
                keys,
            ),
        },
        CertificateCmd::Remove { store, name } => run_certificate_remove(&store, &name, keys),
        CertificateCmd::Audit { store, name } => run_certificate_audit(&store, &name, keys),
    }
}

fn run_certificate_list(store: &str, keys: &KeyOpts) -> Result<(), String> {
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let bundles = loom
        .store()
        .certificate_bundles()
        .map_err(|e| e.to_string())?;
    let references = certificate_bundle_served_listener_reference_map(loom.store())?;
    let seq = loom
        .store()
        .audit_append(Some(actor), "certificate.bundle.list", Some("certificates"))
        .map_err(|e| e.to_string())?;
    let mut out = format!("{{\"seq\":{seq},\"certificates\":[");
    for (idx, bundle) in bundles.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&certificate_bundle_record_json(
            bundle,
            certificate_bundle_references_for(&references, &bundle.name),
        ));
    }
    out.push_str("]}");
    println!("{out}");
    Ok(())
}

fn run_certificate_import(
    store: &str,
    name: &str,
    cert_chain: &str,
    private_key: &str,
    trust_bundle: Option<&str>,
    force: bool,
    keys: &KeyOpts,
) -> Result<(), String> {
    let server_cert_chain_pem =
        std::fs::read(cert_chain).map_err(|e| format!("read --cert-chain {cert_chain}: {e}"))?;
    let private_key_pem =
        std::fs::read(private_key).map_err(|e| format!("read --private-key {private_key}: {e}"))?;
    let trust_bundle_pem = trust_bundle
        .map(|path| std::fs::read(path).map_err(|e| format!("read --trust-bundle {path}: {e}")))
        .transpose()?;
    validate_certificate_material(
        &server_cert_chain_pem,
        &private_key_pem,
        trust_bundle_pem.as_deref(),
    )?;
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let mut record = loom
        .store()
        .certificate_bundle_record(
            name,
            server_cert_chain_pem,
            private_key_pem,
            trust_bundle_pem,
        )
        .map_err(|e| e.to_string())?;
    let action = if force {
        "certificate.bundle.import.force"
    } else {
        "certificate.bundle.import"
    };
    let target = certificate_bundle_target(name);
    let seq = loom
        .store()
        .save_certificate_bundle_audited(&record, Some(actor), action, Some(&target), force)
        .map_err(|e| e.to_string())?;
    record.created_audit_seq = record.created_audit_seq.or(Some(seq));
    record.updated_audit_seq = Some(seq);
    record.unencrypted_private_key_override = !loom.store().is_encrypted() && force;
    println!("{}", certificate_bundle_json(&record, seq, &[]));
    Ok(())
}

fn run_certificate_export(
    store: &str,
    name: &str,
    cert_chain: Option<&str>,
    private_key: Option<&str>,
    trust_bundle: Option<&str>,
    force: bool,
    keys: &KeyOpts,
) -> Result<(), String> {
    if cert_chain.is_none() && private_key.is_none() && trust_bundle.is_none() {
        return Err(
            "provide at least one of --cert-chain, --private-key, or --trust-bundle".to_string(),
        );
    }
    if private_key.is_some() && !force {
        return Err("exporting private keys requires --force".to_string());
    }
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let record = loom
        .store()
        .certificate_bundle(name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("certificate bundle {name:?} not found"))?;
    if let Some(path) = cert_chain {
        write_output_file(path, &record.server_cert_chain_pem, false, force)?;
    }
    if let Some(path) = private_key {
        write_output_file(path, &record.private_key_pem, true, force)?;
    }
    if let Some(path) = trust_bundle {
        let pem = record
            .trust_bundle_pem
            .as_ref()
            .ok_or_else(|| format!("certificate bundle {name:?} has no trust bundle"))?;
        write_output_file(path, pem, false, force)?;
    }
    let target = certificate_bundle_target(name);
    let action = if private_key.is_some() {
        "certificate.bundle.export_private_key"
    } else {
        "certificate.bundle.export"
    };
    let seq = loom
        .store()
        .audit_append(Some(actor), action, Some(&target))
        .map_err(|e| e.to_string())?;
    println!("{{\"seq\":{seq},\"name\":{}}}", json_string(name));
    Ok(())
}

struct CertificateSelfSignedRequest<'a> {
    name: &'a str,
    dns_names: &'a [String],
    ip_addresses: &'a [String],
    cn: Option<&'a str>,
    days: u32,
    algorithm: &'a str,
    force: bool,
}

fn run_certificate_generate_self_signed(
    store: &str,
    request: CertificateSelfSignedRequest<'_>,
    keys: &KeyOpts,
) -> Result<(), String> {
    if request.days == 0 {
        return Err("--days must be greater than zero".to_string());
    }
    let san_names = certificate_san_names(request.dns_names, request.ip_addresses, request.cn)?;
    let mut params = CertificateParams::new(san_names).map_err(|e| e.to_string())?;
    if let Some(cn) = request.cn {
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, cn);
        params.distinguished_name = dn;
    }
    let now = OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now
        .checked_add(Duration::days(i64::from(request.days)))
        .ok_or_else(|| "--days is too large".to_string())?;
    let key_pair = KeyPair::generate_for(certificate_algorithm(request.algorithm)?)
        .map_err(|e| format!("generate key pair: {e}"))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| format!("generate certificate: {e}"))?;
    save_generated_certificate(
        store,
        request.name,
        cert.pem().into_bytes(),
        key_pair.serialize_pem().into_bytes(),
        request.force,
        keys,
    )
}

fn save_generated_certificate(
    store: &str,
    name: &str,
    server_cert_chain_pem: Vec<u8>,
    private_key_pem: Vec<u8>,
    force: bool,
    keys: &KeyOpts,
) -> Result<(), String> {
    validate_certificate_material(&server_cert_chain_pem, &private_key_pem, None)?;
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let mut record = loom
        .store()
        .certificate_bundle_record(name, server_cert_chain_pem, private_key_pem, None)
        .map_err(|e| e.to_string())?;
    let action = if force {
        "certificate.bundle.generate_self_signed.force"
    } else {
        "certificate.bundle.generate_self_signed"
    };
    let target = certificate_bundle_target(name);
    let seq = loom
        .store()
        .save_certificate_bundle_audited(&record, Some(actor), action, Some(&target), force)
        .map_err(|e| e.to_string())?;
    record.created_audit_seq = record.created_audit_seq.or(Some(seq));
    record.updated_audit_seq = Some(seq);
    record.unencrypted_private_key_override = !loom.store().is_encrypted() && force;
    println!("{}", certificate_bundle_json(&record, seq, &[]));
    Ok(())
}

fn run_certificate_remove(store: &str, name: &str, keys: &KeyOpts) -> Result<(), String> {
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let references = certificate_bundle_served_listener_references(loom.store(), name)?;
    let target = certificate_bundle_target(name);
    if !references.is_empty() {
        let denied_target = certificate_bundle_denied_remove_target(name, &references);
        loom.store()
            .audit_append(
                Some(actor),
                "certificate.bundle.remove.denied",
                Some(&denied_target),
            )
            .map_err(|e| e.to_string())?;
        return Err(format!(
            "certificate bundle {name:?} is referenced by served listeners: {}",
            references.join(", ")
        ));
    }
    let seq = loom
        .store()
        .remove_certificate_bundle_audited(
            name,
            Some(actor),
            "certificate.bundle.remove",
            Some(&target),
        )
        .map_err(|e| e.to_string())?;
    println!("{{\"seq\":{seq},\"name\":{}}}", json_string(name));
    Ok(())
}

fn certificate_bundle_served_listener_references(
    store: &FileStore,
    name: &str,
) -> Result<Vec<String>, String> {
    Ok(certificate_bundle_served_listener_reference_map(store)?
        .remove(name)
        .unwrap_or_default())
}

fn certificate_bundle_served_listener_reference_map(
    store: &FileStore,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, String> {
    let mut references = std::collections::BTreeMap::<String, Vec<String>>::new();
    for record in store.served_listeners().map_err(|e| e.to_string())? {
        if let Some(name) = record.tls.certificate_bundle_ref.as_deref() {
            references
                .entry(name.to_string())
                .or_default()
                .push(record.id);
        }
    }
    Ok(references)
}

fn certificate_bundle_references_for<'a>(
    references: &'a std::collections::BTreeMap<String, Vec<String>>,
    name: &str,
) -> &'a [String] {
    references.get(name).map(Vec::as_slice).unwrap_or(&[])
}

fn certificate_bundle_denied_remove_target(name: &str, references: &[String]) -> String {
    let mut target = certificate_bundle_target(name);
    target.push_str(";served_listener_count=");
    target.push_str(&references.len().to_string());
    target.push_str(";served_listeners=");
    let mut first = true;
    for reference in references {
        let separator_len = usize::from(!first);
        if target.len() + separator_len + reference.len() > 900 {
            target.push_str(";truncated=true");
            break;
        }
        if !first {
            target.push(',');
        }
        target.push_str(reference);
        first = false;
    }
    target
}

fn run_certificate_audit(store: &str, name: &str, keys: &KeyOpts) -> Result<(), String> {
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let record = loom
        .store()
        .certificate_bundle(name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("certificate bundle {name:?} not found"))?;
    let references = certificate_bundle_served_listener_references(loom.store(), name)?;
    let target = certificate_bundle_target(name);
    let seq = loom
        .store()
        .audit_append(Some(actor), "certificate.bundle.audit", Some(&target))
        .map_err(|e| e.to_string())?;
    println!(
        "{}",
        certificate_bundle_audit_json(&record, seq, &references)
    );
    Ok(())
}

pub(crate) fn certificate_bundle_doctor_lines(store: &FileStore) -> Result<Vec<String>, String> {
    let references = certificate_bundle_served_listener_reference_map(store)?;
    let mut lines = Vec::new();
    for bundle in store.certificate_bundles().map_err(|e| e.to_string())? {
        let bundle_references = certificate_bundle_references_for(&references, &bundle.name);
        let health = certificate_bundle_doctor_health(&bundle);
        let server_certificate_count = health.server_certificate_count;
        let trust_bundle_certificate_count = health.trust_bundle_certificate_count;
        let name = doctor_field(&bundle.name);
        if health.reasons.is_empty() {
            lines.push(format!(
                "certificate_bundle_health\tok\tname={name}\treferences={}\tserver_certificates={server_certificate_count}\ttrust_bundle_certificates={trust_bundle_certificate_count}",
                bundle_references.len()
            ));
        } else {
            for reason in health.reasons {
                lines.push(format!(
                    "certificate_bundle_health\tunhealthy\tname={name}\treferences={}\tserver_certificates={server_certificate_count}\ttrust_bundle_certificates={trust_bundle_certificate_count}\treason={}",
                    bundle_references.len(),
                    doctor_field(&reason)
                ));
            }
        }
    }
    Ok(lines)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CertificateDoctorHealth {
    reasons: Vec<String>,
    server_certificate_count: usize,
    trust_bundle_certificate_count: usize,
}

fn certificate_bundle_doctor_health(
    record: &loom_store::CertificateBundleRecord,
) -> CertificateDoctorHealth {
    let server_infos = parse_certificate_infos(&record.server_cert_chain_pem);
    let trust_infos = record
        .trust_bundle_pem
        .as_ref()
        .map(|pem| parse_certificate_infos(pem));
    let mut reasons = Vec::new();
    let server_certificate_count = match &server_infos {
        Ok(infos) => infos.len(),
        Err(err) => {
            reasons.push(format!("server certificate parse error: {err}"));
            0
        }
    };
    let trust_bundle_certificate_count = match &trust_infos {
        Some(Ok(infos)) => infos.len(),
        Some(Err(err)) => {
            reasons.push(format!("trust bundle parse error: {err}"));
            0
        }
        None => 0,
    };
    let health = certificate_health(
        server_infos.as_ref().ok().map(Vec::as_slice),
        trust_infos
            .as_ref()
            .and_then(|infos| infos.as_ref().ok().map(Vec::as_slice)),
    );
    reasons.extend(health.reasons);
    CertificateDoctorHealth {
        reasons,
        server_certificate_count,
        trust_bundle_certificate_count,
    }
}

fn doctor_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\t' | '\n' | '\r' => ' ',
            _ => ch,
        })
        .collect()
}

fn validate_certificate_material(
    server_cert_chain_pem: &[u8],
    private_key_pem: &[u8],
    trust_bundle_pem: Option<&[u8]>,
) -> Result<(), String> {
    crate::tls_crypto::ensure_rustls_crypto_provider();
    parse_certificate_infos(server_cert_chain_pem)
        .map_err(|e| format!("invalid --cert-chain: {e}"))?;
    if let Some(pem) = trust_bundle_pem {
        parse_certificate_infos(pem).map_err(|e| format!("invalid --trust-bundle: {e}"))?;
    }
    let certs: Vec<_> = rustls::pki_types::CertificateDer::pem_slice_iter(server_cert_chain_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("invalid --cert-chain PEM: {e}"))?;
    if certs.is_empty() {
        return Err("invalid --cert-chain: no CERTIFICATE PEM block found".to_string());
    }
    let private_key = rustls::pki_types::PrivateKeyDer::from_pem_slice(private_key_pem)
        .map_err(|e| format!("invalid --private-key PEM: {e}"))?;
    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, private_key)
        .map(|_| ())
        .map_err(|e| format!("certificate chain and private key do not match: {e}"))
}

fn write_output_file(path: &str, bytes: &[u8], secret: bool, force: bool) -> Result<(), String> {
    if !force && std::path::Path::new(path).exists() {
        return Err(format!(
            "output file {path:?} exists; use --force to overwrite"
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if secret {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("write output file {path}: {e}"))?;
    file.write_all(bytes)
        .map_err(|e| format!("write output file {path}: {e}"))
}

fn certificate_san_names(
    dns_names: &[String],
    ip_addresses: &[String],
    cn: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    names.extend(dns_names.iter().cloned());
    names.extend(ip_addresses.iter().cloned());
    if names.is_empty() {
        let cn = cn.ok_or_else(|| {
            "provide at least one --dns, --ip, or --cn for a self-signed certificate".to_string()
        })?;
        names.push(cn.to_string());
    }
    Ok(names)
}

fn certificate_algorithm(name: &str) -> Result<&'static SignatureAlgorithm, String> {
    match name {
        "p256" | "ecdsa-p256" | "ecdsa-p256-sha256" => Ok(&PKCS_ECDSA_P256_SHA256),
        "p384" | "ecdsa-p384" | "ecdsa-p384-sha384" => Ok(&PKCS_ECDSA_P384_SHA384),
        "ed25519" => Ok(&PKCS_ED25519),
        _ => Err(format!(
            "unsupported --algorithm {name:?}; use p256, p384, or ed25519"
        )),
    }
}

fn certificate_bundle_target(name: &str) -> String {
    format!("name={name}")
}

fn certificate_bundle_json(
    record: &loom_store::CertificateBundleRecord,
    seq: u64,
    references: &[String],
) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"seq\":");
    out.push_str(&seq.to_string());
    out.push(',');
    out.push_str(&certificate_bundle_record_json(record, references)[1..]);
    out
}

fn certificate_bundle_record_json(
    record: &loom_store::CertificateBundleRecord,
    references: &[String],
) -> String {
    let server_infos = parse_certificate_infos(&record.server_cert_chain_pem).ok();
    let trust_infos = record
        .trust_bundle_pem
        .as_ref()
        .map(|pem| parse_certificate_infos(pem).ok());
    let health = certificate_health(
        server_infos.as_deref(),
        trust_infos
            .as_ref()
            .and_then(|infos| infos.as_ref().map(Vec::as_slice)),
    );
    let mut out = String::new();
    out.push('{');
    out.push_str("\"name\":");
    out.push_str(&json_string(&record.name));
    out.push_str(",\"schema_version\":");
    out.push_str(&record.schema_version.to_string());
    out.push_str(",\"profile\":");
    out.push_str(&json_string(&record.profile));
    out.push_str(",\"health\":");
    out.push_str(&health_json(&health));
    out.push_str(",\"server_certificates\":");
    out.push_str(&server_infos.as_ref().map_or(0, Vec::len).to_string());
    out.push_str(",\"server_cert_chain_digest\":");
    out.push_str(&json_string(&record.server_cert_chain_digest.to_string()));
    out.push_str(",\"private_key_digest\":");
    out.push_str(&json_string(&record.private_key_digest.to_string()));
    out.push_str(",\"trust_bundle_certificates\":");
    match trust_infos
        .as_ref()
        .and_then(|infos| infos.as_ref().map(Vec::len))
    {
        Some(count) => out.push_str(&count.to_string()),
        None => out.push_str("null"),
    }
    out.push_str(",\"trust_bundle_digest\":");
    match record.trust_bundle_digest {
        Some(digest) => out.push_str(&json_string(&digest.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"reference_count\":");
    out.push_str(&references.len().to_string());
    out.push_str(",\"served_listener_references\":");
    out.push_str(&json_string_array(references));
    out.push_str(",\"created_audit_seq\":");
    push_json_u64(&mut out, record.created_audit_seq);
    out.push_str(",\"updated_audit_seq\":");
    push_json_u64(&mut out, record.updated_audit_seq);
    out.push_str(",\"unencrypted_private_key_override\":");
    out.push_str(if record.unencrypted_private_key_override {
        "true"
    } else {
        "false"
    });
    out.push('}');
    out
}

fn certificate_bundle_audit_json(
    record: &loom_store::CertificateBundleRecord,
    seq: u64,
    references: &[String],
) -> String {
    let server_infos = parse_certificate_infos(&record.server_cert_chain_pem);
    let trust_infos = record
        .trust_bundle_pem
        .as_ref()
        .map(|pem| parse_certificate_infos(pem));
    let health = certificate_health(
        server_infos.as_ref().ok().map(Vec::as_slice),
        trust_infos
            .as_ref()
            .and_then(|infos| infos.as_ref().ok().map(Vec::as_slice)),
    );
    let mut out = String::new();
    out.push('{');
    out.push_str("\"seq\":");
    out.push_str(&seq.to_string());
    out.push_str(",\"bundle\":");
    out.push_str(&certificate_bundle_record_json(record, references));
    out.push_str(",\"health\":");
    out.push_str(&health_json(&health));
    out.push_str(",\"server_cert_chain\":");
    out.push_str(&certificate_infos_json_result(server_infos));
    out.push_str(",\"trust_bundle\":");
    match trust_infos {
        Some(infos) => out.push_str(&certificate_infos_json_result(infos)),
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

fn json_string_array(values: &[String]) -> String {
    let mut out = String::new();
    out.push('[');
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(value));
    }
    out.push(']');
    out
}

fn push_json_u64(out: &mut String, value: Option<u64>) {
    match value {
        Some(value) => out.push_str(&value.to_string()),
        None => out.push_str("null"),
    }
}

fn certificate_infos_json_result(infos: Result<Vec<CertificateInfo>, String>) -> String {
    match infos {
        Ok(infos) => certificate_infos_json(&infos),
        Err(err) => {
            let mut out = String::from("{\"parse_error\":");
            out.push_str(&json_string(&err));
            out.push('}');
            out
        }
    }
}

fn certificate_infos_json(infos: &[CertificateInfo]) -> String {
    let mut out = String::new();
    out.push('[');
    for (idx, info) in infos.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&certificate_info_json(info));
    }
    out.push(']');
    out
}

fn certificate_info_json(info: &CertificateInfo) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"subject_common_name\":");
    push_json_string_option(&mut out, info.subject_common_name.as_deref());
    out.push_str(",\"issuer_common_name\":");
    push_json_string_option(&mut out, info.issuer_common_name.as_deref());
    out.push_str(",\"not_before\":");
    out.push_str(&json_string(&info.not_before));
    out.push_str(",\"not_after\":");
    out.push_str(&json_string(&info.not_after));
    out.push_str(",\"expired\":");
    out.push_str(if info.expired { "true" } else { "false" });
    out.push_str(",\"expiring_soon\":");
    out.push_str(if info.expiring_soon { "true" } else { "false" });
    out.push('}');
    out
}

fn push_json_string_option(out: &mut String, value: Option<&str>) {
    match value {
        Some(value) => out.push_str(&json_string(value)),
        None => out.push_str("null"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CertificateInfo {
    issuer_common_name: Option<String>,
    subject_common_name: Option<String>,
    not_before: String,
    not_after: String,
    expired: bool,
    expiring_soon: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CertificateHealth {
    status: &'static str,
    reasons: Vec<String>,
}

fn parse_certificate_infos(pem: &[u8]) -> Result<Vec<CertificateInfo>, String> {
    let mut infos = Vec::new();
    let now = OffsetDateTime::now_utc();
    let expiring_soon_cutoff = now + Duration::days(EXPIRING_SOON_DAYS);
    for block in Pem::iter_from_buffer(pem) {
        let block = block.map_err(|e| e.to_string())?;
        if block.label != "CERTIFICATE" {
            continue;
        }
        let (_remaining, cert) =
            parse_x509_certificate(&block.contents).map_err(|e| e.to_string())?;
        let not_before = cert.validity().not_before.to_datetime();
        let not_after = cert.validity().not_after.to_datetime();
        infos.push(CertificateInfo {
            issuer_common_name: common_name(cert.issuer()),
            subject_common_name: common_name(cert.subject()),
            not_before: not_before.to_string(),
            not_after: not_after.to_string(),
            expired: not_after < now,
            expiring_soon: not_after >= now && not_after <= expiring_soon_cutoff,
        });
    }
    if infos.is_empty() {
        return Err("no CERTIFICATE PEM block found".to_string());
    }
    Ok(infos)
}

fn common_name(name: &X509Name<'_>) -> Option<String> {
    name.iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .map(str::to_string)
}

fn certificate_health(
    server_infos: Option<&[CertificateInfo]>,
    trust_infos: Option<&[CertificateInfo]>,
) -> CertificateHealth {
    let mut reasons = Vec::new();
    collect_health_reasons("server", server_infos, &mut reasons);
    collect_health_reasons("trust", trust_infos, &mut reasons);
    CertificateHealth {
        status: if reasons.is_empty() {
            "healthy"
        } else {
            "unhealthy"
        },
        reasons,
    }
}

fn collect_health_reasons(
    label: &str,
    infos: Option<&[CertificateInfo]>,
    reasons: &mut Vec<String>,
) {
    let Some(infos) = infos else {
        return;
    };
    for (idx, info) in infos.iter().enumerate() {
        let display = info
            .subject_common_name
            .as_deref()
            .unwrap_or("<no subject CN>");
        if info.expired {
            reasons.push(format!(
                "{label} certificate {idx} {display} expired at {}",
                info.not_after
            ));
        } else if info.expiring_soon {
            reasons.push(format!(
                "{label} certificate {idx} {display} expires within {EXPIRING_SOON_DAYS} days at {}",
                info.not_after
            ));
        }
    }
}

fn health_json(health: &CertificateHealth) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"status\":");
    out.push_str(&json_string(health.status));
    out.push_str(",\"reasons\":[");
    for (idx, reason) in health.reasons.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(reason));
    }
    out.push_str("]}");
    out
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use super::*;

    #[test]
    fn certificate_generate_export_import_audit_and_remove() {
        let store = temp("certificate-cli.loom");
        let cert = temp("certificate-cli-cert.pem");
        let key = temp("certificate-cli-key.pem");
        trun(store_init(store.clone())).unwrap();
        let err = trun(Command::Certificate {
            action: CertificateCmd::Generate {
                action: CertificateGenerateCmd::SelfSigned {
                    store: store.clone(),
                    name: "local".into(),
                    dns_names: vec!["localhost".into()],
                    ip_addresses: Vec::new(),
                    cn: Some("localhost".into()),
                    days: 365,
                    algorithm: "p256".into(),
                    force: false,
                },
            },
        })
        .unwrap_err();
        assert!(err.contains("--force"));
        trun(Command::Certificate {
            action: CertificateCmd::Generate {
                action: CertificateGenerateCmd::SelfSigned {
                    store: store.clone(),
                    name: "local".into(),
                    dns_names: vec!["localhost".into()],
                    ip_addresses: Vec::new(),
                    cn: Some("localhost".into()),
                    days: 365,
                    algorithm: "p256".into(),
                    force: true,
                },
            },
        })
        .unwrap();
        trun(Command::Certificate {
            action: CertificateCmd::List {
                store: store.clone(),
            },
        })
        .unwrap();
        trun(Command::Certificate {
            action: CertificateCmd::Audit {
                store: store.clone(),
                name: "local".into(),
            },
        })
        .unwrap();
        let err = trun(Command::Certificate {
            action: CertificateCmd::Export {
                store: store.clone(),
                name: "local".into(),
                cert_chain: Some(cert.clone()),
                private_key: Some(key.clone()),
                trust_bundle: None,
                force: false,
            },
        })
        .unwrap_err();
        assert!(err.contains("--force"));
        trun(Command::Certificate {
            action: CertificateCmd::Export {
                store: store.clone(),
                name: "local".into(),
                cert_chain: Some(cert.clone()),
                private_key: Some(key.clone()),
                trust_bundle: None,
                force: true,
            },
        })
        .unwrap();
        trun(Command::Certificate {
            action: CertificateCmd::Remove {
                store: store.clone(),
                name: "local".into(),
            },
        })
        .unwrap();
        trun(Command::Certificate {
            action: CertificateCmd::Import {
                store: store.clone(),
                name: "local".into(),
                cert_chain: cert,
                private_key: key,
                trust_bundle: None,
                force: true,
            },
        })
        .unwrap();
        let records = cli_open_loom_read(&store, &KeyOpts::default())
            .unwrap()
            .store()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .any(|entry| entry.action == "certificate.bundle.audit")
        );
        assert!(
            records
                .iter()
                .any(|entry| entry.action == "certificate.bundle.import.force")
        );
    }

    #[test]
    fn certificate_material_rejects_mismatched_key() {
        let key_one = KeyPair::generate().unwrap();
        let key_two = KeyPair::generate().unwrap();
        let cert = CertificateParams::new(vec!["localhost".into()])
            .unwrap()
            .self_signed(&key_one)
            .unwrap();
        let err = validate_certificate_material(
            cert.pem().as_bytes(),
            key_two.serialize_pem().as_bytes(),
            None,
        )
        .unwrap_err();
        assert!(err.contains("do not match"));
    }

    #[test]
    fn certificate_doctor_reports_invalid_bundle_material() {
        let store = temp("certificate-doctor-invalid.loom");
        trun(store_init(store.clone())).unwrap();
        let loom = cli_open_loom(&store, &KeyOpts::default()).unwrap();
        let actor = require_global_admin_actor(&loom).unwrap();
        let record = loom
            .store()
            .certificate_bundle_record(
                "bad",
                b"-----BEGIN CERTIFICATE-----\ncert\n-----END CERTIFICATE-----\n".to_vec(),
                b"-----BEGIN PRIVATE KEY-----\nkey\n-----END PRIVATE KEY-----\n".to_vec(),
                None,
            )
            .unwrap();
        loom.store()
            .save_certificate_bundle_audited(
                &record,
                Some(actor),
                "certificate.bundle.import.force",
                Some("name=bad"),
                true,
            )
            .unwrap();
        let lines = certificate_bundle_doctor_lines(loom.store()).unwrap();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("certificate_bundle_health\tunhealthy"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("server certificate parse error"))
        );
    }

    #[test]
    fn certificate_remove_rejects_served_listener_reference() {
        let store = temp("certificate-remove-reference.loom");
        trun(store_init(store.clone())).unwrap();
        trun(Command::Certificate {
            action: CertificateCmd::Generate {
                action: CertificateGenerateCmd::SelfSigned {
                    store: store.clone(),
                    name: "local".into(),
                    dns_names: vec!["localhost".into()],
                    ip_addresses: Vec::new(),
                    cn: Some("localhost".into()),
                    days: 365,
                    algorithm: "p256".into(),
                    force: true,
                },
            },
        })
        .unwrap();
        trun(Command::Serve {
            action: ServeCmd::Configure(Box::new(ServeConfigureArgs {
                store: store.clone(),
                surface: "admin".into(),
                selector: Vec::new(),
                bind: "127.0.0.1:8033".into(),
                transport: Some("rest".into()),
                profile: None,
                disabled: true,
                tls_certificate_bundle: Some("local".into()),
                tls_mode: None,
                auth_mode: None,
                exposure: None,
                audit_mode: None,
                request_size_limit: None,
                idle_timeout_ms: None,
                session_timeout_ms: None,
                network_access_policy: None,
            })),
        })
        .unwrap();
        let err = trun(Command::Certificate {
            action: CertificateCmd::Remove {
                store: store.clone(),
                name: "local".into(),
            },
        })
        .unwrap_err();
        assert!(err.contains("referenced by served listeners"));
        let loom = cli_open_loom(&store, &KeyOpts::default()).unwrap();
        let listener = loom.store().served_listeners().unwrap().remove(0);
        assert!(err.contains(&listener.id));
        let references =
            certificate_bundle_served_listener_references(loom.store(), "local").unwrap();
        assert_eq!(references, vec![listener.id.clone()]);
        let bundle = loom.store().certificate_bundle("local").unwrap().unwrap();
        let bundle_json = certificate_bundle_record_json(&bundle, &references);
        assert!(bundle_json.contains("\"reference_count\":1"));
        assert!(bundle_json.contains(&listener.id));
        let records = loom.store().audit_records().unwrap();
        assert!(
            records
                .iter()
                .any(|entry| entry.action == "certificate.bundle.remove.denied")
        );
        let actor = require_global_admin_actor(&loom).unwrap();
        loom.store()
            .remove_served_listener_audited(
                &listener.id,
                Some(actor),
                "serve.listener.remove",
                Some("surface=admin"),
            )
            .unwrap();
        drop(loom);
        trun(Command::Certificate {
            action: CertificateCmd::Remove {
                store,
                name: "local".into(),
            },
        })
        .unwrap();
    }

    fn trun(command: Command) -> Result<(), String> {
        run(command, &KeyOpts::default())
    }

    fn store_init(store: String) -> Command {
        Command::Store {
            action: StoreCmd::Init {
                store,
                encrypt: false,
                suite: None,
                identity_profile: None,
                fips: false,
            },
        }
    }

    fn temp(name: &str) -> String {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-certificate-cmd-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        path.to_string_lossy().into_owned()
    }
}
