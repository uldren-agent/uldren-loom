//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

pub(crate) fn run_network_access(action: NetworkAccessCmd, keys: &KeyOpts) -> Result<(), String> {
    match action {
        NetworkAccessCmd::List { store } => run_network_access_list(&store, keys),
        NetworkAccessCmd::Set(args) => run_network_access_set(*args, keys),
        NetworkAccessCmd::Remove { store, name } => run_network_access_remove(&store, &name, keys),
        NetworkAccessCmd::Audit { store, name } => run_network_access_audit(&store, &name, keys),
    }
}

fn run_network_access_list(store: &str, keys: &KeyOpts) -> Result<(), String> {
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let policies = loom
        .store()
        .network_access_policies()
        .map_err(|e| e.to_string())?;
    let references = network_access_served_listener_reference_map(loom.store())?;
    let seq = loom
        .store()
        .audit_append(
            Some(actor),
            "network-access.policy.list",
            Some("network-access"),
        )
        .map_err(|e| e.to_string())?;
    let mut out = format!("{{\"seq\":{seq},\"policies\":[");
    for (idx, policy) in policies.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&network_access_policy_record_json(
            loom.store(),
            policy,
            network_access_references_for(&references, &policy.name),
        )?);
    }
    out.push_str("]}");
    println!("{out}");
    Ok(())
}

fn run_network_access_set(args: NetworkAccessSetArgs, keys: &KeyOpts) -> Result<(), String> {
    let default_action =
        loom_store::NetworkAccessAction::parse(&args.default_action).map_err(|e| e.to_string())?;
    let rules = network_access_rules_from_args(&args)?;
    let loom = cli_open_loom(&args.store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let mut policy = FileStore::network_access_policy_record(
        &args.name,
        args.description,
        default_action,
        rules,
    )
    .map_err(|e| e.to_string())?;
    let target = network_access_policy_target(&args.name);
    let seq = loom
        .store()
        .save_network_access_policy_audited(
            &policy,
            Some(actor),
            "network-access.policy.set",
            Some(&target),
        )
        .map_err(|e| e.to_string())?;
    policy.created_audit_seq = policy.created_audit_seq.or(Some(seq));
    policy.updated_audit_seq = Some(seq);
    println!(
        "{}",
        network_access_policy_json(loom.store(), &policy, seq, &[])?
    );
    Ok(())
}

fn run_network_access_remove(store: &str, name: &str, keys: &KeyOpts) -> Result<(), String> {
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let references = network_access_served_listener_references(loom.store(), name)?;
    let target = network_access_policy_target(name);
    if !references.is_empty() {
        let denied_target = network_access_denied_remove_target(name, &references);
        loom.store()
            .audit_append(
                Some(actor),
                "network-access.policy.remove.denied",
                Some(&denied_target),
            )
            .map_err(|e| e.to_string())?;
        return Err(format!(
            "network access policy {name:?} is referenced by served listeners: {}",
            references.join(", ")
        ));
    }
    let seq = loom
        .store()
        .remove_network_access_policy_audited(
            name,
            Some(actor),
            "network-access.policy.remove",
            Some(&target),
        )
        .map_err(|e| e.to_string())?;
    println!("{{\"seq\":{seq},\"name\":{}}}", json_string(name));
    Ok(())
}

fn run_network_access_audit(store: &str, name: &str, keys: &KeyOpts) -> Result<(), String> {
    let loom = cli_open_loom(store, keys)?;
    let actor = require_global_admin_actor(&loom)?;
    let policy = loom
        .store()
        .network_access_policy(name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("network access policy {name:?} not found"))?;
    let references = network_access_served_listener_references(loom.store(), name)?;
    let target = network_access_policy_target(name);
    let seq = loom
        .store()
        .audit_append(Some(actor), "network-access.policy.audit", Some(&target))
        .map_err(|e| e.to_string())?;
    println!(
        "{}",
        network_access_policy_json(loom.store(), &policy, seq, &references)?
    );
    Ok(())
}

fn network_access_rules_from_args(
    args: &NetworkAccessSetArgs,
) -> Result<Vec<loom_store::NetworkAccessRule>, String> {
    let mut rules = match args.rules.as_deref() {
        Some(path) => network_access_rules_from_json_file(path)?,
        None => Vec::new(),
    };
    for value in &args.deny_sources {
        push_network_access_cidr_rule(
            &mut rules,
            "deny-source",
            loom_store::NetworkAccessAction::Deny,
            value,
        )?;
    }
    for value in &args.allow_sources {
        push_network_access_cidr_rule(
            &mut rules,
            "allow-source",
            loom_store::NetworkAccessAction::Allow,
            value,
        )?;
    }
    if args.deny_mtls {
        push_network_access_mtls_rule(
            &mut rules,
            "deny-mtls",
            loom_store::NetworkAccessAction::Deny,
            None,
            None,
            None,
        );
    }
    if args.allow_mtls {
        push_network_access_mtls_rule(
            &mut rules,
            "allow-mtls",
            loom_store::NetworkAccessAction::Allow,
            None,
            None,
            None,
        );
    }
    for value in &args.deny_mtls_subjects {
        push_network_access_mtls_rule(
            &mut rules,
            "deny-mtls-subject",
            loom_store::NetworkAccessAction::Deny,
            Some(value.clone()),
            None,
            None,
        );
    }
    for value in &args.allow_mtls_subjects {
        push_network_access_mtls_rule(
            &mut rules,
            "allow-mtls-subject",
            loom_store::NetworkAccessAction::Allow,
            Some(value.clone()),
            None,
            None,
        );
    }
    for value in &args.deny_mtls_sans {
        push_network_access_mtls_rule(
            &mut rules,
            "deny-mtls-san",
            loom_store::NetworkAccessAction::Deny,
            None,
            Some(value.clone()),
            None,
        );
    }
    for value in &args.allow_mtls_sans {
        push_network_access_mtls_rule(
            &mut rules,
            "allow-mtls-san",
            loom_store::NetworkAccessAction::Allow,
            None,
            Some(value.clone()),
            None,
        );
    }
    for value in &args.deny_mtls_issuers {
        push_network_access_mtls_rule(
            &mut rules,
            "deny-mtls-issuer",
            loom_store::NetworkAccessAction::Deny,
            None,
            None,
            Some(value.clone()),
        );
    }
    for value in &args.allow_mtls_issuers {
        push_network_access_mtls_rule(
            &mut rules,
            "allow-mtls-issuer",
            loom_store::NetworkAccessAction::Allow,
            None,
            None,
            Some(value.clone()),
        );
    }
    for value in &args.trusted_proxies {
        let cidr = loom_store::NetworkAccessCidr::parse(value).map_err(|e| e.to_string())?;
        let id = format!("trusted-proxy-{}", rules.len() + 1);
        rules.push(loom_store::NetworkAccessRule {
            id,
            action: loom_store::NetworkAccessAction::Allow,
            source_cidr: None,
            trusted_proxy_cidr: Some(cidr),
            require_mtls: false,
            client_cert_subject: None,
            client_cert_san: None,
            client_cert_issuer: None,
            description: None,
        });
    }
    Ok(rules)
}

fn network_access_rules_from_json_file(
    path: &str,
) -> Result<Vec<loom_store::NetworkAccessRule>, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read --rules {path}: {e}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse --rules {path}: {e}"))?;
    let rules = value
        .as_array()
        .ok_or_else(|| "--rules JSON must be an array".to_string())?;
    rules
        .iter()
        .enumerate()
        .map(|(idx, value)| network_access_rule_from_json(idx, value))
        .collect()
}

fn network_access_rule_from_json(
    idx: usize,
    value: &serde_json::Value,
) -> Result<loom_store::NetworkAccessRule, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("rule {idx} must be an object"))?;
    let id = json_string_field(object, "id")?.unwrap_or_else(|| format!("rule-{}", idx + 1));
    let action = json_string_field(object, "action")?
        .ok_or_else(|| format!("rule {idx} missing action"))
        .and_then(|value| {
            loom_store::NetworkAccessAction::parse(&value).map_err(|e| e.to_string())
        })?;
    let source_cidr = json_string_field(object, "source_cidr")?
        .map(|value| loom_store::NetworkAccessCidr::parse(&value).map_err(|e| e.to_string()))
        .transpose()?;
    let trusted_proxy_cidr = json_string_field(object, "trusted_proxy_cidr")?
        .map(|value| loom_store::NetworkAccessCidr::parse(&value).map_err(|e| e.to_string()))
        .transpose()?;
    let require_mtls = json_bool_field(object, "require_mtls")?.unwrap_or(false);
    Ok(loom_store::NetworkAccessRule {
        id,
        action,
        source_cidr,
        trusted_proxy_cidr,
        require_mtls,
        client_cert_subject: json_string_field(object, "client_cert_subject")?,
        client_cert_san: json_string_field(object, "client_cert_san")?,
        client_cert_issuer: json_string_field(object, "client_cert_issuer")?,
        description: json_string_field(object, "description")?,
    })
}

fn json_string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<String>, String> {
    match object.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("rule field {key:?} must be a string or null")),
    }
}

fn json_bool_field(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<bool>, String> {
    match object.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("rule field {key:?} must be a boolean or null")),
    }
}

fn push_network_access_cidr_rule(
    rules: &mut Vec<loom_store::NetworkAccessRule>,
    prefix: &str,
    action: loom_store::NetworkAccessAction,
    value: &str,
) -> Result<(), String> {
    let cidr = loom_store::NetworkAccessCidr::parse(value).map_err(|e| e.to_string())?;
    let id = format!("{prefix}-{}", rules.len() + 1);
    rules.push(loom_store::NetworkAccessRule {
        id,
        action,
        source_cidr: Some(cidr),
        trusted_proxy_cidr: None,
        require_mtls: false,
        client_cert_subject: None,
        client_cert_san: None,
        client_cert_issuer: None,
        description: None,
    });
    Ok(())
}

fn push_network_access_mtls_rule(
    rules: &mut Vec<loom_store::NetworkAccessRule>,
    prefix: &str,
    action: loom_store::NetworkAccessAction,
    subject: Option<String>,
    san: Option<String>,
    issuer: Option<String>,
) {
    let id = format!("{prefix}-{}", rules.len() + 1);
    rules.push(loom_store::NetworkAccessRule {
        id,
        action,
        source_cidr: None,
        trusted_proxy_cidr: None,
        require_mtls: true,
        client_cert_subject: subject,
        client_cert_san: san,
        client_cert_issuer: issuer,
        description: None,
    });
}

fn network_access_served_listener_references(
    store: &FileStore,
    name: &str,
) -> Result<Vec<String>, String> {
    Ok(network_access_served_listener_reference_map(store)?
        .remove(name)
        .unwrap_or_default())
}

fn network_access_served_listener_reference_map(
    store: &FileStore,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, String> {
    let mut references = std::collections::BTreeMap::<String, Vec<String>>::new();
    for record in store.served_listeners().map_err(|e| e.to_string())? {
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
    references: &'a std::collections::BTreeMap<String, Vec<String>>,
    name: &str,
) -> &'a [String] {
    references.get(name).map(Vec::as_slice).unwrap_or(&[])
}

fn network_access_policy_target(name: &str) -> String {
    format!("name={name}")
}

fn network_access_denied_remove_target(name: &str, references: &[String]) -> String {
    let mut target = network_access_policy_target(name);
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

fn network_access_policy_json(
    store: &FileStore,
    policy: &loom_store::NetworkAccessPolicyRecord,
    seq: u64,
    references: &[String],
) -> Result<String, String> {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"seq\":");
    out.push_str(&seq.to_string());
    out.push(',');
    out.push_str(&network_access_policy_record_json(store, policy, references)?[1..]);
    Ok(out)
}

fn network_access_policy_record_json(
    store: &FileStore,
    policy: &loom_store::NetworkAccessPolicyRecord,
    references: &[String],
) -> Result<String, String> {
    let digest = store
        .network_access_policy_digest(policy)
        .map_err(|e| e.to_string())?;
    let mut out = String::new();
    out.push('{');
    out.push_str("\"name\":");
    out.push_str(&json_string(&policy.name));
    out.push_str(",\"schema_version\":");
    out.push_str(&policy.schema_version.to_string());
    out.push_str(",\"digest\":");
    out.push_str(&json_string(&digest.to_string()));
    out.push_str(",\"description\":");
    push_json_option(&mut out, policy.description.as_deref());
    out.push_str(",\"default_action\":");
    out.push_str(&json_string(policy.default_action.as_str()));
    out.push_str(",\"created_audit_seq\":");
    push_network_access_json_u64_option(&mut out, policy.created_audit_seq);
    out.push_str(",\"updated_audit_seq\":");
    push_network_access_json_u64_option(&mut out, policy.updated_audit_seq);
    out.push_str(",\"references\":[");
    for (idx, reference) in references.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(reference));
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
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&rule.id));
    out.push_str(",\"action\":");
    out.push_str(&json_string(rule.action.as_str()));
    out.push_str(",\"source_cidr\":");
    push_json_option(
        &mut out,
        rule.source_cidr
            .as_ref()
            .map(|value| value.to_string())
            .as_deref(),
    );
    out.push_str(",\"trusted_proxy_cidr\":");
    push_json_option(
        &mut out,
        rule.trusted_proxy_cidr
            .as_ref()
            .map(|value| value.to_string())
            .as_deref(),
    );
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

pub(crate) fn network_access_policy_doctor_lines(store: &FileStore) -> Result<Vec<String>, String> {
    let policies = store.network_access_policies().map_err(|e| e.to_string())?;
    let references = network_access_served_listener_reference_map(store)?;
    let policy_map = policies
        .iter()
        .map(|policy| (policy.name.clone(), policy.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut lines = Vec::new();
    for policy in &policies {
        let refs = network_access_references_for(&references, &policy.name);
        let has_allow_rule = policy
            .rules
            .iter()
            .any(|rule| rule.action == loom_store::NetworkAccessAction::Allow);
        let digest = store
            .network_access_policy_digest(policy)
            .map_err(|e| e.to_string())?;
        let name = network_access_doctor_field(&policy.name);
        if policy.default_action == loom_store::NetworkAccessAction::Deny && !has_allow_rule {
            lines.push(format!(
                "network_access_policy_health\twarning\tname={name}\treferences={}\trules={}\tdigest={}\treason=default_deny_without_allow_rules",
                refs.len(),
                policy.rules.len(),
                digest
            ));
        } else {
            lines.push(format!(
                "network_access_policy_health\tok\tname={name}\treferences={}\trules={}\tdigest={digest}",
                refs.len(),
                policy.rules.len()
            ));
        }
    }
    let mut missing_refs = Vec::new();
    let policy_names = store
        .network_access_policies()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|policy| policy.name)
        .collect::<std::collections::BTreeSet<_>>();
    for (name, listeners) in references {
        if !policy_names.contains(&name) {
            missing_refs.push((name, listeners));
        }
    }
    for (name, listeners) in missing_refs {
        lines.push(format!(
            "network_access_policy_health\tunhealthy\tname={}\treferences={}\treason=referenced_policy_missing",
            network_access_doctor_field(&name),
            listeners.len()
        ));
    }
    for listener in store.served_listeners().map_err(|e| e.to_string())? {
        if !listener.enabled {
            continue;
        }
        if listener.network_access_policy_ref.is_none()
            && served_listener_bind_is_public(&listener.bind)
        {
            lines.push(format!(
                "network_access_listener_health\twarning\tlistener={}\tsurface={}\ttransport={}\tbind={}\treason=public_bind_without_network_access_policy",
                network_access_doctor_field(&listener.id),
                network_access_doctor_field(&listener.surface),
                network_access_doctor_field(&listener.transport),
                network_access_doctor_field(&listener.bind)
            ));
            continue;
        }
        let Some(policy_name) = listener.network_access_policy_ref.as_deref() else {
            continue;
        };
        let Some(policy) = policy_map.get(policy_name) else {
            continue;
        };
        if !network_access_policy_requires_mtls(policy) {
            continue;
        }
        if listener.tls.mode != "direct" {
            lines.push(format!(
                "network_access_listener_health\twarning\tlistener={}\tpolicy={}\treason=mtls_policy_without_direct_tls",
                network_access_doctor_field(&listener.id),
                network_access_doctor_field(policy_name)
            ));
            continue;
        }
        let Some(bundle_name) = listener.tls.certificate_bundle_ref.as_deref() else {
            lines.push(format!(
                "network_access_listener_health\twarning\tlistener={}\tpolicy={}\treason=mtls_policy_without_certificate_bundle",
                network_access_doctor_field(&listener.id),
                network_access_doctor_field(policy_name)
            ));
            continue;
        };
        match store.certificate_bundle(bundle_name).map_err(|e| e.to_string())? {
            Some(bundle) if bundle.trust_bundle_pem.is_some() => {}
            Some(_) => lines.push(format!(
                "network_access_listener_health\twarning\tlistener={}\tpolicy={}\tbundle={}\treason=mtls_policy_without_trust_bundle",
                network_access_doctor_field(&listener.id),
                network_access_doctor_field(policy_name),
                network_access_doctor_field(bundle_name)
            )),
            None => lines.push(format!(
                "network_access_listener_health\twarning\tlistener={}\tpolicy={}\tbundle={}\treason=mtls_policy_certificate_bundle_missing",
                network_access_doctor_field(&listener.id),
                network_access_doctor_field(policy_name),
                network_access_doctor_field(bundle_name)
            )),
        }
    }
    Ok(lines)
}

fn network_access_policy_requires_mtls(policy: &loom_store::NetworkAccessPolicyRecord) -> bool {
    policy.rules.iter().any(|rule| {
        rule.require_mtls
            || rule.client_cert_subject.is_some()
            || rule.client_cert_san.is_some()
            || rule.client_cert_issuer.is_some()
    })
}

fn served_listener_bind_is_public(bind: &str) -> bool {
    bind.parse::<std::net::SocketAddr>()
        .map(|addr| !addr.ip().is_loopback())
        .unwrap_or(false)
}

fn push_network_access_json_u64_option(out: &mut String, value: Option<u64>) {
    match value {
        Some(value) => out.push_str(&value.to_string()),
        None => out.push_str("null"),
    }
}

fn network_access_doctor_field(value: &str) -> String {
    value.replace(['\t', '\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_access_policy_attach_blocks_remove() {
        let store = temp("attach-blocks-remove");
        trun(store_init(store.clone())).unwrap();
        trun(Command::NetworkAccess {
            action: NetworkAccessCmd::Set(Box::new(NetworkAccessSetArgs {
                store: store.clone(),
                name: "office".into(),
                description: Some("office network".into()),
                default_action: "deny".into(),
                allow_sources: vec!["127.0.0.1".into()],
                deny_sources: Vec::new(),
                allow_mtls: false,
                deny_mtls: false,
                allow_mtls_subjects: Vec::new(),
                deny_mtls_subjects: Vec::new(),
                allow_mtls_sans: Vec::new(),
                deny_mtls_sans: Vec::new(),
                allow_mtls_issuers: Vec::new(),
                deny_mtls_issuers: Vec::new(),
                trusted_proxies: Vec::new(),
                rules: None,
            })),
        })
        .unwrap();
        trun(Command::Serve {
            action: ServeCmd::Configure(Box::new(ServeConfigureArgs {
                store: store.clone(),
                surface: "admin".into(),
                selector: Vec::new(),
                bind: "127.0.0.1:8044".into(),
                transport: Some("rest".into()),
                profile: None,
                mode: None,
                disabled: true,
                tls_certificate_bundle: None,
                tls_mode: None,
                auth_mode: None,
                exposure: None,
                audit_mode: None,
                request_size_limit: None,
                idle_timeout_ms: None,
                session_timeout_ms: None,
                network_access_policy: Some("office".into()),
            })),
        })
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let listener = loom.store().served_listeners().unwrap().remove(0);
        assert_eq!(
            listener.network_access_policy_ref.as_deref(),
            Some("office")
        );
        drop(loom);

        let err = trun(Command::NetworkAccess {
            action: NetworkAccessCmd::Remove {
                store: store.clone(),
                name: "office".into(),
            },
        })
        .unwrap_err();
        assert!(err.contains("referenced by served listeners"));
        assert!(err.contains(&listener.id));
        let _ = std::fs::remove_file(store);
    }

    #[test]
    fn network_access_doctor_warns_for_public_bind_without_policy() {
        let store = temp("doctor-public-bind");
        trun(store_init(store.clone())).unwrap();
        trun(Command::Serve {
            action: ServeCmd::Configure(Box::new(ServeConfigureArgs {
                store: store.clone(),
                surface: "admin".into(),
                selector: Vec::new(),
                bind: "0.0.0.0:8045".into(),
                transport: Some("rest".into()),
                profile: None,
                mode: None,
                disabled: false,
                tls_certificate_bundle: None,
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

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let lines = network_access_policy_doctor_lines(loom.store()).unwrap();
        assert!(lines.iter().any(|line| {
            line.contains("network_access_listener_health")
                && line.contains("public_bind_without_network_access_policy")
        }));
        drop(loom);
        let _ = std::fs::remove_file(store);
    }

    #[test]
    fn network_access_doctor_warns_for_mtls_policy_without_direct_tls() {
        let store = temp("doctor-mtls");
        trun(store_init(store.clone())).unwrap();
        trun(Command::NetworkAccess {
            action: NetworkAccessCmd::Set(Box::new(NetworkAccessSetArgs {
                store: store.clone(),
                name: "mtls".into(),
                description: None,
                default_action: "deny".into(),
                allow_sources: Vec::new(),
                deny_sources: Vec::new(),
                allow_mtls: true,
                deny_mtls: false,
                allow_mtls_subjects: Vec::new(),
                deny_mtls_subjects: Vec::new(),
                allow_mtls_sans: Vec::new(),
                deny_mtls_sans: Vec::new(),
                allow_mtls_issuers: Vec::new(),
                deny_mtls_issuers: Vec::new(),
                trusted_proxies: Vec::new(),
                rules: None,
            })),
        })
        .unwrap();
        let loom = cli_open_loom(&store, &KeyOpts::default()).unwrap();
        let mut listener =
            FileStore::served_listener_record("admin", Vec::new(), "rest", "127.0.0.1:8046", true)
                .unwrap();
        listener.network_access_policy_ref = Some("mtls".to_string());
        loom.store()
            .save_served_listener_audited(
                &listener,
                None,
                "serve.listener.configure",
                Some("test=mtls"),
            )
            .unwrap();
        let lines = network_access_policy_doctor_lines(loom.store()).unwrap();
        assert!(lines.iter().any(|line| {
            line.contains("network_access_listener_health")
                && line.contains("mtls_policy_without_direct_tls")
        }));
        drop(loom);
        let _ = std::fs::remove_file(store);
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
            "loom-network-access-cmd-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        path.to_string_lossy().into_owned()
    }
}
