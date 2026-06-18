//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// LoomSession - the direct, non-SQL engine surface (B2): version-control verbs and table inspection.
// ---------------------------------------------------------------------------------------------------

/// An open Loom: a reopenable handle to a `.loom` path. Like [`LoomSqlSession`], it does **not** hold
/// the loom (or its lock) between calls - each op opens the loom for its duration: read ops use the
/// lock-free read path (concurrent readers never block), write ops take the exclusive write lock only
/// for that op. Opaque to C; create with [`loom_open`], free with [`loom_close`].
pub struct LoomSession {
    path: String,
    /// Unlock passphrase for an encrypted loom, or `None`. Like [`LoomSqlSession`], the
    /// handle reopens the loom per op, so it holds the key for the handle's lifetime; the host supplies
    /// it (no environment variable is consulted).
    key: Option<KeySpec>,
    session_id: Option<String>,
    session_principal: Option<WorkspaceId>,
}

/// Reopen the handle's loom for write, unlocking it if encrypted.
pub(crate) fn open_h_write(h: &LoomSession) -> LoomResult<Loom<FileStore>> {
    attach_session_state(h, open_loom_unlocked(&h.path, h.key.as_ref())?)
}

/// Reopen the handle's loom read-only and lock-free, unlocking it if encrypted.
pub(crate) fn open_h_read(h: &LoomSession) -> LoomResult<Loom<FileStore>> {
    attach_session_state(h, open_loom_read_unlocked(&h.path, h.key.as_ref())?)
}

fn attach_session_state(h: &LoomSession, mut loom: Loom<FileStore>) -> LoomResult<Loom<FileStore>> {
    if let Some(mut identity) = loom.store().identity_store()? {
        if let (Some(session_id), Some(principal)) = (h.session_id.as_deref(), h.session_principal)
        {
            identity.bind_session(principal, session_id)?;
            loom.set_session(session_id.to_string());
        }
        loom.set_identity_store(identity);
    }
    if let Some(acl) = loom.store().acl_store()? {
        loom.set_acl_store(acl);
    }
    Ok(loom)
}

pub(crate) fn task_handle(h: &LoomSession) -> LoomSession {
    LoomSession {
        path: h.path.clone(),
        key: h.key.clone(),
        session_id: h.session_id.clone(),
        session_principal: h.session_principal,
    }
}

fn resolve_ns(loom: &Loom<FileStore>, name: &str) -> LoomResult<WorkspaceId> {
    // Workspace names are unique across the loom, so a name or UUID identifies a workspace on its own.
    let selector = match WorkspaceId::parse(name) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Name(name.to_string()),
    };
    loom.registry().open(&selector)
}

pub(crate) fn resolve_workspace_arg(
    loom: &Loom<FileStore>,
    workspace: &str,
) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Name(workspace.to_string()),
    };
    loom.registry().open(&selector)
}

pub(crate) fn random_workspace_id() -> LoomResult<WorkspaceId> {
    let mut id = [0u8; 16];
    rng_fill(&mut id)?;
    Ok(WorkspaceId::v4_from_bytes(id))
}

pub(crate) fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
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

unsafe fn optional_arg_string(ptr: *const c_char, what: &str) -> LoomResult<Option<String>> {
    if ptr.is_null() {
        return Ok(None);
    }
    unsafe { cstr(ptr) }
        .map(|value| Some(value.to_string()))
        .ok_or_else(|| LoomError::invalid(format!("{what}: non-UTF-8 argument")))
}

fn parse_watch_change_kinds(value: Option<&str>) -> LoomResult<Vec<ChangeKind>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if value.is_empty() {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(|kind| match kind.trim() {
            "added" => Ok(ChangeKind::Added),
            "modified" => Ok(ChangeKind::Modified),
            "deleted" => Ok(ChangeKind::Deleted),
            other => Err(LoomError::invalid(format!(
                "unknown watch change kind {other:?}"
            ))),
        })
        .collect()
}

fn exec_error(err: loom_compute::ExecError) -> LoomError {
    LoomError::new(err.code(), err.to_string())
}

/// Execute a canonical `loom.exec.request.v1` CBOR request and return a canonical
/// `loom.exec.result.v1` CBOR response. The result buffer is owned by the caller and must be freed
/// with [`loom_bytes_free`].
///
/// # Safety
/// `handle` must be from [`loom_open`]; `request` must point to `request_len` readable bytes;
/// `out_ptr`/`out_len` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_exec_cbor(
    handle: *mut LoomSession,
    request: *const c_uchar,
    request_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_exec_cbor");
    let request = if request_len == 0 {
        &[]
    } else if request.is_null() {
        return fail_arg("loom_exec_cbor: null request");
    } else {
        // SAFETY: caller guarantees `request` is readable for `request_len` bytes (see fn docs).
        unsafe { core::slice::from_raw_parts(request, request_len) }
    };
    match (|| {
        let mut loom = open_h_write(h)?;
        let bytes = loom_compute::execute_cbor(&mut loom, request).map_err(exec_error)?;
        save_loom(&mut loom)?;
        Ok::<Vec<u8>, LoomError>(bytes)
    })() {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

fn principal_kind_str(kind: PrincipalKind) -> &'static str {
    match kind {
        PrincipalKind::Root => "root",
        PrincipalKind::User => "user",
        PrincipalKind::Service => "service",
    }
}

fn parse_principal_kind(value: &str) -> LoomResult<PrincipalKind> {
    match value {
        "root" => Ok(PrincipalKind::Root),
        "user" => Ok(PrincipalKind::User),
        "service" => Ok(PrincipalKind::Service),
        other => Err(LoomError::invalid(format!(
            "unknown principal kind {other:?}"
        ))),
    }
}

fn acl_effect_str(effect: AclEffect) -> &'static str {
    match effect {
        AclEffect::Allow => "allow",
        AclEffect::Deny => "deny",
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

fn parse_acl_effect(value: i32) -> LoomResult<AclEffect> {
    match value {
        0 => Ok(AclEffect::Allow),
        1 => Ok(AclEffect::Deny),
        other => Err(LoomError::invalid(format!("unknown acl effect {other}"))),
    }
}

fn parse_acl_rights(mask: u32) -> LoomResult<Vec<AclRight>> {
    const KNOWN: u32 = 0x3f;
    if mask == 0 {
        return Err(LoomError::invalid("acl rights mask must not be empty"));
    }
    if mask & !KNOWN != 0 {
        return Err(LoomError::invalid(format!(
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

fn parse_acl_subject(value: &str) -> LoomResult<AclSubject> {
    match value {
        "*" | "everyone" => Ok(AclSubject::Everyone),
        role if role.starts_with("role:") => Ok(AclSubject::Role(WorkspaceId::parse(&role[5..])?)),
        other => Ok(AclSubject::Principal(WorkspaceId::parse(other)?)),
    }
}

fn parse_acl_scope_kind(value: i32) -> LoomResult<AclScopeKind> {
    match value {
        0 => Ok(AclScopeKind::Ref),
        1 => Ok(AclScopeKind::Collection),
        2 => Ok(AclScopeKind::Path),
        3 => Ok(AclScopeKind::Key),
        4 => Ok(AclScopeKind::Table),
        5 => Ok(AclScopeKind::Exec),
        other => Err(LoomError::invalid(format!(
            "unknown acl scope kind {other}"
        ))),
    }
}

unsafe fn acl_scopes_from_raw(
    scope_count: usize,
    scope_kinds: *const i32,
    scope_prefixes: *const *const c_uchar,
    scope_prefix_lens: *const usize,
    fn_name: &str,
) -> LoomResult<Vec<AclScope>> {
    if scope_count == 0 {
        return Ok(vec![AclScope::All]);
    }
    if scope_kinds.is_null() || scope_prefixes.is_null() || scope_prefix_lens.is_null() {
        return Err(LoomError::invalid(format!(
            "{fn_name}: scoped ACL arrays are required when scope_count is nonzero"
        )));
    }
    let kinds = unsafe { core::slice::from_raw_parts(scope_kinds, scope_count) };
    let prefixes = unsafe { core::slice::from_raw_parts(scope_prefixes, scope_count) };
    let lens = unsafe { core::slice::from_raw_parts(scope_prefix_lens, scope_count) };
    let mut out = Vec::with_capacity(scope_count);
    for idx in 0..scope_count {
        let len = lens[idx];
        let prefix = if len == 0 {
            Vec::new()
        } else {
            let ptr = prefixes[idx];
            if ptr.is_null() {
                return Err(LoomError::invalid(format!(
                    "{fn_name}: scope prefix pointer {idx} is null"
                )));
            }
            unsafe { core::slice::from_raw_parts(ptr, len) }.to_vec()
        };
        out.push(AclScope::Prefix {
            kind: parse_acl_scope_kind(kinds[idx])?,
            prefix,
        });
    }
    Ok(out)
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

fn identity_list_json(identity: &IdentityStore) -> String {
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
    out.push_str(",\"authority\":");
    out.push_str(&identity_authority_state_json(identity.authority_state()));
    out.push_str(",\"authority_handoffs\":[");
    for (idx, handoff) in identity.authority_handoffs().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&identity_authority_handoff_json(handoff));
    }
    out.push_str("],\"forced_detach\":");
    match identity.forced_detach() {
        Some(detach) => out.push_str(&identity_authority_detach_json(detach)),
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

fn identity_authority_state_json(state: &loom_core::IdentityAuthorityState) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"mode\":");
    out.push_str(&json_string(identity_authority_mode_str(state.mode)));
    out.push_str(",\"authority\":");
    out.push_str(&json_string(&state.authority.to_string()));
    out.push_str(",\"generation\":");
    out.push_str(&state.generation.to_string());
    out.push_str(",\"head\":");
    match state.head {
        Some(head) => out.push_str(&json_string(&head.to_string())),
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

fn identity_authority_handoff_json(handoff: &loom_core::IdentityAuthorityHandoff) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"from\":");
    out.push_str(&json_string(&handoff.from.to_string()));
    out.push_str(",\"to\":");
    out.push_str(&json_string(&handoff.to.to_string()));
    out.push_str(",\"generation\":");
    out.push_str(&handoff.generation.to_string());
    out.push_str(",\"head\":");
    match handoff.head {
        Some(head) => out.push_str(&json_string(&head.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"signed_record_hex\":");
    out.push_str(&json_string(&hex_bytes(&handoff.signed_record)));
    out.push('}');
    out
}

fn identity_authority_detach_json(detach: &loom_core::IdentityAuthorityDetach) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"previous_authority\":");
    out.push_str(&json_string(&detach.previous_authority.to_string()));
    out.push_str(",\"new_authority\":");
    out.push_str(&json_string(&detach.new_authority.to_string()));
    out.push_str(",\"generation\":");
    out.push_str(&detach.generation.to_string());
    out.push_str(",\"reason\":");
    out.push_str(&json_string(&detach.reason));
    out.push('}');
    out
}

fn identity_authority_mode_str(mode: loom_core::IdentityAuthorityMode) -> &'static str {
    match mode {
        loom_core::IdentityAuthorityMode::Authority => "authority",
        loom_core::IdentityAuthorityMode::Mirror => "mirror",
        loom_core::IdentityAuthorityMode::Detached => "detached",
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
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
    out.push_str(&acl_predicate_json(grant.predicate.as_ref()));
    out.push('}');
    out
}

fn acl_predicate_json(predicate: Option<&loom_core::AclPredicate>) -> String {
    match predicate {
        None => String::from("null"),
        Some(predicate) => {
            let mut out = String::from("{\"language\":");
            out.push_str(&json_string(&predicate.language));
            out.push_str(",\"expression\":");
            out.push_str(&json_string(&predicate.expression));
            out.push('}');
            out
        }
    }
}

fn acl_scope_json(scope: &AclScope) -> String {
    match scope {
        AclScope::All => String::from("{\"kind\":\"all\"}"),
        AclScope::Prefix { kind, prefix } => {
            let mut out = String::from("{\"kind\":");
            out.push_str(&json_string(acl_scope_kind_str(*kind)));
            out.push_str(",\"prefix_hex\":");
            out.push_str(&json_string(&hex::encode(prefix)));
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

fn acl_list_json(acl: &AclStore) -> String {
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

fn workspace_list_json(loom: &Loom<FileStore>) -> String {
    let records = loom.registry().list(None);
    let mut out = String::from("[");
    for (i, ns) in records.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        out.push_str(&json_string(&ns.id.to_string()));
        out.push_str(",\"name\":");
        out.push_str(&json_string(&ns.name));
        out.push_str(",\"facets\":[");
        for (j, facet) in ns.facets.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str(&json_string(facet.as_str()));
        }
        out.push_str("],\"head\":");
        match ns.head {
            Some(head) => out.push_str(&json_string(&head.to_string())),
            None => out.push_str("null"),
        }
        out.push('}');
    }
    out.push(']');
    out
}

pub(crate) fn commit_ns(
    h: &LoomSession,
    name: &str,
    author: &str,
    message: &str,
) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    let d = loom.commit(ns, author, message, now_ms())?;
    save_loom(&mut loom)?;
    Ok(d.to_string())
}

pub(crate) fn branch_ns(h: &LoomSession, name: &str, branch: &str) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.branch(ns, branch)?;
    save_loom(&mut loom)
}

pub(crate) fn checkout_ns(h: &LoomSession, name: &str, branch: &str) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.checkout_branch(ns, branch)?;
    save_loom(&mut loom)
}

pub(crate) fn log_ns(h: &LoomSession, name: &str, branch: &str) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    let commits = loom.log(ns, branch)?;
    result_cbor::commit_log_cbor(&commits)
}

pub(crate) fn merge_ns(
    h: &LoomSession,
    name: &str,
    from: &str,
    author: &str,
    cells: bool,
) -> LoomResult<Vec<u8>> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    let outcome = if cells {
        loom.merge_cell_level(ns, from, author, now_ms())
    } else {
        loom.merge(ns, from, author, now_ms())
    }?;
    save_loom(&mut loom)?;
    result_cbor::merge_outcome_cbor(&outcome)
}

/// Whether `name`'s `facet` workspace has a conflicted merge awaiting continue/abort.
pub(crate) fn merge_in_progress_ns(h: &LoomSession, name: &str) -> LoomResult<bool> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.merge_in_progress(ns)
}

/// The still-unresolved conflict paths of the in-progress merge as a JSON string array.
pub(crate) fn merge_conflicts_ns(h: &LoomSession, name: &str) -> LoomResult<String> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    let mut out = String::from("[");
    for (i, p) in loom.merge_conflicts(ns)?.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_string(p));
    }
    out.push(']');
    Ok(out)
}

/// Settle one conflicted path: `resolution` is 0 ours, 1 theirs, 2 working.
pub(crate) fn merge_resolve_ns(
    h: &LoomSession,
    name: &str,
    path: &str,
    resolution: i32,
) -> LoomResult<()> {
    let res = match resolution {
        0 => loom_core::ConflictResolution::Ours,
        1 => loom_core::ConflictResolution::Theirs,
        2 => loom_core::ConflictResolution::Working,
        other => {
            return Err(LoomError::invalid(format!(
                "unknown conflict resolution {other} (expected 0 ours, 1 theirs, 2 working)"
            )));
        }
    };
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.merge_resolve(ns, path, res)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Abandon the in-progress merge, restoring the pre-merge working tree.
pub(crate) fn merge_abort_ns(h: &LoomSession, name: &str) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.merge_abort(ns)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Finish the in-progress merge, returning the new merge commit's content address.
pub(crate) fn merge_continue_ns(h: &LoomSession, name: &str, author: &str) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    let digest = loom.merge_continue(ns, author, now_ms())?;
    save_loom(&mut loom)?;
    Ok(digest.to_string())
}

/// Stage one path into the workspace's shared index.
pub(crate) fn stage_ns(h: &LoomSession, name: &str, path: &str) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.stage(ns, &[path])?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Stage the whole working tree of a workspace into its shared index.
pub(crate) fn stage_all_ns(h: &LoomSession, name: &str) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.stage_all(ns)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Unstage one path, reverting its index entry to HEAD.
pub(crate) fn unstage_ns(h: &LoomSession, name: &str, path: &str) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.unstage(ns, &[path])?;
    save_loom(&mut loom)?;
    Ok(())
}

/// The stable label for a status change kind.
fn change_kind_str(kind: loom_core::ChangeKind) -> &'static str {
    match kind {
        loom_core::ChangeKind::Added => "added",
        loom_core::ChangeKind::Modified => "modified",
        loom_core::ChangeKind::Deleted => "deleted",
    }
}

/// Render a status change list as a JSON array of `{ "path", "kind" }`.
fn push_change_array(out: &mut String, changes: &[loom_core::Change]) {
    out.push('[');
    for (i, c) in changes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"path\":");
        out.push_str(&json_string(&c.path));
        out.push_str(",\"kind\":");
        out.push_str(&json_string(change_kind_str(c.kind)));
        out.push('}');
    }
    out.push(']');
}

/// Render a string list as a JSON array.
fn push_string_array(out: &mut String, items: &[String]) {
    out.push('[');
    for (i, s) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_string(s));
    }
    out.push(']');
}

/// The workspace status as JSON (`{ staged, unstaged, untracked, conflicts }`).
pub(crate) fn status_json_ns(h: &LoomSession, name: &str) -> LoomResult<String> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    let st = loom.status(ns)?;
    let mut out = String::from("{\"staged\":");
    push_change_array(&mut out, &st.staged);
    out.push_str(",\"unstaged\":");
    push_change_array(&mut out, &st.unstaged);
    out.push_str(",\"untracked\":");
    push_string_array(&mut out, &st.untracked);
    out.push_str(",\"conflicts\":");
    push_string_array(&mut out, &st.conflicts);
    out.push('}');
    Ok(out)
}

/// Commit only the staged index, returning the new commit's content address.
pub(crate) fn commit_staged_ns(
    h: &LoomSession,
    name: &str,
    author: &str,
    message: &str,
) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    let digest = loom.commit_staged(ns, author, message, now_ms())?;
    save_loom(&mut loom)?;
    Ok(digest.to_string())
}

/// Create-or-replace file `path` with `bytes` and `mode` (a `0` mode uses the default `0o100644`).
pub(crate) fn write_file_ns(
    h: &LoomSession,
    name: &str,
    path: &str,
    bytes: &[u8],
    mode: u32,
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    let mode = if mode == 0 { 0o100644 } else { mode };
    loom.write_file(ns, path, bytes, mode)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Read file `path`'s bytes from the workspace working tree.
pub(crate) fn read_file_ns(h: &LoomSession, name: &str, path: &str) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.read_file(ns, path)
}

/// Remove file `path` from the workspace working tree (a staged deletion).
pub(crate) fn remove_file_ns(h: &LoomSession, name: &str, path: &str) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.remove_file(ns, path)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Append `bytes` to file `path`, creating it if absent (the parent directory must exist).
pub(crate) fn append_file_ns(
    h: &LoomSession,
    name: &str,
    path: &str,
    bytes: &[u8],
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.append_file(ns, path, bytes)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Create a symbolic link at `link_path` whose target is `target` (opaque; may be dangling).
pub(crate) fn symlink_ns(
    h: &LoomSession,
    name: &str,
    target: &str,
    link_path: &str,
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.symlink(ns, target, link_path)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Read the target of the symbolic link at `path`.
pub(crate) fn read_link_ns(h: &LoomSession, name: &str, path: &str) -> LoomResult<String> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.read_link(ns, path)
}

/// Map the stable 1-byte open-mode code to [`OpenMode`] (0 Read, 1 Write, 2 ReadWrite, 3 Append).
fn open_mode_from_code(mode: u8) -> LoomResult<OpenMode> {
    Ok(match mode {
        0 => OpenMode::Read,
        1 => OpenMode::Write,
        2 => OpenMode::ReadWrite,
        3 => OpenMode::Append,
        other => {
            return Err(LoomError::new(
                Code::InvalidArgument,
                format!("unknown open mode {other}"),
            ));
        }
    })
}

/// Read `[offset, offset + len)` of file `path` (bounded chunk read).
pub(crate) fn read_at_ns(
    h: &LoomSession,
    name: &str,
    path: &str,
    offset: u64,
    len: u64,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.read_at(ns, path, offset, len)
}

/// Write `bytes` at `offset` of file `path`, creating it if absent and zero-filling any gap.
pub(crate) fn write_at_ns(
    h: &LoomSession,
    name: &str,
    path: &str,
    offset: u64,
    bytes: &[u8],
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.write_at(ns, path, offset, bytes)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Resize file `path` to `size` (zero-extend or drop), creating it if absent.
pub(crate) fn truncate_ns(h: &LoomSession, name: &str, path: &str, size: u64) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.truncate_file(ns, path, size)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Open a file handle on `path` with `mode`, returning the handle id (persisted in the open-file table).
pub(crate) fn file_open_ns(h: &LoomSession, name: &str, path: &str, mode: u8) -> LoomResult<u64> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    let handle = loom.file_open(ns, path, open_mode_from_code(mode)?)?;
    save_loom(&mut loom)?;
    Ok(handle)
}

/// Sequential read of up to `len` bytes from the handle's cursor (advances it).
pub(crate) fn file_read_ns(h: &LoomSession, handle: u64, len: u64) -> LoomResult<Vec<u8>> {
    let mut loom = open_h_write(h)?;
    let bytes = loom.file_read(handle, len)?;
    save_loom(&mut loom)?;
    Ok(bytes)
}

/// Positional read of up to `len` bytes at `offset` (does not move the cursor).
pub(crate) fn file_read_at_ns(
    h: &LoomSession,
    handle: u64,
    offset: u64,
    len: u64,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    loom.file_read_at(handle, offset, len)
}

/// Sequential write at the handle's cursor (or EOF for an append handle); advances it.
pub(crate) fn file_write_ns(h: &LoomSession, handle: u64, bytes: &[u8]) -> LoomResult<u64> {
    let mut loom = open_h_write(h)?;
    let n = loom.file_write(handle, bytes)?;
    save_loom(&mut loom)?;
    Ok(n)
}

/// Positional write of `bytes` at `offset` (does not move the cursor).
pub(crate) fn file_write_at_ns(
    h: &LoomSession,
    handle: u64,
    offset: u64,
    bytes: &[u8],
) -> LoomResult<u64> {
    let mut loom = open_h_write(h)?;
    let n = loom.file_write_at(handle, offset, bytes)?;
    save_loom(&mut loom)?;
    Ok(n)
}

/// Resize the handle's file to `size`.
pub(crate) fn file_truncate_ns(h: &LoomSession, handle: u64, size: u64) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    loom.file_truncate(handle, size)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Validate the handle (writes already apply per operation; durability is the per-op save).
pub(crate) fn file_flush_ns(h: &LoomSession, handle: u64) -> LoomResult<()> {
    let loom = open_h_read(h)?;
    loom.file_flush(handle)
}

/// The handle's live `(size, mode)`.
pub(crate) fn file_stat_ns(h: &LoomSession, handle: u64) -> LoomResult<(u64, u32)> {
    let loom = open_h_read(h)?;
    let st = loom.file_stat(handle)?;
    Ok((st.size, st.mode))
}

/// Close the handle (delete-on-last-close for an unlinked inode).
pub(crate) fn file_close_ns(h: &LoomSession, handle: u64) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    loom.file_close(handle)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Create tag `tag` at the commit `rev` resolves to. A non-empty `message` makes an annotated tag.
/// Returns the ref target digest (the commit, or the tag object).
pub(crate) fn tag_create_ns(
    h: &LoomSession,
    name: &str,
    tag: &str,
    rev: &str,
    tagger: &str,
    message: &str,
) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    let target = loom.tag_create(ns, tag, rev, tagger, message, now_ms())?;
    save_loom(&mut loom)?;
    Ok(target.to_string())
}

/// All tag names in the workspace as a JSON string array.
pub(crate) fn tag_list_ns(h: &LoomSession, name: &str) -> LoomResult<String> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    let mut out = String::from("[");
    for (i, t) in loom.tag_list(ns)?.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_string(t));
    }
    out.push(']');
    Ok(out)
}

/// The raw ref target of tag `tag` (commit for lightweight, tag object for annotated), or `None`.
pub(crate) fn tag_target_ns(h: &LoomSession, name: &str, tag: &str) -> LoomResult<Option<String>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    Ok(loom.tag_target(ns, tag)?.map(|d| d.to_string()))
}

/// Delete tag `tag` (NOT_FOUND if absent).
pub(crate) fn tag_delete_ns(h: &LoomSession, name: &str, tag: &str) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.tag_delete(ns, tag)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Rename tag `old` to `new`, preserving its target.
pub(crate) fn tag_rename_ns(h: &LoomSession, name: &str, old: &str, new: &str) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.tag_rename(ns, old, new)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Restore one path in the working tree to the snapshot `rev` resolves to (working tree only).
pub(crate) fn restore_file_ns(
    h: &LoomSession,
    name: &str,
    rev: &str,
    path: &str,
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.restore_file(ns, rev, path)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Restore the subtree under `prefix` to the snapshot `rev` resolves to (path-restricted checkout).
pub(crate) fn restore_path_ns(
    h: &LoomSession,
    name: &str,
    rev: &str,
    prefix: &str,
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    loom.restore_path(ns, rev, prefix)?;
    save_loom(&mut loom)?;
    Ok(())
}

/// Parse a comma-separated list of commit digests (`algo:hex`), skipping blanks.
fn parse_commit_list(s: &str) -> LoomResult<Vec<Digest>> {
    s.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(Digest::parse)
        .collect()
}

/// Render a [`ReplayOutcome`] as a small JSON object: `{"outcome":...}` with `tip` for `replayed` and
/// `paths` for `conflicts`.
fn replay_outcome_json(outcome: ReplayOutcome) -> String {
    match outcome {
        ReplayOutcome::Replayed(d) => format!(
            "{{\"outcome\":\"replayed\",\"tip\":{}}}",
            json_string(&d.to_string())
        ),
        ReplayOutcome::Clean => "{\"outcome\":\"clean\"}".to_string(),
        ReplayOutcome::Empty => "{\"outcome\":\"empty\"}".to_string(),
        ReplayOutcome::Conflicts(paths) => {
            let mut s = String::from("{\"outcome\":\"conflicts\",\"paths\":[");
            for (i, p) in paths.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&json_string(p));
            }
            s.push_str("]}");
            s
        }
    }
}

/// Cherry-pick the comma-separated `commits` onto the current branch tip; returns the outcome JSON.
pub(crate) fn cherry_pick_ns(
    h: &LoomSession,
    name: &str,
    commits: &str,
    dry_run: bool,
) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    let list = parse_commit_list(commits)?;
    let outcome = loom.cherry_pick(ns, &list, now_ms(), dry_run)?;
    if !dry_run {
        save_loom(&mut loom)?;
    }
    Ok(replay_outcome_json(outcome))
}

/// Revert the comma-separated `commits` on the current branch; returns the outcome JSON.
pub(crate) fn revert_ns(
    h: &LoomSession,
    name: &str,
    commits: &str,
    author: &str,
    dry_run: bool,
) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    let list = parse_commit_list(commits)?;
    let outcome = loom.revert(ns, &list, author, now_ms(), dry_run)?;
    if !dry_run {
        save_loom(&mut loom)?;
    }
    Ok(replay_outcome_json(outcome))
}

/// Rebase the current branch onto `onto`; returns the outcome JSON.
pub(crate) fn rebase_ns(
    h: &LoomSession,
    name: &str,
    onto: &str,
    dry_run: bool,
) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    let outcome = loom.rebase(ns, onto, now_ms(), dry_run)?;
    if !dry_run {
        save_loom(&mut loom)?;
    }
    Ok(replay_outcome_json(outcome))
}

/// Squash commits after `onto` up to the tip into one commit; returns the new commit digest.
pub(crate) fn squash_ns(
    h: &LoomSession,
    name: &str,
    onto: &str,
    author: &str,
    message: &str,
) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_ns(&loom, name)?;
    let new = loom.squash(ns, onto, author, message, now_ms())?;
    save_loom(&mut loom)?;
    Ok(new.to_string())
}

pub(crate) fn read_table_ns(h: &LoomSession, name: &str, table: &str) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    let t = loom.read_table(ns, table)?;
    result_cbor::table_cbor(&t)
}

pub(crate) fn read_table_at_ns(
    h: &LoomSession,
    name: &str,
    table: &str,
    commit_hex: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    let commit = Digest::parse(commit_hex)?;
    let t = loom.read_table_at(ns, table, commit)?;
    result_cbor::table_cbor(&t)
}

pub(crate) fn index_scan_ns(
    h: &LoomSession,
    name: &str,
    table: &str,
    index: &str,
    prefix: &[u8],
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    // The lookup-prefix input is canonical CBOR (a cell array), the same codec as the result payload -
    // the whole ABI speaks one canonical form.
    let values = lookup_cbor::values_from_cbor(prefix)?;
    let rows = loom.index_scan(ns, table, index, &values)?;
    let schema = loom.read_table(ns, table)?.schema().clone();
    result_cbor::rows_cbor(&schema, &rows)
}

pub(crate) fn index_scan_at_ns(
    h: &LoomSession,
    name: &str,
    table: &str,
    index: &str,
    prefix: &[u8],
    commit_hex: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    let commit = Digest::parse(commit_hex)?;
    let values = lookup_cbor::values_from_cbor(prefix)?;
    let rows = loom.index_scan_at(ns, table, index, &values, commit)?;
    let schema = loom.read_table_at(ns, table, commit)?.schema().clone();
    result_cbor::rows_cbor(&schema, &rows)
}

pub(crate) fn blame_table_ns(
    h: &LoomSession,
    name: &str,
    branch: &str,
    table: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    let rows = loom.blame_table(ns, branch, table)?;
    result_cbor::blame_cbor(&rows)
}

pub(crate) fn diff_table_ns(
    h: &LoomSession,
    name: &str,
    table: &str,
    from_hex: &str,
    to_hex: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    let from = Digest::parse(from_hex)?;
    let to = Digest::parse(to_hex)?;
    let diffs = loom.diff_table(ns, table, from, to)?;
    result_cbor::diff_cbor(&diffs)
}

pub(crate) fn diff_table_records_ns(
    h: &LoomSession,
    name: &str,
    table: &str,
    from_hex: &str,
    to_hex: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    let from = Digest::parse(from_hex)?;
    let to = Digest::parse(to_hex)?;
    let records = loom.diff_table_records(ns, table, from, to)?;
    result_cbor::table_diff_cbor(&records)
}

pub(crate) fn vcs_blame_ns(h: &LoomSession, name: &str, branch: &str) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    let paths = loom.blame(ns, branch)?;
    result_cbor::path_blame_cbor(&paths)
}

pub(crate) fn vcs_diff_ns(
    h: &LoomSession,
    name: &str,
    from_hex: &str,
    to_hex: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, name)?;
    let from = Digest::parse(from_hex)?;
    let to = Digest::parse(to_hex)?;
    loom.diff_commits(ns, from, to)
}

pub(crate) fn watch_subscribe_ns(
    h: &LoomSession,
    workspace: &str,
    branch: &str,
    facet: Option<&str>,
    path_prefix: Option<&str>,
    change_kinds: Option<&str>,
    from_commit: Option<&str>,
) -> LoomResult<String> {
    let loom = open_h_read(h)?;
    let ns = resolve_ns(&loom, workspace)?;
    let mut selector = WatchSelector::new(ns, branch)?;
    if let Some(facet) = facet.filter(|value| !value.is_empty()) {
        selector = selector.with_facet(FacetKind::parse(facet)?);
    }
    if let Some(path_prefix) = path_prefix.filter(|value| !value.is_empty()) {
        selector = selector.with_path_prefix(path_prefix);
    }
    for kind in parse_watch_change_kinds(change_kinds)? {
        selector = selector.with_change_kind(kind);
    }
    let from = from_commit
        .filter(|value| !value.is_empty())
        .map(Digest::parse)
        .transpose()?;
    Ok(loom.watch_subscribe(&selector, from)?.encode())
}

pub(crate) fn watch_poll_ns(h: &LoomSession, cursor: &str, max: u32) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let cursor = WatchCursor::decode(cursor)?;
    let batch = loom.watch_poll(&cursor, max as usize)?;
    watch_batch_to_cbor(&batch)
}

/// Subscribe to workspace history changes. Optional `facet`, `path_prefix`, `change_kinds`, and
/// `from_commit` pointers may be null or empty. `change_kinds` is a comma-separated list of
/// `added`, `modified`, and `deleted`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; required strings valid C strings; optional strings null or
/// valid C strings; `out_cursor` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_watch_subscribe(
    handle: *mut LoomSession,
    ns_name: *const c_char,
    branch: *const c_char,
    facet: *const c_char,
    path_prefix: *const c_char,
    change_kinds: *const c_char,
    from_commit: *const c_char,
    out_cursor: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_watch_subscribe");
    let n = arg_str!(ns_name, "loom_watch_subscribe");
    let b = arg_str!(branch, "loom_watch_subscribe");
    let args = || -> LoomResult<_> {
        Ok((
            unsafe { optional_arg_string(facet, "loom_watch_subscribe") }?,
            unsafe { optional_arg_string(path_prefix, "loom_watch_subscribe") }?,
            unsafe { optional_arg_string(change_kinds, "loom_watch_subscribe") }?,
            unsafe { optional_arg_string(from_commit, "loom_watch_subscribe") }?,
        ))
    };
    let (facet, path_prefix, change_kinds, from_commit) = match args() {
        Ok(args) => args,
        Err(e) => return fail(e),
    };
    match watch_subscribe_ns(
        h,
        n,
        b,
        facet.as_deref(),
        path_prefix.as_deref(),
        change_kinds.as_deref(),
        from_commit.as_deref(),
    ) {
        Ok(cursor) => unsafe { ok_str(out_cursor, &cursor) },
        Err(e) => fail(e),
    }
}

/// Poll a watch cursor and return a canonical-CBOR `loom.watch.batch.v1` batch.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `cursor` a valid C string; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_watch_poll(
    handle: *mut LoomSession,
    cursor: *const c_char,
    max: u32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_watch_poll");
    let c = arg_str!(cursor, "loom_watch_poll");
    match watch_poll_ns(h, c, max) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Open `loom_path`; on success writes an owned handle to `*out` (free with [`loom_close`]) and
/// returns `0`.
///
/// # Safety
/// `loom_path` must be a valid C string; `out` a valid `*mut *mut LoomSession`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_open(loom_path: *const c_char, out: *mut *mut LoomSession) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `loom_path` is a valid C string (see fn docs).
    let Some(path) = (unsafe { cstr(loom_path) }) else {
        return fail_arg("loom_open: null or non-UTF-8 path");
    };
    // SAFETY: caller guarantees `out` is writable (see fn docs).
    unsafe { open_handle_into(path, None, out) }
}

/// Like [`loom_open`], but unlocks an **encrypted** loom with the passphrase bytes at
/// `(passphrase, passphrase_len)`. A null/empty passphrase opens an unencrypted loom (and then fails
/// `E2eLocked` on an encrypted one, exactly like [`loom_open`]). The host acquires the passphrase
/// securely (OS keychain, prompt, KMS); the FFI never reads an environment variable.
///
/// # Safety
/// `loom_path` must be a valid C string; `passphrase` may be null or point to `passphrase_len` bytes;
/// `out` a valid `*mut *mut LoomSession`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_open_keyed(
    loom_path: *const c_char,
    passphrase: *const c_uchar,
    passphrase_len: usize,
    out: *mut *mut LoomSession,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `loom_path` is a valid C string (see fn docs).
    let Some(path) = (unsafe { cstr(loom_path) }) else {
        return fail_arg("loom_open_keyed: null or non-UTF-8 path");
    };
    // SAFETY: caller guarantees `passphrase`/`passphrase_len` describe a valid (or null) buffer.
    let key = match unsafe { passphrase_arg(passphrase, passphrase_len, "loom_open_keyed") } {
        Ok(k) => k,
        Err(code) => return code,
    };
    // SAFETY: caller guarantees `out` is writable (see fn docs).
    unsafe { open_handle_into(path, key.map(KeySpec::passphrase), out) }
}

/// Like [`loom_open`], but unlocks an encrypted loom with a caller-supplied 256-bit **KEK**
/// at `(kek, kek_len)` (= 32 bytes). A null/empty KEK behaves like [`loom_open`].
///
/// # Safety
/// `loom_path` a valid C string; `kek` null or `kek_len` readable bytes; `out` a valid
/// `*mut *mut LoomSession`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_open_with_kek(
    loom_path: *const c_char,
    kek: *const c_uchar,
    kek_len: usize,
    out: *mut *mut LoomSession,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `loom_path` is a valid C string (see fn docs).
    let Some(path) = (unsafe { cstr(loom_path) }) else {
        return fail_arg("loom_open_with_kek: null or non-UTF-8 path");
    };
    // SAFETY: caller guarantees the KEK buffer (see fn docs).
    let key = match unsafe { kek_arg(kek, kek_len, "loom_open_with_kek") } {
        Ok(k) => k,
        Err(code) => return code,
    };
    // SAFETY: caller guarantees `out` is writable (see fn docs).
    unsafe { open_handle_into(path, key, out) }
}

/// Validate that `path` opens (unlocking with `key` if encrypted) and, on success, write an owned
/// handle carrying the key to `*out`. The validating open is dropped immediately; each op reopens.
///
/// # Safety
/// `out` must be null or a valid writable `*mut *mut LoomSession`.
unsafe fn open_handle_into(path: &str, key: Option<KeySpec>, out: *mut *mut LoomSession) -> i32 {
    match open_loom_read_unlocked(path, key.as_ref()) {
        Ok(_) => {
            if !out.is_null() {
                // SAFETY: caller guarantees `out` is writable (see fn docs).
                unsafe {
                    *out = Box::into_raw(Box::new(LoomSession {
                        path: path.to_string(),
                        key,
                        session_id: None,
                        session_principal: None,
                    }))
                };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Read an optional passphrase argument: `null`/`len == 0` -> `None` (unencrypted); otherwise the bytes
/// must be valid UTF-8 (a text passphrase). On non-UTF-8 input it records `INVALID_ARGUMENT` and returns
/// that status code for the caller to return.
///
/// # Safety
/// `ptr` must be null or point to `len` readable bytes.
pub(crate) unsafe fn passphrase_arg(
    ptr: *const c_uchar,
    len: usize,
    who: &str,
) -> core::result::Result<Option<String>, i32> {
    if ptr.is_null() || len == 0 {
        return Ok(None);
    }
    // SAFETY: caller guarantees `ptr` points to `len` readable bytes (see fn docs).
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    match core::str::from_utf8(bytes) {
        Ok(s) => Ok(Some(s.to_string())),
        Err(_) => Err(fail_arg(&format!("{who}: passphrase is not valid UTF-8"))),
    }
}

/// Parse the identity-profile selector: `default`/`blake3` (the default profile) or
/// `fips`/`sha256` (the FIPS profile).
fn parse_profile(s: &str) -> LoomResult<Algo> {
    match s {
        "default" | "blake3" => Ok(Algo::Blake3),
        "fips" | "sha256" => Ok(Algo::Sha256),
        other => Err(LoomError::invalid(format!(
            "unknown identity profile {other:?} (expected `default`/`blake3` or `fips`/`sha256`)"
        ))),
    }
}

fn rng_fill(buf: &mut [u8]) -> LoomResult<()> {
    getrandom::fill(buf).map_err(|e| LoomError::new(Code::Internal, format!("rng: {e}")))
}

/// Create a fresh `.loom` under `profile`, optionally encrypted with `key`.
fn create_store(
    path: &str,
    profile: &str,
    suite: Option<&str>,
    key: Option<KeySpec>,
) -> LoomResult<()> {
    let digest_algo = parse_profile(profile)?;
    let store = match key {
        None => {
            if suite.is_some() {
                return Err(LoomError::invalid(
                    "a suite was given without a credential; encryption requires a passphrase or KEK",
                ));
            }
            FileStore::create_with_profile(path, digest_algo)?
        }
        Some(spec) => {
            // The FIPS profile pairs AES-256-GCM by default; the default profile pairs XChaCha20.
            let suite = match suite {
                Some(s) => Suite::parse(s)?,
                None if digest_algo == Algo::Sha256 => Suite::Aes256Gcm,
                None => Suite::XChaCha20Poly1305,
            };
            // The key layer takes randomness as input; the FFI supplies it (salt, DEK, wrap nonce). A raw
            // KEK ignores the salt; create records the matching wrap source.
            let mut salt = [0u8; 16];
            let mut dek = [0u8; loom_core::keys::KEY_LEN];
            let mut wrap_nonce = [0u8; 24];
            rng_fill(&mut salt)?;
            rng_fill(&mut dek)?;
            rng_fill(&mut wrap_nonce)?;
            let (meta, session) =
                EncryptionMeta::create(&spec, suite, salt.to_vec(), dek, wrap_nonce.to_vec())?;
            FileStore::create_encrypted_with_profile(path, meta.encode(), session, digest_algo)?
        }
    };
    let root = random_workspace_id()?;
    store.save_identity_store(&IdentityStore::new(root))?;
    let mut acl = AclStore::new();
    acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])?;
    store.save_acl_store(&acl)?;
    Ok(())
}

/// Create a fresh `.loom` at `loom_path` under an identity profile, optionally encrypted - the binding
/// counterpart of `loom init [--identity-profile fips] [--encrypt [--suite ...]]`. This is how a
/// binding chooses **default vs FIPS** and sets up at-rest encryption.
///
/// - `profile`: `"default"`/`"blake3"` (default profile) or `"fips"`/`"sha256"` (FIPS profile).
///   Immutable once written.
/// - `suite`: AEAD suite when encrypting (`"xchacha20-poly1305"` / `"aes-256-gcm"`); null/empty picks
///   the profile default (XChaCha for default, AES-256-GCM for FIPS). Ignored when not encrypting.
/// - `passphrase`/`passphrase_len`: null/empty is an unencrypted store; otherwise the store is encrypted
///   and the DEK is wrapped under this passphrase (Argon2id). The host confirms the passphrase before
///   calling; a typo permanently locks an immutable-at-creation store.
///
/// Fails `ALREADY_EXISTS` if a non-empty file already exists at `loom_path`. Returns `0` on success.
///
/// # Safety
/// `loom_path` and `profile` must be valid C strings; `suite` null or a valid C string; `passphrase`
/// null or pointing to `passphrase_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_create(
    loom_path: *const c_char,
    profile: *const c_char,
    suite: *const c_char,
    passphrase: *const c_uchar,
    passphrase_len: usize,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `loom_path`/`profile` are valid C strings (see fn docs).
    let (Some(path), Some(profile)) = (unsafe { cstr(loom_path) }, unsafe { cstr(profile) }) else {
        return fail_arg("loom_create: null or non-UTF-8 path/profile");
    };
    // SAFETY: caller guarantees `suite` is null or a valid C string; `cstr` returns None for null.
    let suite_arg = unsafe { cstr(suite) };
    // SAFETY: caller guarantees the passphrase buffer (see fn docs).
    let key = match unsafe { passphrase_arg(passphrase, passphrase_len, "loom_create") } {
        Ok(k) => k,
        Err(code) => return code,
    };
    match create_store(path, profile, suite_arg, key.map(KeySpec::passphrase)) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Like [`loom_create`], but wraps the DEK under a caller-supplied 256-bit **KEK**
/// instead of a passphrase: the host computed the KEK from an external provider (OS keychain,
/// Secure Enclave / TPM, passkey PRF, KMS/HSM) and passes the 32 bytes. The store records a `RawKek`
/// wrap; open it later with [`loom_open_with_kek`] / [`loom_sql_open_with_kek`]. A null/empty `kek`
/// creates an **unencrypted** store (use [`loom_create`] for a passphrase).
///
/// # Safety
/// `loom_path`/`profile` valid C strings; `suite` null or a valid C string; `kek` null or exactly
/// `kek_len` (= 32) readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_create_with_kek(
    loom_path: *const c_char,
    profile: *const c_char,
    suite: *const c_char,
    kek: *const c_uchar,
    kek_len: usize,
) -> i32 {
    clear_error();
    // SAFETY: caller guarantees `loom_path`/`profile` are valid C strings (see fn docs).
    let (Some(path), Some(profile)) = (unsafe { cstr(loom_path) }, unsafe { cstr(profile) }) else {
        return fail_arg("loom_create_with_kek: null or non-UTF-8 path/profile");
    };
    // SAFETY: `suite` is null or a valid C string; `cstr` returns None for null.
    let suite_arg = unsafe { cstr(suite) };
    // SAFETY: caller guarantees the KEK buffer (see fn docs).
    let key = match unsafe { kek_arg(kek, kek_len, "loom_create_with_kek") } {
        Ok(k) => k,
        Err(code) => return code,
    };
    match create_store(path, profile, suite_arg, key) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Free a handle from [`loom_open`]. Passing null is a no-op.
///
/// # Safety
/// `handle` must be a pointer returned by [`loom_open`] and not previously freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_close(handle: *mut LoomSession) {
    if !handle.is_null() {
        // SAFETY: `handle` came from `Box::into_raw` in `loom_open` (see fn docs).
        drop(unsafe { Box::from_raw(handle) });
    }
}

/// Return local daemon status for `loom_path` as JSON. A missing/stopped daemon is a successful
/// `STOPPED` result; invalid paths are errors.
///
/// # Safety
/// `loom_path` must be a valid C string; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_daemon_status_json(
    loom_path: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let path = arg_str!(loom_path, "loom_daemon_status_json");
    match daemon_status_json(path) {
        // SAFETY: `out` is writable per fn docs.
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Attach a named session to a running local daemon.
///
/// # Safety
/// `loom_path` and `session` must be valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_daemon_session_attach(
    loom_path: *const c_char,
    session: *const c_char,
) -> i32 {
    clear_error();
    let path = arg_str!(loom_path, "loom_daemon_session_attach");
    let session = arg_str!(session, "loom_daemon_session_attach");
    match daemon::paths(path).and_then(|paths| daemon::session_attach(&paths, session)) {
        Ok(_) => 0,
        Err(e) => fail(e),
    }
}

/// Detach a named session from a running local daemon.
///
/// # Safety
/// `loom_path` and `session` must be valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_daemon_session_detach(
    loom_path: *const c_char,
    session: *const c_char,
) -> i32 {
    clear_error();
    let path = arg_str!(loom_path, "loom_daemon_session_detach");
    let session = arg_str!(session, "loom_daemon_session_detach");
    match daemon::paths(path).and_then(|paths| daemon::session_detach(&paths, session)) {
        Ok(_) => 0,
        Err(e) => fail(e),
    }
}

/// Add a long-lived pin to a running local daemon.
///
/// # Safety
/// `loom_path` and `pin` must be valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_daemon_pin_add(loom_path: *const c_char, pin: *const c_char) -> i32 {
    clear_error();
    let path = arg_str!(loom_path, "loom_daemon_pin_add");
    let pin = arg_str!(pin, "loom_daemon_pin_add");
    match daemon::paths(path).and_then(|paths| daemon::pin_add(&paths, pin)) {
        Ok(_) => 0,
        Err(e) => fail(e),
    }
}

/// Remove a long-lived pin from a running local daemon.
///
/// # Safety
/// `loom_path` and `pin` must be valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_daemon_pin_remove(
    loom_path: *const c_char,
    pin: *const c_char,
) -> i32 {
    clear_error();
    let path = arg_str!(loom_path, "loom_daemon_pin_remove");
    let pin = arg_str!(pin, "loom_daemon_pin_remove");
    match daemon::paths(path).and_then(|paths| daemon::pin_remove(&paths, pin)) {
        Ok(_) => 0,
        Err(e) => fail(e),
    }
}

/// Acquire a daemon-backed lock and return its token as JSON.
///
/// # Safety
/// String arguments must be valid C strings; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lock_acquire_json(
    loom_path: *const c_char,
    key: *const c_char,
    principal: *const c_char,
    session: *const c_char,
    mode: *const c_char,
    permits: u32,
    capacity: u32,
    lease_ms: u64,
    wait_ms: u64,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let path = arg_str!(loom_path, "loom_lock_acquire_json");
    let key = arg_str!(key, "loom_lock_acquire_json");
    let principal = arg_str!(principal, "loom_lock_acquire_json");
    let session = arg_str!(session, "loom_lock_acquire_json");
    let mode = arg_str!(mode, "loom_lock_acquire_json");
    let result = daemon::paths(path).and_then(|paths| {
        let mode = daemon::parse_lock_mode(mode, permits, capacity)?;
        daemon::lock_acquire(
            &paths,
            daemon::AcquireRequest {
                key,
                principal,
                session,
                mode,
                lease_ms,
                wait_ms,
                now_ms: now_ms(),
            },
        )
        .and_then(|response| lock_token_json(&response))
    });
    match result {
        // SAFETY: `out` is writable per fn docs.
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Refresh a daemon-backed lock and return the refreshed token as JSON.
///
/// # Safety
/// String arguments must be valid C strings; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lock_refresh_json(
    loom_path: *const c_char,
    key: *const c_char,
    principal: *const c_char,
    session: *const c_char,
    mode: *const c_char,
    permits: u32,
    capacity: u32,
    fence_low: u64,
    fence_high: u64,
    lease_ms: u64,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let path = arg_str!(loom_path, "loom_lock_refresh_json");
    let key = arg_str!(key, "loom_lock_refresh_json");
    let principal = arg_str!(principal, "loom_lock_refresh_json");
    let session = arg_str!(session, "loom_lock_refresh_json");
    let mode = arg_str!(mode, "loom_lock_refresh_json");
    let result = daemon::paths(path).and_then(|paths| {
        let mode = daemon::parse_lock_mode(mode, permits, capacity)?;
        daemon::lock_refresh(
            &paths,
            daemon::RefreshRequest {
                key,
                principal,
                session,
                mode,
                fence: loom_core::Fence::from_limbs(fence_low, fence_high),
                lease_ms,
                now_ms: now_ms(),
            },
        )
        .and_then(|response| lock_token_json(&response))
    });
    match result {
        // SAFETY: `out` is writable per fn docs.
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Release a daemon-backed lock.
///
/// # Safety
/// String arguments must be valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_lock_release(
    loom_path: *const c_char,
    key: *const c_char,
    principal: *const c_char,
    session: *const c_char,
    mode: *const c_char,
    permits: u32,
    capacity: u32,
    fence_low: u64,
    fence_high: u64,
) -> i32 {
    clear_error();
    let path = arg_str!(loom_path, "loom_lock_release");
    let key = arg_str!(key, "loom_lock_release");
    let principal = arg_str!(principal, "loom_lock_release");
    let session = arg_str!(session, "loom_lock_release");
    let mode = arg_str!(mode, "loom_lock_release");
    let result = daemon::paths(path).and_then(|paths| {
        let mode = daemon::parse_lock_mode(mode, permits, capacity)?;
        daemon::lock_release(
            &paths,
            daemon::ReleaseRequest {
                key,
                principal,
                session,
                mode,
                fence: loom_core::Fence::from_limbs(fence_low, fence_high),
                now_ms: now_ms(),
            },
        )
    });
    match result {
        Ok(_) => 0,
        Err(e) => fail(e),
    }
}

/// Authenticate `principal` with a passphrase and bind the resulting identity to this session.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `principal` must be a valid C string; `passphrase` null or
/// `passphrase_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_authenticate_passphrase(
    handle: *mut LoomSession,
    principal: *const c_char,
    passphrase: *const c_uchar,
    passphrase_len: usize,
) -> i32 {
    clear_error();
    let h = handle_mut!(handle, "loom_authenticate_passphrase");
    let principal = arg_str!(principal, "loom_authenticate_passphrase");
    // SAFETY: caller guarantees the passphrase buffer (see fn docs).
    let passphrase =
        match unsafe { passphrase_arg(passphrase, passphrase_len, "loom_authenticate_passphrase") }
        {
            Ok(Some(value)) => value,
            Ok(None) => {
                return fail_arg("loom_authenticate_passphrase: passphrase is required");
            }
            Err(code) => return code,
        };
    let principal = match WorkspaceId::parse(principal) {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let session_id = match random_workspace_id() {
        Ok(id) => id.to_string(),
        Err(e) => return fail(e),
    };
    let result = (|| -> LoomResult<()> {
        let loom = open_loom_read_unlocked(&h.path, h.key.as_ref())?;
        let mut identity: IdentityStore = loom.store().identity_store()?.ok_or_else(|| {
            LoomError::new(Code::AuthenticationFailed, "identity store not found")
        })?;
        let session = identity.authenticate_passphrase(principal, &passphrase, session_id)?;
        h.session_id = Some(session.id);
        h.session_principal = Some(session.principal);
        Ok(())
    })();
    match result {
        Ok(()) => {
            clear_error();
            0
        }
        Err(e) => fail(e),
    }
}

/// Clear the authenticated identity bound to this session.
///
/// # Safety
/// `handle` must be from [`loom_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_clear_authentication(handle: *mut LoomSession) -> i32 {
    clear_error();
    let h = handle_mut!(handle, "loom_clear_authentication");
    h.session_id = None;
    h.session_principal = None;
    0
}

fn require_global_admin_actor(loom: &Loom<FileStore>) -> LoomResult<WorkspaceId> {
    let identity = loom
        .identity_store()
        .ok_or_else(|| LoomError::new(Code::Unsupported, "identity store not initialized"))?;
    let principal = loom
        .effective_principal()?
        .ok_or_else(|| LoomError::new(Code::AuthenticationFailed, "authentication required"))?;
    let roles = identity.effective_roles(principal)?;
    loom.acl_store().authorize_global_admin_with_roles(
        identity.authenticated_mode(),
        principal,
        roles,
    )?;
    Ok(principal)
}

fn require_global_admin(loom: &Loom<FileStore>) -> LoomResult<()> {
    require_global_admin_actor(loom).map(|_| ())
}

fn role_assignment_target(principal: WorkspaceId, role: WorkspaceId) -> String {
    format!("principal={principal};role={role}")
}

fn optional_workspace_arg(
    loom: &Loom<FileStore>,
    value: Option<&str>,
) -> LoomResult<Option<WorkspaceId>> {
    match value {
        Some(v) if !v.is_empty() => Ok(Some(resolve_workspace_arg(loom, v)?)),
        _ => Ok(None),
    }
}

fn optional_acl_domain_arg(value: Option<&str>) -> LoomResult<Option<AclDomain>> {
    match value {
        Some(value) if !value.is_empty() => Ok(Some(AclDomain::parse(value)?)),
        _ => Ok(None),
    }
}

struct AclGrantArgs<'a> {
    effect: i32,
    subject: &'a str,
    workspace: Option<&'a str>,
    domain: Option<&'a str>,
    rights_mask: u32,
    ref_glob: Option<&'a str>,
    scopes: Vec<AclScope>,
    predicate: Option<AclPredicate>,
}

fn acl_grant_from_args(loom: &Loom<FileStore>, args: AclGrantArgs<'_>) -> LoomResult<AclGrant> {
    Ok(AclGrant {
        subject: parse_acl_subject(args.subject)?,
        workspace: optional_workspace_arg(loom, args.workspace)?,
        domain: optional_acl_domain_arg(args.domain)?,
        ref_glob: args
            .ref_glob
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        scopes: args.scopes,
        rights: parse_acl_rights(args.rights_mask)?.into_iter().collect(),
        effect: parse_acl_effect(args.effect)?,
        predicate: args.predicate,
    })
}

fn optional_acl_predicate(
    language: Option<&str>,
    expression: Option<&str>,
) -> LoomResult<Option<AclPredicate>> {
    match (
        language.filter(|value| !value.is_empty()),
        expression.filter(|value| !value.is_empty()),
    ) {
        (None, None) => Ok(None),
        (Some("cel"), Some(expression)) => AclPredicate::cel(expression).map(Some),
        (Some(_), Some(_)) => Err(LoomError::invalid("acl predicate language must be cel")),
        _ => Err(LoomError::invalid(
            "acl predicate requires language and expression",
        )),
    }
}

fn save_acl_grant(loom: &mut Loom<FileStore>, grant: AclGrant) -> LoomResult<()> {
    let actor = require_global_admin_actor(loom)?;
    let target = acl_grant_json(&grant);
    let snapshot = {
        let acl = loom.acl_store_mut();
        acl.grant(grant)?;
        acl.clone()
    };
    loom.store()
        .save_acl_store_audited(&snapshot, Some(actor), "acl.grant", Some(&target))
        .map(|_| ())
}

fn save_acl_revoke(loom: &mut Loom<FileStore>, grant: &AclGrant) -> LoomResult<bool> {
    let actor = require_global_admin_actor(loom)?;
    let target = acl_grant_json(grant);
    let (removed, snapshot) = {
        let acl = loom.acl_store_mut();
        let removed = acl.revoke(grant);
        (removed, acl.clone())
    };
    if removed {
        loom.store()
            .save_acl_store_audited(&snapshot, Some(actor), "acl.revoke", Some(&target))?;
    }
    Ok(removed)
}

/// List principals as JSON. In unauthenticated root mode this is the bootstrap discovery surface;
/// after authentication is enabled it requires global `Admin`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `out` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_identity_list_json(
    handle: *mut LoomSession,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_identity_list_json");
    match open_h_read(h).and_then(|loom| {
        require_global_admin(&loom)?;
        let identity = loom
            .identity_store()
            .ok_or_else(|| LoomError::new(Code::Unsupported, "identity store not initialized"))?;
        Ok(identity_list_json(identity))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Create a principal and return its UUID string. `kind` is `root`, `user`, or `service`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `principal_handle`/`name`/`kind` valid C strings; `out_id` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_identity_add_principal(
    handle: *mut LoomSession,
    principal_handle: *const c_char,
    name: *const c_char,
    kind: *const c_char,
    out_id: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_identity_add_principal");
    let principal_handle = arg_str!(principal_handle, "loom_identity_add_principal");
    let name = arg_str!(name, "loom_identity_add_principal");
    let kind = arg_str!(kind, "loom_identity_add_principal");
    let result = (|| -> LoomResult<String> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let kind = parse_principal_kind(kind)?;
        let id = random_workspace_id()?;
        let snapshot = {
            let identity = loom.identity_store_mut().ok_or_else(|| {
                LoomError::new(Code::Unsupported, "identity store not initialized")
            })?;
            identity.add_principal_with_handle(id, principal_handle, name, kind)?;
            identity.clone()
        };
        let target = id.to_string();
        loom.store().save_identity_store_audited(
            &snapshot,
            Some(actor),
            "identity.add_principal",
            Some(&target),
        )?;
        Ok(id.to_string())
    })();
    match result {
        Ok(id) => unsafe { ok_str(out_id, &id) },
        Err(e) => fail(e),
    }
}

/// Rename a principal handle while retaining the previous handle as an alias.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `principal` and `principal_handle` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_identity_rename_principal_handle(
    handle: *mut LoomSession,
    principal: *const c_char,
    principal_handle: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_identity_rename_principal_handle");
    let principal = arg_str!(principal, "loom_identity_rename_principal_handle");
    let principal_handle = arg_str!(principal_handle, "loom_identity_rename_principal_handle");
    let result = (|| -> LoomResult<()> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let principal = WorkspaceId::parse(principal)?;
        let snapshot = {
            let identity = loom.identity_store_mut().ok_or_else(|| {
                LoomError::new(Code::Unsupported, "identity store not initialized")
            })?;
            identity.rename_principal_handle(principal, principal_handle)?;
            identity.clone()
        };
        loom.store().save_identity_store_audited(
            &snapshot,
            Some(actor),
            "identity.rename_principal_handle",
            Some(&principal.to_string()),
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(error) => fail(error),
    }
}

/// Set or replace a principal passphrase.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `principal` valid C string; `passphrase` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_identity_set_passphrase(
    handle: *mut LoomSession,
    principal: *const c_char,
    passphrase: *const c_uchar,
    passphrase_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_identity_set_passphrase");
    let principal = arg_str!(principal, "loom_identity_set_passphrase");
    let passphrase =
        match unsafe { passphrase_arg(passphrase, passphrase_len, "loom_identity_set_passphrase") }
        {
            Ok(Some(value)) => value,
            Ok(None) => return fail_arg("loom_identity_set_passphrase: passphrase is required"),
            Err(code) => return code,
        };
    let result = (|| -> LoomResult<()> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let principal = WorkspaceId::parse(principal)?;
        let mut salt = [0u8; 16];
        rng_fill(&mut salt)?;
        let snapshot = {
            let identity = loom.identity_store_mut().ok_or_else(|| {
                LoomError::new(Code::Unsupported, "identity store not initialized")
            })?;
            identity.set_passphrase(principal, &passphrase, &salt)?;
            identity.clone()
        };
        let target = principal.to_string();
        loom.store().save_identity_store_audited(
            &snapshot,
            Some(actor),
            "identity.set_passphrase",
            Some(&target),
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Remove a principal from the identity store.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `principal` valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_identity_remove_principal(
    handle: *mut LoomSession,
    principal: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_identity_remove_principal");
    let principal = arg_str!(principal, "loom_identity_remove_principal");
    let result = (|| -> LoomResult<()> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let principal = WorkspaceId::parse(principal)?;
        let snapshot = {
            let identity = loom.identity_store_mut().ok_or_else(|| {
                LoomError::new(Code::Unsupported, "identity store not initialized")
            })?;
            identity.remove_principal(principal)?;
            identity.clone()
        };
        let target = principal.to_string();
        loom.store().save_identity_store_audited(
            &snapshot,
            Some(actor),
            "identity.remove_principal",
            Some(&target),
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Assign a role to a principal.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `principal` and `role` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_identity_assign_role(
    handle: *mut LoomSession,
    principal: *const c_char,
    role: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_identity_assign_role");
    let principal = arg_str!(principal, "loom_identity_assign_role");
    let role = arg_str!(role, "loom_identity_assign_role");
    let result = (|| -> LoomResult<()> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let principal = WorkspaceId::parse(principal)?;
        let role = WorkspaceId::parse(role)?;
        let snapshot = {
            let identity = loom.identity_store_mut().ok_or_else(|| {
                LoomError::new(Code::Unsupported, "identity store not initialized")
            })?;
            identity.assign_role(principal, role)?;
            identity.clone()
        };
        let target = role_assignment_target(principal, role);
        loom.store().save_identity_store_audited(
            &snapshot,
            Some(actor),
            "identity.assign_role",
            Some(&target),
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Revoke a role from a principal. Writes whether anything was removed to `out_removed` when non-null.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `principal` and `role` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_identity_revoke_role(
    handle: *mut LoomSession,
    principal: *const c_char,
    role: *const c_char,
    out_removed: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_identity_revoke_role");
    let principal = arg_str!(principal, "loom_identity_revoke_role");
    let role = arg_str!(role, "loom_identity_revoke_role");
    let result = (|| -> LoomResult<bool> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let principal = WorkspaceId::parse(principal)?;
        let role = WorkspaceId::parse(role)?;
        let (removed, snapshot) = {
            let identity = loom.identity_store_mut().ok_or_else(|| {
                LoomError::new(Code::Unsupported, "identity store not initialized")
            })?;
            let removed = identity.revoke_role(principal, role)?;
            (removed, identity.clone())
        };
        if removed {
            let target = role_assignment_target(principal, role);
            loom.store().save_identity_store_audited(
                &snapshot,
                Some(actor),
                "identity.revoke_role",
                Some(&target),
            )?;
        }
        Ok(removed)
    })();
    match result {
        Ok(removed) => {
            if !out_removed.is_null() {
                unsafe { *out_removed = i32::from(removed) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Create an external-provider credential binding and return its UUID string.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string pointers must be valid C strings; `out_id` null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_identity_create_external_credential(
    handle: *mut LoomSession,
    principal: *const c_char,
    kind: *const c_char,
    label: *const c_char,
    issuer: *const c_char,
    subject: *const c_char,
    material_digest: *const c_char,
    out_id: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_identity_create_external_credential");
    let principal = arg_str!(principal, "loom_identity_create_external_credential");
    let kind = arg_str!(kind, "loom_identity_create_external_credential");
    let label = arg_str!(label, "loom_identity_create_external_credential");
    let issuer = arg_str!(issuer, "loom_identity_create_external_credential");
    let subject = arg_str!(subject, "loom_identity_create_external_credential");
    let material_digest = unsafe { cstr(material_digest) }.map(str::to_string);
    let result = (|| -> LoomResult<String> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let principal = WorkspaceId::parse(principal)?;
        let kind = ExternalCredentialKind::parse(kind)?;
        let id = random_workspace_id()?;
        let snapshot = {
            let identity = loom.identity_store_mut().ok_or_else(|| {
                LoomError::new(Code::Unsupported, "identity store not initialized")
            })?;
            identity.create_external_credential(
                principal,
                ExternalCredentialSpec {
                    id,
                    kind,
                    label: label.to_string(),
                    issuer: issuer.to_string(),
                    subject: subject.to_string(),
                    material_digest,
                },
            )?;
            identity.clone()
        };
        let target = format!("{principal}:{id}");
        loom.store().save_identity_store_audited(
            &snapshot,
            Some(actor),
            "identity.external_credential.create",
            Some(&target),
        )?;
        Ok(id.to_string())
    })();
    match result {
        Ok(id) => unsafe { ok_str(out_id, &id) },
        Err(e) => fail(e),
    }
}

/// Revoke an external-provider credential binding.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `credential` must be a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_identity_revoke_external_credential(
    handle: *mut LoomSession,
    credential: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_identity_revoke_external_credential");
    let credential = arg_str!(credential, "loom_identity_revoke_external_credential");
    let result = (|| -> LoomResult<()> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let credential_id = WorkspaceId::parse(credential)?;
        let (credential, snapshot) = {
            let identity = loom.identity_store_mut().ok_or_else(|| {
                LoomError::new(Code::Unsupported, "identity store not initialized")
            })?;
            let credential = identity.revoke_external_credential(credential_id)?;
            (credential, identity.clone())
        };
        let target = format!("{}:{}", credential.principal, credential.id);
        loom.store().save_identity_store_audited(
            &snapshot,
            Some(actor),
            "identity.external_credential.revoke",
            Some(&target),
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Add a principal-bound public verification key and return its UUID string.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string pointers must be valid C strings; `out_id` null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_identity_add_public_key(
    handle: *mut LoomSession,
    principal: *const c_char,
    label: *const c_char,
    algorithm: *const c_char,
    public_key_hex: *const c_char,
    out_id: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_identity_add_public_key");
    let principal = arg_str!(principal, "loom_identity_add_public_key");
    let label = arg_str!(label, "loom_identity_add_public_key");
    let algorithm = arg_str!(algorithm, "loom_identity_add_public_key");
    let public_key_hex = arg_str!(public_key_hex, "loom_identity_add_public_key");
    let result = (|| -> LoomResult<String> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let principal = WorkspaceId::parse(principal)?;
        let id = random_workspace_id()?;
        let public_key =
            hex::decode(public_key_hex.strip_prefix("0x").unwrap_or(public_key_hex))
                .map_err(|err| LoomError::invalid(format!("invalid public key hex: {err}")))?;
        let snapshot = {
            let identity = loom.identity_store_mut().ok_or_else(|| {
                LoomError::new(Code::Unsupported, "identity store not initialized")
            })?;
            identity.add_public_key(
                principal,
                IdentityPublicKeySpec {
                    id,
                    label: label.to_string(),
                    algorithm: algorithm.to_string(),
                    public_key,
                },
            )?;
            identity.clone()
        };
        let target = format!("{principal}:{id}");
        loom.store().save_identity_store_audited(
            &snapshot,
            Some(actor),
            "identity.public_key.add",
            Some(&target),
        )?;
        Ok(id.to_string())
    })();
    match result {
        Ok(id) => unsafe { ok_str(out_id, &id) },
        Err(e) => fail(e),
    }
}

/// Revoke a principal-bound public verification key.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `key` must be a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_identity_revoke_public_key(
    handle: *mut LoomSession,
    key: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_identity_revoke_public_key");
    let key = arg_str!(key, "loom_identity_revoke_public_key");
    let result = (|| -> LoomResult<()> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let key_id = WorkspaceId::parse(key)?;
        let (key, snapshot) = {
            let identity = loom.identity_store_mut().ok_or_else(|| {
                LoomError::new(Code::Unsupported, "identity store not initialized")
            })?;
            let key = identity.revoke_public_key(key_id)?;
            (key, identity.clone())
        };
        let target = format!("{}:{}", key.principal, key.id);
        loom.store().save_identity_store_audited(
            &snapshot,
            Some(actor),
            "identity.public_key.revoke",
            Some(&target),
        )?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// List direct ACL grants as JSON. Requires global `Admin` after authentication is enabled.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `out` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_acl_list_json(
    handle: *mut LoomSession,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_acl_list_json");
    match open_h_read(h).and_then(|loom| {
        require_global_admin(&loom)?;
        Ok(acl_list_json(loom.acl_store()))
    }) {
        Ok(json) => unsafe { ok_str(out, &json) },
        Err(e) => fail(e),
    }
}

/// Add a direct ACL grant. `effect`: 0 allow, 1 deny. `subject`: `*`, `everyone`, a principal UUID,
/// or `role:<role-uuid>`. `workspace` and `domain` may be null or empty for global wildcard. Rights
/// mask bits: read=1, write=2, advance=4, merge=8, execute=16, admin=32.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments null only where documented.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_acl_grant(
    handle: *mut LoomSession,
    effect: i32,
    subject: *const c_char,
    workspace: *const c_char,
    domain: *const c_char,
    rights_mask: u32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_acl_grant");
    let subject = arg_str!(subject, "loom_acl_grant");
    let workspace = unsafe { cstr(workspace) };
    let domain = unsafe { cstr(domain) };
    let result = (|| -> LoomResult<()> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let grant = acl_grant_from_args(
            &loom,
            AclGrantArgs {
                effect,
                subject,
                workspace,
                domain,
                rights_mask,
                ref_glob: None,
                scopes: vec![AclScope::All],
                predicate: None,
            },
        )?;
        let target = acl_grant_json(&grant);
        let snapshot = {
            let acl = loom.acl_store_mut();
            acl.grant(grant)?;
            acl.clone()
        };
        loom.store()
            .save_acl_store_audited(&snapshot, Some(actor), "acl.grant", Some(&target))?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Add a direct ACL grant with optional ref glob and typed prefix scopes. `scope_count == 0` means all
/// scopes. Scope kind values are ref=0, collection=1, path=2, key=3, table=4, exec=5.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments null only where documented; scoped arrays
/// must contain `scope_count` entries when `scope_count` is nonzero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_acl_grant_scoped(
    handle: *mut LoomSession,
    effect: i32,
    subject: *const c_char,
    workspace: *const c_char,
    domain: *const c_char,
    rights_mask: u32,
    ref_glob: *const c_char,
    scope_count: usize,
    scope_kinds: *const i32,
    scope_prefixes: *const *const c_uchar,
    scope_prefix_lens: *const usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_acl_grant_scoped");
    let subject = arg_str!(subject, "loom_acl_grant_scoped");
    let workspace = unsafe { cstr(workspace) };
    let domain = unsafe { cstr(domain) };
    let ref_glob = unsafe { cstr(ref_glob) };
    let scopes = match unsafe {
        acl_scopes_from_raw(
            scope_count,
            scope_kinds,
            scope_prefixes,
            scope_prefix_lens,
            "loom_acl_grant_scoped",
        )
    } {
        Ok(scopes) => scopes,
        Err(e) => return fail(e),
    };
    let result = (|| -> LoomResult<()> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let grant = acl_grant_from_args(
            &loom,
            AclGrantArgs {
                effect,
                subject,
                workspace,
                domain,
                rights_mask,
                ref_glob,
                scopes,
                predicate: None,
            },
        )?;
        let target = acl_grant_json(&grant);
        let snapshot = {
            let acl = loom.acl_store_mut();
            acl.grant(grant)?;
            acl.clone()
        };
        loom.store()
            .save_acl_store_audited(&snapshot, Some(actor), "acl.grant", Some(&target))?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Add a scoped ACL grant with an optional predicate. Null predicate language and expression create
/// an unconditional grant; otherwise both must be present and the language must be `cel`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments null only where documented; scoped arrays
/// must contain `scope_count` entries when `scope_count` is nonzero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_acl_grant_scoped_predicate(
    handle: *mut LoomSession,
    effect: i32,
    subject: *const c_char,
    workspace: *const c_char,
    domain: *const c_char,
    rights_mask: u32,
    ref_glob: *const c_char,
    scope_count: usize,
    scope_kinds: *const i32,
    scope_prefixes: *const *const c_uchar,
    scope_prefix_lens: *const usize,
    predicate_language: *const c_char,
    predicate_expression: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_acl_grant_scoped_predicate");
    let subject = arg_str!(subject, "loom_acl_grant_scoped_predicate");
    let workspace = unsafe { cstr(workspace) };
    let domain = unsafe { cstr(domain) };
    let ref_glob = unsafe { cstr(ref_glob) };
    let predicate_language = unsafe { cstr(predicate_language) };
    let predicate_expression = unsafe { cstr(predicate_expression) };
    let scopes = match unsafe {
        acl_scopes_from_raw(
            scope_count,
            scope_kinds,
            scope_prefixes,
            scope_prefix_lens,
            "loom_acl_grant_scoped_predicate",
        )
    } {
        Ok(scopes) => scopes,
        Err(e) => return fail(e),
    };
    let result = (|| -> LoomResult<()> {
        let mut loom = open_h_write(h)?;
        let grant = acl_grant_from_args(
            &loom,
            AclGrantArgs {
                effect,
                subject,
                workspace,
                domain,
                rights_mask,
                ref_glob,
                scopes,
                predicate: optional_acl_predicate(predicate_language, predicate_expression)?,
            },
        )?;
        save_acl_grant(&mut loom, grant)
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Remove direct ACL grants exactly matching the supplied fields. Writes whether anything was removed
/// to `out_removed` when non-null.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments null only where documented.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_acl_revoke(
    handle: *mut LoomSession,
    effect: i32,
    subject: *const c_char,
    workspace: *const c_char,
    domain: *const c_char,
    rights_mask: u32,
    out_removed: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_acl_revoke");
    let subject = arg_str!(subject, "loom_acl_revoke");
    let workspace = unsafe { cstr(workspace) };
    let domain = unsafe { cstr(domain) };
    let result = (|| -> LoomResult<bool> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let grant = acl_grant_from_args(
            &loom,
            AclGrantArgs {
                effect,
                subject,
                workspace,
                domain,
                rights_mask,
                ref_glob: None,
                scopes: vec![AclScope::All],
                predicate: None,
            },
        )?;
        let target = acl_grant_json(&grant);
        let (removed, snapshot) = {
            let acl = loom.acl_store_mut();
            let removed = acl.revoke(&grant);
            (removed, acl.clone())
        };
        if removed {
            loom.store().save_acl_store_audited(
                &snapshot,
                Some(actor),
                "acl.revoke",
                Some(&target),
            )?;
        }
        Ok(removed)
    })();
    match result {
        Ok(removed) => {
            if !out_removed.is_null() {
                unsafe { *out_removed = i32::from(removed) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Remove direct ACL grants exactly matching optional ref glob and typed prefix scopes.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments null only where documented; scoped arrays
/// must contain `scope_count` entries when `scope_count` is nonzero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_acl_revoke_scoped(
    handle: *mut LoomSession,
    effect: i32,
    subject: *const c_char,
    workspace: *const c_char,
    domain: *const c_char,
    rights_mask: u32,
    ref_glob: *const c_char,
    scope_count: usize,
    scope_kinds: *const i32,
    scope_prefixes: *const *const c_uchar,
    scope_prefix_lens: *const usize,
    out_removed: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_acl_revoke_scoped");
    let subject = arg_str!(subject, "loom_acl_revoke_scoped");
    let workspace = unsafe { cstr(workspace) };
    let domain = unsafe { cstr(domain) };
    let ref_glob = unsafe { cstr(ref_glob) };
    let scopes = match unsafe {
        acl_scopes_from_raw(
            scope_count,
            scope_kinds,
            scope_prefixes,
            scope_prefix_lens,
            "loom_acl_revoke_scoped",
        )
    } {
        Ok(scopes) => scopes,
        Err(e) => return fail(e),
    };
    let result = (|| -> LoomResult<bool> {
        let mut loom = open_h_write(h)?;
        let actor = require_global_admin_actor(&loom)?;
        let grant = acl_grant_from_args(
            &loom,
            AclGrantArgs {
                effect,
                subject,
                workspace,
                domain,
                rights_mask,
                ref_glob,
                scopes,
                predicate: None,
            },
        )?;
        let target = acl_grant_json(&grant);
        let (removed, snapshot) = {
            let acl = loom.acl_store_mut();
            let removed = acl.revoke(&grant);
            (removed, acl.clone())
        };
        if removed {
            loom.store().save_acl_store_audited(
                &snapshot,
                Some(actor),
                "acl.revoke",
                Some(&target),
            )?;
        }
        Ok(removed)
    })();
    match result {
        Ok(removed) => {
            if !out_removed.is_null() {
                unsafe { *out_removed = i32::from(removed) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Remove a scoped ACL grant with an optional predicate, matching the grant exactly.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments null only where documented; scoped arrays
/// must contain `scope_count` entries when `scope_count` is nonzero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_acl_revoke_scoped_predicate(
    handle: *mut LoomSession,
    effect: i32,
    subject: *const c_char,
    workspace: *const c_char,
    domain: *const c_char,
    rights_mask: u32,
    ref_glob: *const c_char,
    scope_count: usize,
    scope_kinds: *const i32,
    scope_prefixes: *const *const c_uchar,
    scope_prefix_lens: *const usize,
    predicate_language: *const c_char,
    predicate_expression: *const c_char,
    out_removed: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_acl_revoke_scoped_predicate");
    let subject = arg_str!(subject, "loom_acl_revoke_scoped_predicate");
    let workspace = unsafe { cstr(workspace) };
    let domain = unsafe { cstr(domain) };
    let ref_glob = unsafe { cstr(ref_glob) };
    let predicate_language = unsafe { cstr(predicate_language) };
    let predicate_expression = unsafe { cstr(predicate_expression) };
    let scopes = match unsafe {
        acl_scopes_from_raw(
            scope_count,
            scope_kinds,
            scope_prefixes,
            scope_prefix_lens,
            "loom_acl_revoke_scoped_predicate",
        )
    } {
        Ok(scopes) => scopes,
        Err(e) => return fail(e),
    };
    let result = (|| -> LoomResult<bool> {
        let mut loom = open_h_write(h)?;
        let grant = acl_grant_from_args(
            &loom,
            AclGrantArgs {
                effect,
                subject,
                workspace,
                domain,
                rights_mask,
                ref_glob,
                scopes,
                predicate: optional_acl_predicate(predicate_language, predicate_expression)?,
            },
        )?;
        save_acl_revoke(&mut loom, &grant)
    })();
    match result {
        Ok(removed) => {
            if !out_removed.is_null() {
                unsafe { *out_removed = i32::from(removed) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// List protected-ref policies for one workspace as JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string; `out_json` must be a
/// valid output pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_protected_ref_list_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    out_json: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_protected_ref_list_json");
    let workspace = arg_str!(workspace, "loom_protected_ref_list_json");
    let result = (|| -> LoomResult<String> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(protected_ref_policies_json(
            &loom.protected_ref_policies(ns)?,
        ))
    })();
    match result {
        Ok(json) => unsafe { ok_str(out_json, &json) },
        Err(e) => fail(e),
    }
}

/// Return one protected-ref policy as JSON, or JSON null when absent.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `ref_name` must be valid C strings;
/// `out_json` must be a valid output pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_protected_ref_get_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ref_name: *const c_char,
    out_json: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_protected_ref_get_json");
    let workspace = arg_str!(workspace, "loom_protected_ref_get_json");
    let ref_name = arg_str!(ref_name, "loom_protected_ref_get_json");
    let result = (|| -> LoomResult<String> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(match loom.protected_ref_policy(ns, ref_name)? {
            Some(policy) => protected_ref_policy_json(ref_name, &policy),
            None => "null".to_string(),
        })
    })();
    match result {
        Ok(json) => unsafe { ok_str(out_json, &json) },
        Err(e) => fail(e),
    }
}

/// Store or replace one protected-ref policy.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `ref_name` must be valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_protected_ref_set(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ref_name: *const c_char,
    fast_forward_only: bool,
    signed_commits_required: bool,
    signed_ref_advance_required: bool,
    required_review_count: u32,
    retention_lock: bool,
    governance_lock: bool,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_protected_ref_set");
    let workspace = arg_str!(workspace, "loom_protected_ref_set");
    let ref_name = arg_str!(ref_name, "loom_protected_ref_set");
    let result = (|| -> LoomResult<()> {
        let mut loom = open_h_write(h)?;
        let actor = loom.effective_principal()?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        let policy = ProtectedRefPolicy {
            fast_forward_only,
            signed_commits_required,
            signed_ref_advance_required,
            required_review_count,
            retention_lock,
            governance_lock,
        };
        loom.set_protected_ref_policy(ns, ref_name, policy)?;
        save_loom(&mut loom)?;
        let target = format!("workspace={ns};ref={ref_name}");
        loom.store()
            .audit_append(actor, "protected_ref.set", Some(&target))?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Remove one protected-ref policy and report whether it existed.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `ref_name` must be valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_protected_ref_remove(
    handle: *mut LoomSession,
    workspace: *const c_char,
    ref_name: *const c_char,
    out_removed: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_protected_ref_remove");
    let workspace = arg_str!(workspace, "loom_protected_ref_remove");
    let ref_name = arg_str!(ref_name, "loom_protected_ref_remove");
    let result = (|| -> LoomResult<bool> {
        let mut loom = open_h_write(h)?;
        let actor = loom.effective_principal()?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        let removed = loom.remove_protected_ref_policy(ns, ref_name)?;
        if removed {
            save_loom(&mut loom)?;
            let target = format!("workspace={ns};ref={ref_name}");
            loom.store()
                .audit_append(actor, "protected_ref.remove", Some(&target))?;
        }
        Ok(removed)
    })();
    match result {
        Ok(removed) => {
            if !out_removed.is_null() {
                unsafe { *out_removed = i32::from(removed) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

fn open_file_store_for_key_update(h: &LoomSession) -> LoomResult<FileStore> {
    let fs = FileStore::open(&h.path)?;
    if let Some(key) = &h.key {
        fs.unlock(key)?;
        if fs.is_encrypted() && !fs.is_unlocked() {
            return Err(LoomError::new(
                Code::Internal,
                "loom-store: key update failed to unlock store",
            ));
        }
    }
    fs.validate_runtime_policy()?;
    Ok(fs)
}

fn add_wrap_to_handle(
    h: &LoomSession,
    new_spec: &KeySpec,
    allow_no_recovery: bool,
) -> LoomResult<()> {
    let fs = open_file_store_for_key_update(h)?;
    let mut salt = [0u8; 16];
    let mut wrap_nonce = [0u8; 24];
    rng_fill(&mut salt)?;
    rng_fill(&mut wrap_nonce)?;
    fs.add_wrap(
        new_spec,
        salt.to_vec(),
        wrap_nonce.to_vec(),
        allow_no_recovery,
    )
}

/// Add a passphrase unlock wrap to an encrypted store opened by `handle`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `passphrase` null or `passphrase_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_key_add_wrap_keyed(
    handle: *mut LoomSession,
    passphrase: *const c_uchar,
    passphrase_len: usize,
    allow_no_recovery: bool,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_key_add_wrap_keyed");
    // SAFETY: caller guarantees the passphrase buffer (see fn docs).
    let key = match unsafe { passphrase_arg(passphrase, passphrase_len, "loom_key_add_wrap_keyed") }
    {
        Ok(Some(k)) => KeySpec::passphrase(k),
        Ok(None) => return fail_arg("loom_key_add_wrap_keyed: empty passphrase"),
        Err(code) => return code,
    };
    match add_wrap_to_handle(h, &key, allow_no_recovery) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Add a caller-supplied KEK unlock wrap to an encrypted store opened by `handle`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `kek` null or `kek_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_key_add_wrap_with_kek(
    handle: *mut LoomSession,
    kek: *const c_uchar,
    kek_len: usize,
    allow_no_recovery: bool,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_key_add_wrap_with_kek");
    // SAFETY: caller guarantees the KEK buffer (see fn docs).
    let key = match unsafe { kek_arg(kek, kek_len, "loom_key_add_wrap_with_kek") } {
        Ok(Some(k)) => k,
        Ok(None) => return fail_arg("loom_key_add_wrap_with_kek: empty KEK"),
        Err(code) => return code,
    };
    match add_wrap_to_handle(h, &key, allow_no_recovery) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Remove one unlock wrap by zero-based index from an encrypted store opened by `handle`.
///
/// # Safety
/// `handle` must be from [`loom_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_key_remove_wrap(
    handle: *mut LoomSession,
    index: usize,
    allow_no_recovery: bool,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_key_remove_wrap");
    match (|| {
        let fs = open_file_store_for_key_update(h)?;
        fs.remove_wrap(index, allow_no_recovery)
    })() {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Create a workspace, optionally with an initial facet. Null or empty `name` uses the default name.
/// Writes the workspace UUID string to `*out`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `name` and `facet` may be null or valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_workspace_create(
    handle: *mut LoomSession,
    name: *const c_char,
    facet: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_workspace_create");
    let name = if name.is_null() {
        None
    } else {
        let value = arg_str!(name, "loom_workspace_create");
        (!value.is_empty()).then_some(value)
    };
    let facet = if facet.is_null() {
        None
    } else {
        let value = arg_str!(facet, "loom_workspace_create");
        if value.is_empty() {
            None
        } else {
            match FacetKind::parse(value) {
                Ok(f) => Some(f),
                Err(e) => return fail(e),
            }
        }
    };
    match (|| {
        let mut loom = open_h_write(h)?;
        let id = random_workspace_id()?;
        let ns = match facet {
            Some(facet) => loom.registry_mut().create(facet, name, id)?,
            None => loom.registry_mut().create_workspace(name, id)?,
        };
        save_loom(&mut loom)?;
        Ok::<String, LoomError>(ns.to_string())
    })() {
        // SAFETY: `out` is writable per fn docs.
        Ok(id) => unsafe { ok_str(out, &id) },
        Err(e) => fail(e),
    }
}

/// List workspaces as JSON. Each item has `{ id, name, facets, head }`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `out` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_workspace_list_json(
    handle: *mut LoomSession,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_workspace_list_json");
    match open_h_read(h) {
        // SAFETY: `out` is writable per fn docs.
        Ok(loom) => unsafe { ok_str(out, &workspace_list_json(&loom)) },
        Err(e) => fail(e),
    }
}

/// Rename a workspace selected by UUID or current name.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `new_name` must be valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_workspace_rename(
    handle: *mut LoomSession,
    workspace: *const c_char,
    new_name: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_workspace_rename");
    let workspace = arg_str!(workspace, "loom_workspace_rename");
    let new_name = arg_str!(new_name, "loom_workspace_rename");
    match (|| {
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom.registry_mut().rename(ns, new_name)?;
        save_loom(&mut loom)
    })() {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Delete a workspace selected by UUID or name.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_workspace_delete(
    handle: *mut LoomSession,
    workspace: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_workspace_delete");
    let workspace = arg_str!(workspace, "loom_workspace_delete");
    match (|| {
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom.registry_mut().delete(ns)?;
        save_loom(&mut loom)
    })() {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}
