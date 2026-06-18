//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch < '\u{20}' => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn principal_kind_str(kind: PrincipalKind) -> &'static str {
    match kind {
        PrincipalKind::Root => "root",
        PrincipalKind::User => "user",
        PrincipalKind::Service => "service",
    }
}

fn parse_principal_kind(value: &str) -> PyResult<PrincipalKind> {
    match value {
        "root" => Ok(PrincipalKind::Root),
        "user" => Ok(PrincipalKind::User),
        "service" => Ok(PrincipalKind::Service),
        other => Err(PyRuntimeError::new_err(format!(
            "unknown principal kind {other:?}"
        ))),
    }
}

fn parse_external_credential_kind(value: &str) -> PyResult<ExternalCredentialKind> {
    ExternalCredentialKind::parse(value).map_err(py_err)
}

fn acl_effect_str(effect: AclEffect) -> &'static str {
    match effect {
        AclEffect::Allow => "allow",
        AclEffect::Deny => "deny",
    }
}

fn parse_acl_effect(value: i32) -> PyResult<AclEffect> {
    match value {
        0 => Ok(AclEffect::Allow),
        1 => Ok(AclEffect::Deny),
        other => Err(PyRuntimeError::new_err(format!(
            "unknown acl effect {other}"
        ))),
    }
}

fn acl_right_str(right: AclRight) -> &'static str {
    match right {
        AclRight::Read => "read",
        AclRight::Write => "write",
        AclRight::Advance => "advance",
        AclRight::Merge => "merge",
        AclRight::Execute => "execute",
        AclRight::Admin => "admin",
    }
}

fn parse_acl_rights(mask: u32) -> PyResult<Vec<AclRight>> {
    const KNOWN: u32 = 0x3f;
    if mask == 0 {
        return Err(PyRuntimeError::new_err(
            "acl rights mask must not be empty".to_string(),
        ));
    }
    if mask & !KNOWN != 0 {
        return Err(PyRuntimeError::new_err(format!(
            "unknown acl rights bits {:#x}",
            mask & !KNOWN
        )));
    }
    let mut rights = Vec::new();
    for (bit, right) in [
        (0x01, AclRight::Read),
        (0x02, AclRight::Write),
        (0x04, AclRight::Advance),
        (0x08, AclRight::Merge),
        (0x10, AclRight::Execute),
        (0x20, AclRight::Admin),
    ] {
        if mask & bit != 0 {
            rights.push(right);
        }
    }
    Ok(rights)
}

fn parse_acl_subject(value: &str) -> PyResult<AclSubject> {
    match value {
        "*" | "everyone" => Ok(AclSubject::Everyone),
        role if role.starts_with("role:") => Ok(AclSubject::Role(
            WorkspaceId::parse(&role[5..]).map_err(py_err)?,
        )),
        other => Ok(AclSubject::Principal(
            WorkspaceId::parse(other).map_err(py_err)?,
        )),
    }
}

fn local_auth(
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<LocalOpenAuth> {
    match (auth_principal, auth_passphrase) {
        (Some(principal), Some(passphrase)) => Ok(LocalOpenAuth {
            principal: Some(WorkspaceId::parse(principal).map_err(py_err)?),
            passphrase: Some(passphrase.to_string()),
            ..Default::default()
        }),
        (None, None) => Ok(LocalOpenAuth::default()),
        _ => Err(PyRuntimeError::new_err(
            "auth_principal and auth_passphrase must be supplied together",
        )),
    }
}

fn open_control_read(
    path: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<Loom<FileStore>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let auth = local_auth(auth_principal, auth_passphrase)?;
    attach_local_auth(loom, &auth).map_err(py_err)
}

fn open_control_write(
    path: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<Loom<FileStore>> {
    let loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let auth = local_auth(auth_principal, auth_passphrase)?;
    attach_local_auth(loom, &auth).map_err(py_err)
}

fn role_json(role: &IdentityRole) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&role.id.to_string()));
    out.push_str(",\"name\":");
    out.push_str(&json_string(&role.name));
    out.push_str(",\"enabled\":");
    out.push_str(if role.enabled { "true" } else { "false" });
    out.push('}');
    out
}

fn principal_json(principal: &Principal) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&principal.id.to_string()));
    out.push_str(",\"handle\":");
    out.push_str(&json_string(&principal.handle));
    out.push_str(",\"name\":");
    out.push_str(&json_string(&principal.name));
    out.push_str(",\"kind\":");
    out.push_str(&json_string(principal_kind_str(principal.kind)));
    out.push_str(",\"enabled\":");
    out.push_str(if principal.enabled { "true" } else { "false" });
    out.push_str(",\"has_passphrase\":");
    out.push_str(if principal.has_passphrase {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"roles\":[");
    for (idx, role) in principal.roles.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(&role.to_string()));
    }
    out.push(']');
    out.push('}');
    out
}

fn app_credential_json(credential: &loom_core::AppCredential) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&credential.id.to_string()));
    out.push_str(",\"principal\":");
    out.push_str(&json_string(&credential.principal.to_string()));
    out.push_str(",\"label\":");
    out.push_str(&json_string(&credential.label));
    out.push_str(",\"enabled\":");
    out.push_str(if credential.enabled { "true" } else { "false" });
    out.push('}');
    out
}

fn external_credential_json(credential: &ExternalCredential) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&credential.id.to_string()));
    out.push_str(",\"principal\":");
    out.push_str(&json_string(&credential.principal.to_string()));
    out.push_str(",\"kind\":");
    out.push_str(&json_string(credential.kind.as_str()));
    out.push_str(",\"label\":");
    out.push_str(&json_string(&credential.label));
    out.push_str(",\"issuer\":");
    out.push_str(&json_string(&credential.issuer));
    out.push_str(",\"subject\":");
    out.push_str(&json_string(&credential.subject));
    out.push_str(",\"material_digest\":");
    match credential.material_digest.as_deref() {
        Some(digest) => out.push_str(&json_string(digest)),
        None => out.push_str("null"),
    }
    out.push_str(",\"enabled\":");
    out.push_str(if credential.enabled { "true" } else { "false" });
    out.push('}');
    out
}

fn identity_list_json_inner(identity: &IdentityStore) -> String {
    let mut out = String::from("{\"authenticated_mode\":");
    out.push_str(if identity.authenticated_mode() {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"root\":");
    match identity.root_principal() {
        Some(root) => out.push_str(&json_string(&root.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"principals\":[");
    for (idx, principal) in identity.principals().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&principal_json(principal));
    }
    out.push_str("],\"roles\":[");
    for (idx, role) in identity.roles().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&role_json(role));
    }
    out.push_str("],\"app_credentials\":[");
    for (idx, credential) in identity.app_credentials().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&app_credential_json(credential));
    }
    out.push_str("],\"external_credentials\":[");
    for (idx, credential) in identity.external_credentials().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&external_credential_json(credential));
    }
    out.push_str("],\"public_keys\":[");
    for (idx, key) in identity.public_keys().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&identity_public_key_json(key));
    }
    out.push_str("]}");
    out
}

fn identity_public_key_json(key: &loom_core::IdentityPublicKey) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&key.id.to_string()));
    out.push_str(",\"principal\":");
    out.push_str(&json_string(&key.principal.to_string()));
    out.push_str(",\"label\":");
    out.push_str(&json_string(&key.label));
    out.push_str(",\"algorithm\":");
    out.push_str(&json_string(&key.algorithm));
    out.push_str(",\"public_key_hex\":");
    out.push_str(&json_string(&hex_bytes(&key.public_key)));
    out.push_str(",\"enabled\":");
    out.push_str(if key.enabled { "true" } else { "false" });
    out.push('}');
    out
}

fn acl_grant_json(grant: &AclGrant) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"effect\":");
    out.push_str(&json_string(acl_effect_str(grant.effect)));
    out.push_str(",\"subject\":");
    match grant.subject {
        AclSubject::Everyone => out.push_str(&json_string("*")),
        AclSubject::Principal(principal) => out.push_str(&json_string(&principal.to_string())),
        AclSubject::Role(role) => {
            out.push_str(&json_string(&format!("role:{role}")));
        }
    }
    out.push_str(",\"subject_kind\":");
    match grant.subject {
        AclSubject::Everyone => out.push_str(&json_string("everyone")),
        AclSubject::Principal(_) => out.push_str(&json_string("principal")),
        AclSubject::Role(_) => out.push_str(&json_string("role")),
    }
    out.push_str(",\"workspace\":");
    match grant.workspace {
        Some(ns) => out.push_str(&json_string(&ns.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"domain\":");
    match grant.domain {
        Some(domain) => out.push_str(&json_string(domain.as_str())),
        None => out.push_str("null"),
    }
    out.push_str(",\"ref_glob\":");
    match grant.ref_glob.as_deref() {
        Some(ref_glob) => out.push_str(&json_string(ref_glob)),
        None => out.push_str("null"),
    }
    out.push_str(",\"rights\":[");
    for (idx, right) in grant.rights.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(acl_right_str(*right)));
    }
    out.push_str("],\"scopes\":[");
    for (idx, scope) in grant.scopes.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&acl_scope_json(scope));
    }
    out.push_str("],\"predicate\":");
    match &grant.predicate {
        Some(predicate) => {
            out.push_str("{\"language\":");
            out.push_str(&json_string(&predicate.language));
            out.push_str(",\"expression\":");
            out.push_str(&json_string(&predicate.expression));
            out.push('}');
        }
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

fn acl_scope_json(scope: &AclScope) -> String {
    match scope {
        AclScope::All => String::from("{\"kind\":\"all\"}"),
        AclScope::Prefix { kind, prefix } => {
            let mut out = String::from("{\"kind\":");
            out.push_str(&json_string(acl_scope_kind_str(*kind)));
            out.push_str(",\"prefix_hex\":");
            out.push_str(&json_string(&hex_bytes(prefix)));
            out.push('}');
            out
        }
    }
}

fn acl_scope_kind_str(kind: AclScopeKind) -> &'static str {
    match kind {
        AclScopeKind::Ref => "ref",
        AclScopeKind::Collection => "collection",
        AclScopeKind::Path => "path",
        AclScopeKind::Key => "key",
        AclScopeKind::Table => "table",
        AclScopeKind::Exec => "exec",
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn decode_hex(value: &str) -> PyResult<Vec<u8>> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    if !value.len().is_multiple_of(2) {
        return Err(PyRuntimeError::new_err(
            "hex input must have an even number of digits",
        ));
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> PyResult<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(PyRuntimeError::new_err(
            "hex input contains a non-hex digit",
        )),
    }
}

fn acl_list_json_inner(acl: &AclStore) -> String {
    let mut out = String::from("[");
    for (idx, grant) in acl.grants().iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&acl_grant_json(grant));
    }
    out.push(']');
    out
}

fn protected_ref_policy_json(ref_name: &str, policy: &ProtectedRefPolicy) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"ref\":");
    out.push_str(&json_string(ref_name));
    out.push_str(",\"fast_forward_only\":");
    out.push_str(if policy.fast_forward_only {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"signed_commits_required\":");
    out.push_str(if policy.signed_commits_required {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"signed_ref_advance_required\":");
    out.push_str(if policy.signed_ref_advance_required {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"required_review_count\":");
    out.push_str(&policy.required_review_count.to_string());
    out.push_str(",\"retention_lock\":");
    out.push_str(if policy.retention_lock {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"governance_lock\":");
    out.push_str(if policy.governance_lock {
        "true"
    } else {
        "false"
    });
    out.push('}');
    out
}

fn protected_ref_policies_json(policies: &[(String, ProtectedRefPolicy)]) -> String {
    let mut out = String::from("[");
    for (idx, (ref_name, policy)) in policies.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&protected_ref_policy_json(ref_name, policy));
    }
    out.push(']');
    out
}

fn optional_workspace_arg(
    loom: &Loom<FileStore>,
    workspace: Option<&str>,
) -> PyResult<Option<WorkspaceId>> {
    match workspace {
        Some(value) if !value.is_empty() => resolve_workspace_arg(loom, value).map(Some),
        _ => Ok(None),
    }
}

fn optional_acl_domain_arg(domain: Option<&str>) -> PyResult<Option<loom_core::AclDomain>> {
    match domain {
        Some(value) if !value.is_empty() => {
            loom_core::AclDomain::parse(value).map(Some).map_err(py_err)
        }
        _ => Ok(None),
    }
}

fn parse_acl_scope_kind(value: &str) -> PyResult<AclScopeKind> {
    match value {
        "ref" => Ok(AclScopeKind::Ref),
        "collection" => Ok(AclScopeKind::Collection),
        "path" => Ok(AclScopeKind::Path),
        "key" => Ok(AclScopeKind::Key),
        "table" => Ok(AclScopeKind::Table),
        "exec" => Ok(AclScopeKind::Exec),
        other => Err(PyRuntimeError::new_err(format!(
            "unknown acl scope kind {other:?}"
        ))),
    }
}

fn parse_acl_scope(value: &str) -> PyResult<AclScope> {
    let Some((kind, prefix)) = value.split_once(':') else {
        return Err(PyRuntimeError::new_err(format!(
            "acl scope {value:?} must be KIND:PREFIX"
        )));
    };
    Ok(AclScope::Prefix {
        kind: parse_acl_scope_kind(kind)?,
        prefix: prefix.as_bytes().to_vec(),
    })
}

fn parse_acl_scopes(scopes: Option<Vec<String>>) -> PyResult<Vec<AclScope>> {
    match scopes {
        Some(values) if !values.is_empty() => values
            .iter()
            .map(|value| parse_acl_scope(value))
            .collect::<PyResult<Vec<_>>>(),
        _ => Ok(vec![AclScope::All]),
    }
}

fn acl_grant_from_args(
    loom: &Loom<FileStore>,
    effect: i32,
    subject: &str,
    workspace: Option<&str>,
    domain: Option<&str>,
    rights_mask: u32,
    ref_glob: Option<&str>,
    scopes: Option<Vec<String>>,
    predicate_cel: Option<&str>,
) -> PyResult<AclGrant> {
    Ok(AclGrant {
        subject: parse_acl_subject(subject)?,
        workspace: optional_workspace_arg(loom, workspace)?,
        domain: optional_acl_domain_arg(domain)?,
        ref_glob: ref_glob
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        scopes: parse_acl_scopes(scopes)?,
        rights: parse_acl_rights(rights_mask)?.into_iter().collect(),
        effect: parse_acl_effect(effect)?,
        predicate: optional_acl_predicate(predicate_cel)?,
    })
}

fn optional_acl_predicate(value: Option<&str>) -> PyResult<Option<AclPredicate>> {
    value
        .filter(|value| !value.is_empty())
        .map(AclPredicate::cel)
        .transpose()
        .map_err(py_err)
}

/// Create a fresh `.loom` under an identity profile, optionally encrypted - the Python counterpart of
/// `loom init [--identity-profile fips] [--encrypt]`. `profile` is
/// `"default"`/`"blake3"` or `"fips"`/`"sha256"`; a `None`/empty `passphrase` makes an unencrypted store,
/// otherwise the DEK is wrapped under it with `suite` (or the profile default) as the AEAD. Raises if a
/// non-empty file already exists.
#[pyfunction]
#[pyo3(signature = (path, profile, suite=None, passphrase=None))]
pub(crate) fn create_loom(
    path: &str,
    profile: &str,
    suite: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let digest_algo = parse_profile(profile)?;
    let store = match passphrase.filter(|p| !p.is_empty()) {
        None => FileStore::create_with_profile(path, digest_algo).map_err(py_err)?,
        Some(pass) => {
            let suite = match suite {
                Some(s) => Suite::parse(s).map_err(py_err)?,
                None if digest_algo == Algo::Sha256 => Suite::Aes256Gcm,
                None => Suite::XChaCha20Poly1305,
            };
            let mut salt = [0u8; 16];
            let mut dek = [0u8; loom_core::keys::KEY_LEN];
            let mut wrap_nonce = [0u8; 24];
            rng_fill(&mut salt)?;
            rng_fill(&mut dek)?;
            rng_fill(&mut wrap_nonce)?;
            let (meta, session) = EncryptionMeta::create(
                &KeySpec::passphrase(pass),
                suite,
                salt.to_vec(),
                dek,
                wrap_nonce.to_vec(),
            )
            .map_err(py_err)?;
            FileStore::create_encrypted_with_profile(path, meta.encode(), session, digest_algo)
                .map_err(py_err)?
        }
    };
    let root = random_workspace_id()?;
    store
        .save_identity_store(&IdentityStore::new(root))
        .map_err(py_err)?;
    let mut acl = AclStore::new();
    acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])
        .map_err(py_err)?;
    store.save_acl_store(&acl).map_err(py_err)?;
    Ok(())
}

#[pyfunction]
#[pyo3(signature = (path, principal, principal_passphrase, passphrase=None))]
pub(crate) fn authenticate_passphrase(
    path: &str,
    principal: &str,
    principal_passphrase: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    open_control_read(
        path,
        passphrase,
        Some(principal),
        Some(principal_passphrase),
    )
    .map(|_| ())
}

#[pyfunction]
#[pyo3(signature = (path, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn identity_list_json(
    path: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<String> {
    let loom = open_control_read(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let identity = loom
        .identity_store()
        .ok_or_else(|| PyRuntimeError::new_err("identity store not initialized"))?;
    Ok(identity_list_json_inner(identity))
}

#[pyfunction]
#[pyo3(signature = (path, principal_handle, name, kind, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn identity_add_principal(
    path: &str,
    principal_handle: &str,
    name: &str,
    kind: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let kind = parse_principal_kind(kind)?;
    let id = random_workspace_id()?;
    let snapshot = {
        let identity = loom
            .identity_store_mut()
            .ok_or_else(|| PyRuntimeError::new_err("identity store not initialized"))?;
        identity
            .add_principal_with_handle(id, principal_handle, name, kind)
            .map_err(py_err)?;
        identity.clone()
    };
    loom.store()
        .save_identity_store(&snapshot)
        .map_err(py_err)?;
    Ok(id.to_string())
}

#[pyfunction]
#[pyo3(signature = (path, principal, principal_handle, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn identity_rename_principal_handle(
    path: &str,
    principal: &str,
    principal_handle: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let principal = WorkspaceId::parse(principal).map_err(py_err)?;
    let snapshot = {
        let identity = loom
            .identity_store_mut()
            .ok_or_else(|| PyRuntimeError::new_err("identity store not initialized"))?;
        identity
            .rename_principal_handle(principal, principal_handle)
            .map_err(py_err)?;
        identity.clone()
    };
    loom.store().save_identity_store(&snapshot).map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, principal, principal_passphrase, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn identity_set_passphrase(
    path: &str,
    principal: &str,
    principal_passphrase: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let principal = WorkspaceId::parse(principal).map_err(py_err)?;
    let mut salt = [0u8; 16];
    rng_fill(&mut salt)?;
    let snapshot = {
        let identity = loom
            .identity_store_mut()
            .ok_or_else(|| PyRuntimeError::new_err("identity store not initialized"))?;
        identity
            .set_passphrase(principal, principal_passphrase, &salt)
            .map_err(py_err)?;
        identity.clone()
    };
    loom.store().save_identity_store(&snapshot).map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, principal, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn identity_remove_principal(
    path: &str,
    principal: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let principal = WorkspaceId::parse(principal).map_err(py_err)?;
    let snapshot = {
        let identity = loom
            .identity_store_mut()
            .ok_or_else(|| PyRuntimeError::new_err("identity store not initialized"))?;
        identity.remove_principal(principal).map_err(py_err)?;
        identity.clone()
    };
    loom.store().save_identity_store(&snapshot).map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, principal, role, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn identity_assign_role(
    path: &str,
    principal: &str,
    role: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let principal = WorkspaceId::parse(principal).map_err(py_err)?;
    let role = WorkspaceId::parse(role).map_err(py_err)?;
    let snapshot = {
        let identity = loom
            .identity_store_mut()
            .ok_or_else(|| PyRuntimeError::new_err("identity store not initialized"))?;
        identity.assign_role(principal, role).map_err(py_err)?;
        identity.clone()
    };
    loom.store().save_identity_store(&snapshot).map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, principal, role, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn identity_revoke_role(
    path: &str,
    principal: &str,
    role: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let principal = WorkspaceId::parse(principal).map_err(py_err)?;
    let role = WorkspaceId::parse(role).map_err(py_err)?;
    let (removed, snapshot) = {
        let identity = loom
            .identity_store_mut()
            .ok_or_else(|| PyRuntimeError::new_err("identity store not initialized"))?;
        let removed = identity.revoke_role(principal, role).map_err(py_err)?;
        (removed, identity.clone())
    };
    if removed {
        loom.store()
            .save_identity_store(&snapshot)
            .map_err(py_err)?;
    }
    Ok(removed)
}

#[pyfunction]
#[pyo3(signature = (path, principal, kind, label, issuer, subject, material_digest=None, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn identity_create_external_credential(
    path: &str,
    principal: &str,
    kind: &str,
    label: &str,
    issuer: &str,
    subject: &str,
    material_digest: Option<&str>,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let principal = WorkspaceId::parse(principal).map_err(py_err)?;
    let kind = parse_external_credential_kind(kind)?;
    let id = random_workspace_id()?;
    let snapshot = {
        let identity = loom
            .identity_store_mut()
            .ok_or_else(|| PyRuntimeError::new_err("identity store not initialized"))?;
        identity
            .create_external_credential(
                principal,
                ExternalCredentialSpec {
                    id,
                    kind,
                    label: label.to_string(),
                    issuer: issuer.to_string(),
                    subject: subject.to_string(),
                    material_digest: material_digest.map(str::to_string),
                },
            )
            .map_err(py_err)?;
        identity.clone()
    };
    loom.store()
        .save_identity_store(&snapshot)
        .map_err(py_err)?;
    Ok(id.to_string())
}

#[pyfunction]
#[pyo3(signature = (path, credential, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn identity_revoke_external_credential(
    path: &str,
    credential: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let credential = WorkspaceId::parse(credential).map_err(py_err)?;
    let snapshot = {
        let identity = loom
            .identity_store_mut()
            .ok_or_else(|| PyRuntimeError::new_err("identity store not initialized"))?;
        identity
            .revoke_external_credential(credential)
            .map_err(py_err)?;
        identity.clone()
    };
    loom.store().save_identity_store(&snapshot).map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, principal, label, algorithm, public_key_hex, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn identity_add_public_key(
    path: &str,
    principal: &str,
    label: &str,
    algorithm: &str,
    public_key_hex: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let principal = WorkspaceId::parse(principal).map_err(py_err)?;
    let id = random_workspace_id()?;
    let public_key = decode_hex(public_key_hex)?;
    let snapshot = {
        let identity = loom
            .identity_store_mut()
            .ok_or_else(|| PyRuntimeError::new_err("identity store not initialized"))?;
        identity
            .add_public_key(
                principal,
                IdentityPublicKeySpec {
                    id,
                    label: label.to_string(),
                    algorithm: algorithm.to_string(),
                    public_key,
                },
            )
            .map_err(py_err)?;
        identity.clone()
    };
    loom.store()
        .save_identity_store(&snapshot)
        .map_err(py_err)?;
    Ok(id.to_string())
}

#[pyfunction]
#[pyo3(signature = (path, key, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn identity_revoke_public_key(
    path: &str,
    key: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let key = WorkspaceId::parse(key).map_err(py_err)?;
    let snapshot = {
        let identity = loom
            .identity_store_mut()
            .ok_or_else(|| PyRuntimeError::new_err("identity store not initialized"))?;
        identity.revoke_public_key(key).map_err(py_err)?;
        identity.clone()
    };
    loom.store().save_identity_store(&snapshot).map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn acl_list_json(
    path: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<String> {
    let loom = open_control_read(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    Ok(acl_list_json_inner(loom.acl_store()))
}

#[pyfunction]
#[pyo3(signature = (path, effect, subject, workspace=None, domain=None, rights_mask=0, passphrase=None, auth_principal=None, auth_passphrase=None, predicate_cel=None))]
pub(crate) fn acl_grant(
    path: &str,
    effect: i32,
    subject: &str,
    workspace: Option<&str>,
    domain: Option<&str>,
    rights_mask: u32,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
    predicate_cel: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let grant = acl_grant_from_args(
        &loom,
        effect,
        subject,
        workspace,
        domain,
        rights_mask,
        None,
        None,
        predicate_cel,
    )?;
    let snapshot = {
        let acl = loom.acl_store_mut();
        acl.grant(grant).map_err(py_err)?;
        acl.clone()
    };
    loom.store().save_acl_store(&snapshot).map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, effect, subject, workspace=None, domain=None, rights_mask=0, passphrase=None, auth_principal=None, auth_passphrase=None, predicate_cel=None))]
pub(crate) fn acl_revoke(
    path: &str,
    effect: i32,
    subject: &str,
    workspace: Option<&str>,
    domain: Option<&str>,
    rights_mask: u32,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
    predicate_cel: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let grant = acl_grant_from_args(
        &loom,
        effect,
        subject,
        workspace,
        domain,
        rights_mask,
        None,
        None,
        predicate_cel,
    )?;
    let (removed, snapshot) = {
        let acl = loom.acl_store_mut();
        let removed = acl.revoke(&grant);
        (removed, acl.clone())
    };
    if removed {
        loom.store().save_acl_store(&snapshot).map_err(py_err)?;
    }
    Ok(removed)
}

#[pyfunction]
#[pyo3(signature = (path, effect, subject, workspace=None, domain=None, rights_mask=0, ref_glob=None, scopes=None, passphrase=None, auth_principal=None, auth_passphrase=None, predicate_cel=None))]
pub(crate) fn acl_grant_scoped(
    path: &str,
    effect: i32,
    subject: &str,
    workspace: Option<&str>,
    domain: Option<&str>,
    rights_mask: u32,
    ref_glob: Option<&str>,
    scopes: Option<Vec<String>>,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
    predicate_cel: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let grant = acl_grant_from_args(
        &loom,
        effect,
        subject,
        workspace,
        domain,
        rights_mask,
        ref_glob,
        scopes,
        predicate_cel,
    )?;
    let snapshot = {
        let acl = loom.acl_store_mut();
        acl.grant(grant).map_err(py_err)?;
        acl.clone()
    };
    loom.store().save_acl_store(&snapshot).map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, effect, subject, workspace=None, domain=None, rights_mask=0, ref_glob=None, scopes=None, passphrase=None, auth_principal=None, auth_passphrase=None, predicate_cel=None))]
pub(crate) fn acl_revoke_scoped(
    path: &str,
    effect: i32,
    subject: &str,
    workspace: Option<&str>,
    domain: Option<&str>,
    rights_mask: u32,
    ref_glob: Option<&str>,
    scopes: Option<Vec<String>>,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
    predicate_cel: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    loom.authorize_global_admin().map_err(py_err)?;
    let grant = acl_grant_from_args(
        &loom,
        effect,
        subject,
        workspace,
        domain,
        rights_mask,
        ref_glob,
        scopes,
        predicate_cel,
    )?;
    let (removed, snapshot) = {
        let acl = loom.acl_store_mut();
        let removed = acl.revoke(&grant);
        (removed, acl.clone())
    };
    if removed {
        loom.store().save_acl_store(&snapshot).map_err(py_err)?;
    }
    Ok(removed)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn protected_ref_list_json(
    path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<String> {
    let loom = open_control_read(path, passphrase, auth_principal, auth_passphrase)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(protected_ref_policies_json(
        &loom.protected_ref_policies(ns).map_err(py_err)?,
    ))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ref_name, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn protected_ref_get_json(
    path: &str,
    workspace: &str,
    ref_name: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<String> {
    let loom = open_control_read(path, passphrase, auth_principal, auth_passphrase)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(
        match loom.protected_ref_policy(ns, ref_name).map_err(py_err)? {
            Some(policy) => protected_ref_policy_json(ref_name, &policy),
            None => "null".to_string(),
        },
    )
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ref_name, fast_forward_only, signed_commits_required, signed_ref_advance_required, required_review_count, retention_lock, governance_lock, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn protected_ref_set(
    path: &str,
    workspace: &str,
    ref_name: &str,
    fast_forward_only: bool,
    signed_commits_required: bool,
    signed_ref_advance_required: bool,
    required_review_count: u32,
    retention_lock: bool,
    governance_lock: bool,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    let actor = loom.effective_principal().map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom.set_protected_ref_policy(
        ns,
        ref_name,
        ProtectedRefPolicy {
            fast_forward_only,
            signed_commits_required,
            signed_ref_advance_required,
            required_review_count,
            retention_lock,
            governance_lock,
        },
    )
    .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    let target = format!("workspace={ns};ref={ref_name}");
    loom.store()
        .audit_append(actor, "protected_ref.set", Some(&target))
        .map(|_| ())
        .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, ref_name, passphrase=None, auth_principal=None, auth_passphrase=None))]
pub(crate) fn protected_ref_remove(
    path: &str,
    workspace: &str,
    ref_name: &str,
    passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_control_write(path, passphrase, auth_principal, auth_passphrase)?;
    let actor = loom.effective_principal().map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let removed = loom
        .remove_protected_ref_policy(ns, ref_name)
        .map_err(py_err)?;
    if removed {
        save_loom(&mut loom).map_err(py_err)?;
        let target = format!("workspace={ns};ref={ref_name}");
        loom.store()
            .audit_append(actor, "protected_ref.remove", Some(&target))
            .map_err(py_err)?;
    }
    Ok(removed)
}
/// The library version.
#[pyfunction]
pub(crate) fn version() -> String {
    loom_core::VERSION.to_string()
}
/// The build capability report (0010 section 5) as canonical CBOR: a `CapabilitySet` map with
/// `schema_version` and `records`. Build-aware: capabilities owned by the linked crates are reported
/// with operational state `supported`. Mirrors the C ABI `loom_capabilities`.
#[pyfunction]
pub(crate) fn capabilities(py: Python<'_>) -> Bound<'_, PyBytes> {
    let set = loom_core::capability::registry()
        .with_state_overlay(
            loom_store::provided_capabilities(),
            loom_core::CapabilityOperationalState::Supported,
        )
        .with_state_overlay(
            loom_sql::provided_capabilities(),
            loom_core::CapabilityOperationalState::Supported,
        )
        .with_state_overlay(
            loom_lanes::provided_capabilities(),
            loom_core::CapabilityOperationalState::Supported,
        );
    PyBytes::new(py, &set.to_cbor())
}
/// The runtime provider/profile report as canonical CBOR.
#[pyfunction]
pub(crate) fn runtime_profile(py: Python<'_>) -> Bound<'_, PyBytes> {
    PyBytes::new(py, &loom_core::runtime_profile().to_cbor())
}
#[pyfunction]
#[pyo3(signature = (workspace, set="all"))]
pub(crate) fn studio_surface_catalog_json(workspace: &str, set: &str) -> PyResult<String> {
    loom_substrate::surfaces::surface_catalog_json(workspace, set).map_err(py_err)
}
/// Compute the Blob content address (`"algo:hex"`) of the given bytes.
#[pyfunction]
pub(crate) fn blob_digest(data: &[u8]) -> String {
    Object::Blob(data.to_vec()).digest().to_string()
}
/// Add a passphrase unlock wrap to an encrypted store, unlocking it with the existing `passphrase`.
/// `allow_no_recovery` permits leaving no passphrase recovery wrap. The host supplies both secrets.
#[pyfunction]
#[pyo3(signature = (path, passphrase, new_passphrase, allow_no_recovery=false))]
pub(crate) fn key_add_wrap_keyed(
    path: &str,
    passphrase: &str,
    new_passphrase: &str,
    allow_no_recovery: bool,
) -> PyResult<()> {
    let fs = open_store_for_key_update(path, passphrase)?;
    let (salt, wrap_nonce) = fresh_wrap_material()?;
    fs.add_wrap(
        &KeySpec::passphrase(new_passphrase),
        salt.to_vec(),
        wrap_nonce.to_vec(),
        allow_no_recovery,
    )
    .map_err(py_err)
}
/// Add a host-supplied 256-bit raw-KEK unlock wrap to an encrypted store, unlocking it with
/// `passphrase`. `kek` must be exactly 32 bytes; the host acquires it securely.
#[pyfunction]
#[pyo3(signature = (path, passphrase, kek, allow_no_recovery=false))]
pub(crate) fn key_add_wrap_with_kek(
    path: &str,
    passphrase: &str,
    kek: &[u8],
    allow_no_recovery: bool,
) -> PyResult<()> {
    let spec = kek_spec(kek)?;
    let fs = open_store_for_key_update(path, passphrase)?;
    let (salt, wrap_nonce) = fresh_wrap_material()?;
    fs.add_wrap(&spec, salt.to_vec(), wrap_nonce.to_vec(), allow_no_recovery)
        .map_err(py_err)
}
/// Remove one unlock wrap by zero-based `index` from an encrypted store, unlocking it with
/// `passphrase`. `allow_no_recovery` permits removing the last passphrase recovery wrap.
#[pyfunction]
#[pyo3(signature = (path, passphrase, index, allow_no_recovery=false))]
pub(crate) fn key_remove_wrap(
    path: &str,
    passphrase: &str,
    index: usize,
    allow_no_recovery: bool,
) -> PyResult<()> {
    let fs = open_store_for_key_update(path, passphrase)?;
    fs.remove_wrap(index, allow_no_recovery).map_err(py_err)
}
/// Render a canonical-CBOR result buffer (from a SQL exec or a direct reader) to debug JSON. This is a
/// debug/inspection view, not the typed API: faithful cells decode to type-tagged scalars such as
/// `{"Int":1}` and `{"Text":"hi"}`. Mirrors the C ABI `loom_result_to_json`.
#[pyfunction]
pub(crate) fn result_to_json(bytes: &[u8]) -> PyResult<String> {
    loom_result::result_to_json(bytes).map_err(py_err)
}
/// Render a canonical-CBOR result buffer to lossless bridge JSON, the React Native bridge projection.
/// This is not the normative wire form and not the general typed API: values a JSON number cannot hold
/// exactly cross as tagged `$`-prefixed objects. Mirrors the C ABI `loom_result_to_bridge_json`.
#[pyfunction]
pub(crate) fn result_to_bridge_json(bytes: &[u8]) -> PyResult<String> {
    loom_result::to_bridge_json(bytes).map_err(py_err)
}
