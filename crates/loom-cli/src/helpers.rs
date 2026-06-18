//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Render a tabular row as tab-separated cell values for the `table` verbs' text output.
pub(crate) fn format_row(row: &[loom_core::tabular::Value]) -> String {
    use loom_core::tabular::Value;
    row.iter()
        .map(|v| match v {
            Value::Null => "NULL".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Text(s) => s.clone(),
            Value::Bytes(b) => format!(
                "0x{}",
                b.iter().map(|x| format!("{x:02x}")).collect::<String>()
            ),
            // Rich scalar/composite types (decimals, temporals, uuid, etc.) render via Debug.
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join("\t")
}

/// Print a GlueSQL result payload: a `SELECT` as a tab-separated header then one tab-separated line
/// per row; a DML count or other statement as a short status line.
pub(crate) fn print_payload(payload: &Payload) {
    match payload {
        Payload::Select { labels, rows } => {
            println!("{}", labels.join("\t"));
            for row in rows {
                let cells: Vec<String> = row.iter().map(format_value).collect();
                println!("{}", cells.join("\t"));
            }
        }
        Payload::Insert(n) => println!("INSERT {n}"),
        Payload::Delete(n) => println!("DELETE {n}"),
        Payload::Update(n) => println!("UPDATE {n}"),
        other => println!("{other:?}"),
    }
}

/// Render a GlueSQL cell value for the `sql` verb's text output; uncommon types fall back to `Debug`.
pub(crate) fn format_value(v: &GValue) -> String {
    match v {
        GValue::Null => "NULL".to_string(),
        GValue::Bool(b) => b.to_string(),
        GValue::I64(i) => i.to_string(),
        GValue::F64(f) => f.to_string(),
        GValue::Str(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

/// Unlock `fs` when the store is encrypted, acquiring the passphrase from the configured key source
/// only if needed, so object reads return plaintext and object writes seal under the DEK (an
/// encrypted store must never be written with a plaintext frame). A no-op on an unencrypted store.
pub(crate) fn unlock_if_encrypted(fs: &FileStore, keys: &KeyOpts) -> Result<(), String> {
    if fs.is_encrypted() {
        fs.unlock(&acquire_key_spec(&keys.source, "Passphrase", false)?)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn init_control_state(fs: &FileStore) -> Result<(), String> {
    let root = random_workspace_id()?;
    fs.save_identity_store(&IdentityStore::new(root))
        .map_err(|e| e.to_string())?;
    let mut acl = AclStore::new();
    acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])
        .map_err(|e| e.to_string())?;
    fs.save_acl_store(&acl).map_err(|e| e.to_string())
}

pub(crate) fn ensure_control_state(fs: &FileStore) -> Result<(), String> {
    let Some(identity) = fs.identity_store().map_err(|e| e.to_string())? else {
        return init_control_state(fs);
    };
    if fs.acl_store().map_err(|e| e.to_string())?.is_none() {
        let mut acl = AclStore::new();
        if let Some(root) = identity.root_principal() {
            acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])
                .map_err(|e| e.to_string())?;
        }
        fs.save_acl_store(&acl).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn acquire_auth_session(
    keys: &KeyOpts,
) -> Result<Option<(WorkspaceId, String)>, String> {
    let Some(principal) = &keys.auth_principal else {
        return Ok(None);
    };
    match keys.auth_source {
        KeySource::RawKekFile(_) | KeySource::RawKekFd(_) => {
            return Err("--auth-key-source accepts passphrases only".to_string());
        }
        KeySource::Prompt | KeySource::File(_) | KeySource::Fd(_) => {}
    }
    let principal = WorkspaceId::parse(principal).map_err(|e| e.to_string())?;
    let passphrase = acquire(&keys.auth_source, "Principal passphrase", false)?;
    Ok(Some((principal, passphrase)))
}

pub(crate) fn session_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "cli-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

pub(crate) fn attach_control_state(
    mut loom: Loom<FileStore>,
    keys: &KeyOpts,
) -> Result<Loom<FileStore>, String> {
    let auth = acquire_auth_session(keys)?;
    if let Some(mut identity) = loom.store().identity_store().map_err(|e| e.to_string())? {
        if let Some((principal, passphrase)) = auth {
            let session = identity
                .authenticate_passphrase(principal, &passphrase, session_id())
                .map_err(|e| e.to_string())?;
            loom.set_session(session.id);
        }
        loom.set_identity_store(identity);
    }
    if let Some(acl) = loom.store().acl_store().map_err(|e| e.to_string())? {
        loom.set_acl_store(acl);
    }
    loom.set_acl_predicate_evaluator(std::sync::Arc::new(loom_compute::CelAclPredicateEvaluator));
    Ok(loom)
}

/// Build a full `Loom` from an already-opened store, acquiring + applying the passphrase only
/// if the store is encrypted - so unencrypted stores never prompt - then loading the engine state. The
/// reference-root object is itself sealed, so the unlock must precede `load_state`.
pub(crate) fn open_loom_from(
    fs: FileStore,
    keys: &KeyOpts,
    initialize_control: bool,
) -> Result<Loom<FileStore>, String> {
    unlock_if_encrypted(&fs, keys)?;
    if initialize_control {
        ensure_control_state(&fs)?;
    }
    let root = fs.reference_root();
    let mut loom = Loom::new(fs);
    if let Some(root) = root {
        loom.load_state(root).map_err(|e| e.to_string())?;
    }
    attach_control_state(loom, keys)
}

pub(crate) fn open_loom_registry_from(
    fs: FileStore,
    keys: &KeyOpts,
) -> Result<Loom<FileStore>, String> {
    unlock_if_encrypted(&fs, keys)?;
    let root = fs.reference_root();
    let mut loom = Loom::new(fs);
    if let Some(root) = root {
        loom.load_state_registry(root).map_err(|e| e.to_string())?;
    }
    attach_control_state(loom, keys)
}

pub(crate) struct CliStore {
    store: FileStore,
    _daemon_session: Option<DaemonSessionLease>,
}

impl std::ops::Deref for CliStore {
    type Target = FileStore;

    fn deref(&self) -> &Self::Target {
        &self.store
    }
}

impl std::ops::DerefMut for CliStore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.store
    }
}

pub(crate) struct CliLoom {
    loom: Loom<FileStore>,
    _daemon_session: Option<DaemonSessionLease>,
}

impl std::ops::Deref for CliLoom {
    type Target = Loom<FileStore>;

    fn deref(&self) -> &Self::Target {
        &self.loom
    }
}

impl std::ops::DerefMut for CliLoom {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.loom
    }
}

struct DaemonSessionLease {
    paths: daemon::DaemonPaths,
    session: String,
}

impl Drop for DaemonSessionLease {
    fn drop(&mut self) {
        let _ = daemon::session_detach(&self.paths, &self.session);
    }
}

fn attach_daemon_session_if_running(store: &str) -> Result<Option<DaemonSessionLease>, String> {
    let Ok(paths) = daemon::paths(store) else {
        return Ok(None);
    };
    if daemon::status_response(&paths).is_err() {
        return Ok(None);
    }
    let session = session_id();
    daemon::session_attach(&paths, &session).map_err(|e| e.to_string())?;
    Ok(Some(DaemonSessionLease { paths, session }))
}

pub(crate) fn cli_open_loom(store: &str, keys: &KeyOpts) -> Result<CliLoom, String> {
    let opened = cli_open_store_for_write(store)?;
    let CliStore {
        store,
        _daemon_session,
    } = opened;
    let loom = open_loom_from(store, keys, true)?;
    Ok(CliLoom {
        loom,
        _daemon_session,
    })
}

pub(crate) fn cli_open_store_for_write(store: &str) -> Result<CliStore, String> {
    let resolved = crate::locator_cx::current().resolve_local(store)?;
    let store = resolved.as_str();
    let daemon_session = attach_daemon_session_if_running(store)?;
    let store = match &daemon_session {
        Some(session) => {
            FileStore::open_daemon_authorized(&session.paths.store).map_err(|e| e.to_string())?
        }
        None => FileStore::open(store).map_err(|e| e.to_string())?,
    };
    Ok(CliStore {
        store,
        _daemon_session: daemon_session,
    })
}

#[cfg(all(test, feature = "integration-tests"))]
fn cli_store_is_daemon_attached(store: &CliStore) -> bool {
    store._daemon_session.is_some()
}

#[cfg(all(test, feature = "integration-tests"))]
fn cli_loom_is_daemon_attached(loom: &CliLoom) -> bool {
    loom._daemon_session.is_some()
}

#[cfg(all(test, feature = "integration-tests"))]
fn cli_store_daemon_session_id(store: &CliStore) -> Option<&str> {
    store
        ._daemon_session
        .as_ref()
        .map(|lease| lease.session.as_str())
}

#[cfg(all(test, feature = "integration-tests"))]
fn cli_loom_daemon_session_id(loom: &CliLoom) -> Option<&str> {
    loom._daemon_session
        .as_ref()
        .map(|lease| lease.session.as_str())
}

#[cfg(all(test, feature = "integration-tests"))]
fn fake_daemon_for_store(
    store: &str,
    expected_sessions: usize,
) -> (daemon::DaemonPaths, std::thread::JoinHandle<Vec<String>>) {
    let paths = daemon::paths(store).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let store_path = paths.store.clone();
    let store_id = paths.store_id.clone();
    let handle = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..(1 + expected_sessions * 2) {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            stream.read_to_string(&mut request).unwrap();
            let response = match request
                .trim_end()
                .split('\t')
                .collect::<Vec<_>>()
                .as_slice()
            {
                ["status"] => format!(
                    "running\tprotocol=1\ttransport=tcp\tfake-pid\t{store_path}\tidentity={store_id}\tsessions=0\tpins=0\n"
                ),
                ["session-attach", session] => format!("attached\t{session}\tsessions=1\n"),
                ["session-detach", session] => format!("detached\t{session}\tsessions=0\n"),
                other => panic!("unexpected daemon request {other:?}"),
            };
            requests.push(request);
            write!(stream, "{response}").unwrap();
        }
        requests
    });
    (paths, handle)
}

#[cfg(all(test, feature = "integration-tests"))]
fn cleanup_fake_daemon(paths: &daemon::DaemonPaths) {
    for path in [&paths.addr_file, &paths.pid_file, &paths.lock_file] {
        let _ = std::fs::remove_file(path);
    }
}

/// Read-only, lock-free counterpart of [`cli_open_loom`].
pub(crate) fn cli_open_loom_read(store: &str, keys: &KeyOpts) -> Result<Loom<FileStore>, String> {
    let fs = FileStore::open_read(store).map_err(|e| e.to_string())?;
    open_loom_from(fs, keys, false)
}

pub(crate) fn cli_open_loom_registry_read(
    store: &str,
    keys: &KeyOpts,
) -> Result<Loom<FileStore>, String> {
    let fs = FileStore::open_read(store).map_err(|e| e.to_string())?;
    open_loom_registry_from(fs, keys)
}

/// Milliseconds since the Unix epoch (commit timestamp); 0 if the clock is before the epoch.
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn random_workspace_id() -> Result<WorkspaceId, String> {
    let bytes: [u8; 16] = rand_bytes(16)?
        .try_into()
        .map_err(|_| "rng returned the wrong workspace id length".to_string())?;
    Ok(WorkspaceId::v4_from_bytes(bytes))
}

pub(crate) fn resolve_ns(loom: &Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, String> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Name(workspace.to_string()),
    };
    loom.registry().open(&selector).map_err(|e| e.to_string())
}

pub(crate) fn load_drive_policy_registry(store: &FileStore) -> Result<DrivePolicyRegistry, String> {
    store
        .control_get(&drive_policy_registry_key())
        .map_err(|e| e.to_string())?
        .map(|bytes| DrivePolicyRegistry::decode(&bytes).map_err(|e| e.to_string()))
        .transpose()
        .map(|registry| registry.unwrap_or_else(DrivePolicyRegistry::empty))
}

pub(crate) fn save_drive_policy_registry_audited(
    store: &FileStore,
    registry: &DrivePolicyRegistry,
    actor: Option<WorkspaceId>,
    target: &str,
) -> Result<u64, String> {
    store
        .control_set_audited(
            &drive_policy_registry_key(),
            registry.encode().map_err(|e| e.to_string())?,
            actor,
            "drive.policy_registry.configure",
            Some(target),
        )
        .map_err(|e| e.to_string())
}

pub(crate) fn register_drive_policy_target(
    loom: &Loom<FileStore>,
    actor: Option<WorkspaceId>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<u64, String> {
    let mut registry = load_drive_policy_registry(loom.store())?;
    registry
        .upsert_enabled(
            DrivePolicyTarget::new(workspace, workspace_id, true).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    save_drive_policy_registry_audited(
        loom.store(),
        &registry,
        actor,
        &format!("workspace={workspace};profile={workspace_id};enabled=true"),
    )
}

pub(crate) fn optional_workspace_arg(
    loom: &Loom<FileStore>,
    value: Option<&str>,
) -> Result<Option<WorkspaceId>, String> {
    match value {
        Some(v) if !v.is_empty() => Ok(Some(resolve_ns(loom, v)?)),
        _ => Ok(None),
    }
}

pub(crate) fn optional_acl_domain_arg(value: Option<&str>) -> Result<Option<AclDomain>, String> {
    match value {
        Some(v) if !v.is_empty() => Ok(Some(AclDomain::parse(v).map_err(|e| e.to_string())?)),
        _ => Ok(None),
    }
}

pub(crate) fn require_global_admin(loom: &Loom<FileStore>) -> Result<(), String> {
    let identity = loom
        .identity_store()
        .ok_or_else(|| "identity store not initialized".to_string())?;
    let principal = loom
        .effective_principal()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "authentication required".to_string())?;
    let roles = identity
        .effective_roles(principal)
        .map_err(|e| e.to_string())?;
    loom.acl_store()
        .authorize_global_admin_with_roles(identity.authenticated_mode(), principal, roles)
        .map_err(|e| e.to_string())
}

pub(crate) fn require_global_admin_actor(loom: &Loom<FileStore>) -> Result<WorkspaceId, String> {
    require_global_admin(loom)?;
    loom.effective_principal()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "authentication required".to_string())
}

pub(crate) fn parse_principal_kind(value: &str) -> Result<PrincipalKind, String> {
    match value {
        "root" => Ok(PrincipalKind::Root),
        "user" => Ok(PrincipalKind::User),
        "service" => Ok(PrincipalKind::Service),
        other => Err(format!("unknown principal kind {other:?}")),
    }
}

pub(crate) fn principal_kind_str(kind: PrincipalKind) -> &'static str {
    match kind {
        PrincipalKind::Root => "root",
        PrincipalKind::User => "user",
        PrincipalKind::Service => "service",
    }
}

pub(crate) fn parse_acl_effect(value: &str) -> Result<AclEffect, String> {
    match value {
        "allow" => Ok(AclEffect::Allow),
        "deny" => Ok(AclEffect::Deny),
        other => Err(format!("unknown acl effect {other:?}")),
    }
}

pub(crate) fn acl_effect_str(effect: AclEffect) -> &'static str {
    match effect {
        AclEffect::Allow => "allow",
        AclEffect::Deny => "deny",
    }
}

pub(crate) fn parse_acl_subject(value: &str) -> Result<AclSubject, String> {
    match value {
        "*" | "everyone" => Ok(AclSubject::Everyone),
        role if role.starts_with("role:") => Ok(AclSubject::Role(
            WorkspaceId::parse(&role[5..]).map_err(|e| e.to_string())?,
        )),
        other => Ok(AclSubject::Principal(
            WorkspaceId::parse(other).map_err(|e| e.to_string())?,
        )),
    }
}

pub(crate) fn parse_acl_right(value: &str) -> Result<AclRight, String> {
    match value {
        "read" => Ok(AclRight::Read),
        "write" => Ok(AclRight::Write),
        "advance" => Ok(AclRight::Advance),
        "merge" => Ok(AclRight::Merge),
        "execute" => Ok(AclRight::Execute),
        "admin" => Ok(AclRight::Admin),
        other => Err(format!("unknown acl right {other:?}")),
    }
}

pub(crate) fn parse_acl_scope(value: &str) -> Result<AclScope, String> {
    let (kind, prefix) = value
        .split_once(':')
        .ok_or_else(|| "acl scope must be KIND:PREFIX".to_string())?;
    let kind = match kind {
        "ref" => AclScopeKind::Ref,
        "collection" => AclScopeKind::Collection,
        "path" => AclScopeKind::Path,
        "key" => AclScopeKind::Key,
        "table" => AclScopeKind::Table,
        "exec" => AclScopeKind::Exec,
        other => return Err(format!("unknown acl scope kind {other:?}")),
    };
    Ok(AclScope::Prefix {
        kind,
        prefix: prefix.as_bytes().to_vec(),
    })
}

pub(crate) fn acl_right_str(right: AclRight) -> &'static str {
    match right {
        AclRight::Read => "read",
        AclRight::Write => "write",
        AclRight::Advance => "advance",
        AclRight::Merge => "merge",
        AclRight::Execute => "execute",
        AclRight::Admin => "admin",
    }
}

pub(crate) struct AclGrantArgs<'a> {
    pub effect: &'a str,
    pub subject: &'a str,
    pub workspace: Option<&'a str>,
    pub domain: Option<&'a str>,
    pub rights: &'a [String],
    pub ref_glob: Option<&'a str>,
    pub scopes: &'a [String],
    pub predicate_cel: Option<&'a str>,
}

pub(crate) fn acl_grant_from_args(
    loom: &Loom<FileStore>,
    args: AclGrantArgs<'_>,
) -> Result<AclGrant, String> {
    let scopes = if args.scopes.is_empty() {
        vec![AclScope::All]
    } else {
        args.scopes
            .iter()
            .map(|scope| parse_acl_scope(scope))
            .collect::<Result<_, _>>()?
    };
    Ok(AclGrant {
        subject: parse_acl_subject(args.subject)?,
        workspace: optional_workspace_arg(loom, args.workspace)?,
        domain: optional_acl_domain_arg(args.domain)?,
        ref_glob: args
            .ref_glob
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        scopes,
        rights: args
            .rights
            .iter()
            .map(|right| parse_acl_right(right))
            .collect::<Result<_, _>>()?,
        effect: parse_acl_effect(args.effect)?,
        predicate: optional_acl_predicate(args.predicate_cel)?,
    })
}

pub(crate) fn optional_acl_predicate(value: Option<&str>) -> Result<Option<AclPredicate>, String> {
    value
        .filter(|value| !value.is_empty())
        .map(AclPredicate::cel)
        .transpose()
        .map_err(|e| e.to_string())
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

pub(crate) fn role_json(role: &IdentityRole) -> String {
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

pub(crate) fn principal_json(principal: &Principal) -> String {
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

pub(crate) fn app_credential_json(credential: &AppCredential) -> String {
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

pub(crate) fn external_credential_json(credential: &ExternalCredential) -> String {
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

pub(crate) fn identity_list_json(identity: &IdentityStore) -> String {
    let view = loom_wire::identity::IdentitySnapshotView {
        authenticated_mode: identity.authenticated_mode(),
        root: identity.root_principal(),
        authority: identity.authority_state().clone(),
        authority_handoffs: identity.authority_handoffs().cloned().collect(),
        forced_detach: identity.forced_detach().cloned(),
        principals: identity.principals().cloned().collect(),
        roles: identity.roles().cloned().collect(),
        app_credentials: identity.app_credentials().cloned().collect(),
        external_credentials: identity.external_credentials().cloned().collect(),
        public_keys: identity.public_keys().cloned().collect(),
    };
    identity_snapshot_json(&view)
}

/// Render an [`IdentitySnapshotView`] as the `identity list` JSON. Both the local arm (from the live
/// `IdentityStore`) and the remote arm (from a decoded snapshot) format through this one function, so
/// their output is byte-for-byte identical.
///
/// [`IdentitySnapshotView`]: loom_wire::identity::IdentitySnapshotView
pub(crate) fn identity_snapshot_json(view: &loom_wire::identity::IdentitySnapshotView) -> String {
    let mut out = String::from("{\"authenticated_mode\":");
    out.push_str(if view.authenticated_mode {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"root\":");
    match view.root {
        Some(root) => out.push_str(&json_string(&root.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"authority\":");
    out.push_str(&identity_authority_state_json(&view.authority));
    out.push_str(",\"authority_handoffs\":[");
    for (idx, handoff) in view.authority_handoffs.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&identity_authority_handoff_json(handoff));
    }
    out.push_str("],\"forced_detach\":");
    match &view.forced_detach {
        Some(detach) => out.push_str(&identity_authority_detach_json(detach)),
        None => out.push_str("null"),
    }
    out.push_str(",\"principals\":[");
    for (idx, principal) in view.principals.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&principal_json(principal));
    }
    out.push_str("],\"roles\":[");
    for (idx, role) in view.roles.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&role_json(role));
    }
    out.push_str("],\"app_credentials\":[");
    for (idx, credential) in view.app_credentials.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&app_credential_json(credential));
    }
    out.push_str("],\"external_credentials\":[");
    for (idx, credential) in view.external_credentials.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&external_credential_json(credential));
    }
    out.push_str("],\"public_keys\":[");
    for (idx, key) in view.public_keys.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&identity_public_key_json(key));
    }
    out.push_str("]}");
    out
}

pub(crate) fn identity_public_key_json(key: &loom_core::IdentityPublicKey) -> String {
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

/// Render the `identity public-key list` JSON array. Both the local arm (from the live `IdentityStore`)
/// and the remote arm (from a decoded snapshot) format through this one function.
pub(crate) fn identity_public_keys_json(keys: &[loom_core::IdentityPublicKey]) -> String {
    let mut out = String::from("[");
    for (idx, key) in keys.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&identity_public_key_json(key));
    }
    out.push(']');
    out
}

pub(crate) fn identity_authority_state_json(state: &loom_core::IdentityAuthorityState) -> String {
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

pub(crate) fn identity_authority_handoff_json(
    handoff: &loom_core::IdentityAuthorityHandoff,
) -> String {
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

pub(crate) fn identity_authority_detach_json(
    detach: &loom_core::IdentityAuthorityDetach,
) -> String {
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

pub(crate) fn identity_authority_witness_json(
    witness: &loom_core::IdentityAuthorityWitness,
    algo: loom_core::Algo,
) -> String {
    let record = witness.encode();
    let record_digest = witness.digest(algo);
    let mut out = String::new();
    out.push('{');
    out.push_str("\"authority\":");
    out.push_str(&json_string(&witness.authority.to_string()));
    out.push_str(",\"mode\":");
    out.push_str(&json_string(identity_authority_mode_str(witness.mode)));
    out.push_str(",\"generation\":");
    out.push_str(&witness.generation.to_string());
    out.push_str(",\"head\":");
    match witness.head {
        Some(head) => out.push_str(&json_string(&head.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"snapshot_digest\":");
    out.push_str(&json_string(&witness.snapshot_digest.to_string()));
    out.push_str(",\"latest_handoff_digest\":");
    match witness.latest_handoff_digest {
        Some(digest) => out.push_str(&json_string(&digest.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"record_hex\":");
    out.push_str(&json_string(&hex_bytes(&record)));
    out.push_str(",\"record_digest\":");
    out.push_str(&json_string(&record_digest.to_string()));
    out.push('}');
    out
}

pub(crate) fn identity_authority_sync_report_json(
    report: &loom_core::IdentityAuthoritySyncReport,
    algo: loom_core::Algo,
    seq: u64,
) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"seq\":");
    out.push_str(&seq.to_string());
    out.push_str(",\"from_generation\":");
    out.push_str(&report.from_generation.to_string());
    out.push_str(",\"to_generation\":");
    out.push_str(&report.to_generation.to_string());
    out.push_str(",\"applied\":");
    out.push_str(if report.applied { "true" } else { "false" });
    out.push_str(",\"witness\":");
    out.push_str(&identity_authority_witness_json(&report.witness, algo));
    out.push('}');
    out
}

pub(crate) fn authority_replication_policy_json(
    policy: &loom_store::AuthorityReplicationPolicy,
) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&policy.id));
    out.push_str(",\"source\":");
    out.push_str(&json_string(&policy.source));
    out.push_str(",\"enabled\":");
    out.push_str(if policy.enabled { "true" } else { "false" });
    out.push_str(",\"pull_on_start\":");
    out.push_str(if policy.pull_on_start {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"interval_ms\":");
    push_json_u64_option(&mut out, policy.interval_ms);
    out.push_str(",\"jitter_ms\":");
    out.push_str(&policy.jitter_ms.to_string());
    out.push_str(",\"backoff_ms\":");
    out.push_str(&policy.backoff_ms.to_string());
    out.push_str(",\"publish_witness\":");
    out.push_str(if policy.publish_witness {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"last_success_ms\":");
    push_json_u64_option(&mut out, policy.last_success_ms);
    out.push_str(",\"last_failure_ms\":");
    push_json_u64_option(&mut out, policy.last_failure_ms);
    out.push_str(",\"last_error\":");
    push_json_option(&mut out, policy.last_error.as_deref());
    out.push_str(",\"last_modified_audit_seq\":");
    push_json_u64_option(&mut out, policy.last_modified_audit_seq);
    out.push('}');
    out
}

pub(crate) fn authority_replication_policies_json(
    policies: &[loom_store::AuthorityReplicationPolicy],
) -> String {
    let mut out = String::from("{\"policies\":[");
    for (idx, policy) in policies.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&authority_replication_policy_json(policy));
    }
    out.push_str("]}");
    out
}

pub(crate) fn push_json_u64_option(out: &mut String, value: Option<u64>) {
    match value {
        Some(value) => out.push_str(&value.to_string()),
        None => out.push_str("null"),
    }
}

pub(crate) fn push_json_option(out: &mut String, value: Option<&str>) {
    match value {
        Some(value) => out.push_str(&json_string(value)),
        None => out.push_str("null"),
    }
}

fn identity_authority_mode_str(mode: loom_core::IdentityAuthorityMode) -> &'static str {
    match mode {
        loom_core::IdentityAuthorityMode::Authority => "authority",
        loom_core::IdentityAuthorityMode::Mirror => "mirror",
        loom_core::IdentityAuthorityMode::Detached => "detached",
    }
}

pub(crate) fn acl_grant_json(grant: &AclGrant) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"effect\":");
    out.push_str(&json_string(acl_effect_str(grant.effect)));
    out.push_str(",\"subject\":");
    match grant.subject {
        AclSubject::Everyone => out.push_str(&json_string("*")),
        AclSubject::Principal(principal) => out.push_str(&json_string(&principal.to_string())),
        AclSubject::Role(role) => out.push_str(&json_string(&format!("role:{role}"))),
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

/// Render an ACL grant list as JSON. Shared by the local `acl list` path (over `acl_store().grants()`)
/// and the remote path (which decodes `acl_list` records into `AclGrant`s), so both produce
/// byte-identical output.
pub(crate) fn acl_grants_json(grants: &[AclGrant]) -> String {
    let mut out = String::from("[");
    for (idx, grant) in grants.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&acl_grant_json(grant));
    }
    out.push(']');
    out
}

pub(crate) fn protected_ref_policy_json(ref_name: &str, policy: &ProtectedRefPolicy) -> String {
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

pub(crate) fn protected_ref_policies_json(policies: &[(String, ProtectedRefPolicy)]) -> String {
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

pub(crate) fn parse_kv_tier(value: &str) -> Result<KvTier, String> {
    match value {
        "versioned" => Ok(KvTier::Versioned),
        "ephemeral" => Ok(KvTier::Ephemeral),
        other => Err(format!("unknown kv tier {other:?}")),
    }
}

pub(crate) fn kv_tier_str(tier: KvTier) -> &'static str {
    match tier {
        KvTier::Versioned => "versioned",
        KvTier::Ephemeral => "ephemeral",
    }
}

pub(crate) fn opt_u64_json(value: Option<u64>) -> String {
    value.map_or_else(|| "null".to_string(), |v| v.to_string())
}

pub(crate) fn kv_map_config_json(config: KvMapConfig) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"tier\":");
    out.push_str(&json_string(kv_tier_str(config.tier)));
    out.push_str(",\"default_ttl_ms\":");
    out.push_str(&opt_u64_json(config.default_put.ttl_ms));
    out.push_str(",\"default_idle_ttl_ms\":");
    out.push_str(&opt_u64_json(config.default_put.idle_ttl_ms));
    out.push_str(",\"read_through\":");
    out.push_str(if config.read_through { "true" } else { "false" });
    out.push_str(",\"write_through\":");
    out.push_str(if config.write_through {
        "true"
    } else {
        "false"
    });
    out.push('}');
    out
}

pub(crate) fn ensure_kv_workspace(
    loom: &mut Loom<FileStore>,
    workspace: &str,
) -> Result<WorkspaceId, String> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Kv,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(|e| e.to_string())?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Kv)
        .map_err(|e| e.to_string())?;
    Ok(ns)
}

#[cfg(any(feature = "mcp", feature = "fuse", feature = "nfs"))]
pub(crate) fn mount_open_auth(store: &str, keys: &KeyOpts) -> Result<LocalOpenAuth, String> {
    let fs = FileStore::open_read(store).map_err(|e| e.to_string())?;
    let unlock_key = if fs.is_encrypted() {
        Some(acquire_key_spec(&keys.source, "Passphrase", false)?)
    } else {
        None
    };
    let auth = acquire_auth_session(keys)?;
    let (principal, passphrase, session_id) = match auth {
        Some((principal, passphrase)) => (Some(principal), Some(passphrase), Some(session_id())),
        None => (None, None, None),
    };
    Ok(LocalOpenAuth {
        unlock_key,
        principal,
        passphrase,
        app_credential: None,
        verified_external: None,
        preauthenticated_principal: None,
        session_id,
    })
}

#[cfg(any(feature = "fuse", feature = "nfs"))]
pub(crate) fn ensure_mount_workspace(
    store: &str,
    workspace: &str,
    auth: &LocalOpenAuth,
) -> Result<(), String> {
    let loom = loom_store::open_loom_unlocked(store, auth.unlock_key.as_ref())
        .map_err(|e| e.to_string())?;
    ensure_control_state(loom.store())?;
    let mut loom = loom_store::attach_local_auth(loom, auth).map_err(|e| e.to_string())?;
    let ns = resolve_ns(&loom, workspace)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Files)
        .map_err(|e| e.to_string())?;
    save_loom(&mut loom).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(any(feature = "fuse", feature = "nfs"))]
const MOUNT_PIN_LEASE_MS: u64 = 10_000;

#[cfg(any(feature = "fuse", feature = "nfs"))]
pub(crate) struct MountPinLease {
    store: String,
    pin: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

#[cfg(any(feature = "fuse", feature = "nfs"))]
impl MountPinLease {
    pub(crate) fn acquire(store: &str, pin: &str) -> Result<Self, String> {
        refresh_mount_pin(store, pin)?;
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_store = store.to_string();
        let worker_pin = pin.to_string();
        let worker = std::thread::spawn(move || {
            let interval = std::time::Duration::from_millis(MOUNT_PIN_LEASE_MS / 3);
            while !worker_stop.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(interval);
                if worker_stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let _ = refresh_mount_pin(&worker_store, &worker_pin);
            }
        });
        Ok(Self {
            store: store.to_string(),
            pin: pin.to_string(),
            stop,
            worker: Some(worker),
        })
    }
}

#[cfg(any(feature = "fuse", feature = "nfs"))]
impl Drop for MountPinLease {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = daemon_unpin(&self.store, &self.pin);
    }
}

#[cfg(any(feature = "fuse", feature = "nfs"))]
fn refresh_mount_pin(store: &str, pin: &str) -> Result<(), String> {
    let paths = daemon::paths(store).map_err(|e| e.to_string())?;
    daemon::pin_add_lease(&paths, pin, MOUNT_PIN_LEASE_MS, now_ms())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Start the in-process NFSv3 server and attach it at `mountpoint` so the user can browse it, then
/// unmount on Ctrl-C. macOS and Linux require root to attach an NFS mount, so the `mount`/`umount`
/// calls go through `sudo` (which prompts as needed). The server runs on a background thread; the
/// foreground waits for Ctrl-C and unmounts so the OS is not left with a stale mount.
#[cfg(feature = "nfs")]
pub(crate) fn mount_nfs_flow(
    store: &str,
    workspace: &str,
    listen: &str,
    mountpoint: &str,
    read_only: bool,
    auth: LocalOpenAuth,
) -> Result<(), String> {
    // The mount client connects to the same host:port the server binds (it serves NFS + MOUNT +
    // portmap on the one port).
    let (host, port) = listen
        .rsplit_once(':')
        .ok_or_else(|| format!("--listen must be host:port, got {listen:?}"))?;
    let mount_host = if host.is_empty() || host == "0.0.0.0" {
        "127.0.0.1"
    } else {
        host
    };
    let port: u16 = port
        .parse()
        .map_err(|_| format!("invalid port in --listen {listen:?}"))?;

    // Serve on a background thread (serves until the process exits).
    let (s, n, l, a) = (
        store.to_string(),
        workspace.to_string(),
        listen.to_string(),
        auth,
    );
    std::thread::spawn(move || {
        if let Err(e) =
            loom_vfs_nfs::serve_blocking_with_auth(std::path::Path::new(&s), &n, &l, read_only, a)
        {
            eprintln!("loom: NFS server stopped: {e}");
            std::process::exit(1);
        }
    });

    wait_until_listening(mount_host, port)?;
    let _ = std::fs::create_dir_all(mountpoint); // best effort; the mount reports a bad path clearly
    let pin = format!("mount-nfs:{mountpoint}");
    let _pin_lease = MountPinLease::acquire(store, &pin)?;
    nfs_mount(mount_host, port, mountpoint)?;
    println!("loom: mounted {mount_host}:/ at {mountpoint}  (Ctrl-C to unmount and stop)");

    let mp = mountpoint.to_string();
    let pin_store = store.to_string();
    let pin_id = pin.clone();
    ctrlc::set_handler(move || {
        eprintln!("\nloom: unmounting {mp} ...");
        let _ = daemon_unpin(&pin_store, &pin_id);
        let _ = nfs_umount(&mp);
        std::process::exit(0);
    })
    .map_err(|e| format!("install Ctrl-C handler: {e}"))?;

    loop {
        std::thread::park();
    }
}

/// Poll until the NFS server accepts TCP connections on `host:port` (up to ~5s).
#[cfg(feature = "nfs")]
pub(crate) fn wait_until_listening(host: &str, port: u16) -> Result<(), String> {
    for _ in 0..50 {
        if std::net::TcpStream::connect((host, port)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(format!(
        "NFS server did not start listening on {host}:{port}"
    ))
}

/// Attach the NFS export at `host:port` to `mountpoint` with the OS NFS client (via `sudo`).
#[cfg(feature = "nfs")]
pub(crate) fn nfs_mount(host: &str, port: u16, mountpoint: &str) -> Result<(), String> {
    let src = format!("{host}:/");
    let status = if cfg!(target_os = "macos") {
        let opts = format!("vers=3,tcp,port={port},mountport={port},nolocks,soft");
        std::process::Command::new("sudo")
            .args(["mount_nfs", "-o", &opts, &src, mountpoint])
            .status()
    } else {
        let opts = format!("vers=3,tcp,port={port},mountport={port},nolock,soft");
        std::process::Command::new("sudo")
            .args(["mount", "-t", "nfs", "-o", &opts, &src, mountpoint])
            .status()
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("mount command exited with {s}")),
        Err(e) => Err(format!("could not run the mount command: {e}")),
    }
}

/// Detach the NFS mount at `mountpoint` (via `sudo`).
#[cfg(feature = "nfs")]
pub(crate) fn nfs_umount(mountpoint: &str) -> Result<(), String> {
    let status = std::process::Command::new("sudo")
        .args(["umount", mountpoint])
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("umount exited with {s}")),
        Err(e) => Err(format!("could not run umount: {e}")),
    }
}

/// Print the workspace table from a set of [`WorkspaceInfo`]s. Used by the local `workspace list` path
/// (which reads `loom.registry().list`) and the remote path (which decodes `workspace_list` records), so
/// both produce byte-identical output.
pub(crate) fn print_workspaces_infos(infos: &[loom_core::WorkspaceInfo]) {
    println!("{:<36}  {:<20}  {:<24}  head", "id", "name", "facets");
    for info in infos {
        let head = info.head.map_or_else(|| "-".to_string(), |d| d.to_string());
        let facets = if info.facets.is_empty() {
            "-".to_string()
        } else {
            info.facets
                .iter()
                .map(|f| f.as_str())
                .collect::<Vec<_>>()
                .join(",")
        };
        println!(
            "{:<36}  {:<20}  {:<24}  {head}",
            info.id.to_string(),
            info.name,
            facets
        );
    }
}

pub(crate) fn read_input(path: &str) -> std::io::Result<Vec<u8>> {
    if path == "-" {
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf)?;
        Ok(buf)
    } else {
        std::fs::read(path)
    }
}

/// `n` fresh random bytes from the OS (for the encryption KDF salt, DEK, and AEAD nonces).
pub(crate) fn rand_bytes(n: usize) -> Result<Vec<u8>, String> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).map_err(|e| format!("rng: {e}"))?;
    Ok(b)
}

pub(crate) fn write_output(out: Option<&str>, bytes: &[u8]) -> std::io::Result<()> {
    match out {
        Some(path) => std::fs::write(path, bytes),
        None => std::io::stdout().write_all(bytes),
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Run a command with the default key source. Commands that need credentials pass explicit keys.
    fn trun(command: Command) -> Result<(), String> {
        run(command, &KeyOpts::default())
    }

    fn ns_create(store: String, name: &str, facet: Option<&str>) -> Command {
        Command::Management {
            action: ManagementCmd::Workspace {
                action: WorkspaceCmd::Create {
                    store,
                    name: name.into(),
                    facet: facet.map(str::to_string),
                },
            },
        }
    }

    fn ns_list(store: String) -> Command {
        Command::Management {
            action: ManagementCmd::Workspace {
                action: WorkspaceCmd::List { store },
            },
        }
    }

    fn ns_rename(store: String, workspace: String, new_name: &str) -> Command {
        Command::Management {
            action: ManagementCmd::Workspace {
                action: WorkspaceCmd::Rename {
                    store,
                    workspace,
                    new_name: new_name.into(),
                },
            },
        }
    }

    fn ns_delete(store: String, workspace: &str) -> Command {
        Command::Management {
            action: ManagementCmd::Workspace {
                action: WorkspaceCmd::Delete {
                    store,
                    workspace: workspace.into(),
                },
            },
        }
    }

    fn identity_cmd(action: IdentityCmd) -> Command {
        Command::Management {
            action: ManagementCmd::Identity { action },
        }
    }

    fn acl_cmd(action: AclCmd) -> Command {
        Command::Management {
            action: ManagementCmd::Acl { action },
        }
    }

    fn protected_ref_cmd(action: ProtectedRefCmd) -> Command {
        Command::Management {
            action: ManagementCmd::ProtectedRef { action },
        }
    }

    fn management_kv_config(action: ManagementKvConfigCmd) -> Command {
        Command::Management {
            action: ManagementCmd::Kv {
                action: ManagementKvCmd::Config { action },
            },
        }
    }

    fn audit_cmd(action: AuditCmd) -> Command {
        Command::Audit { action }
    }

    fn serve_command(store: String, surface: &str, selector: Vec<&str>, bind: &str) -> Command {
        Command::Serve {
            action: ServeCmd::Configure(Box::new(ServeConfigureArgs {
                store,
                surface: surface.into(),
                selector: selector.into_iter().map(str::to_string).collect(),
                bind: bind.into(),
                transport: None,
                profile: None,
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
        }
    }

    fn serve_action(action: &str, store: String, selector: Vec<String>) -> Command {
        let id = || selector[0].clone();
        let action = match action {
            "list" => ServeCmd::List { store },
            "enable" => ServeCmd::Enable { store, id: id() },
            "disable" => ServeCmd::Disable { store, id: id() },
            "remove" => ServeCmd::Remove { store, id: id() },
            other => panic!("unknown serve action {other:?}"),
        };
        Command::Serve { action }
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

    fn store_put(store: String, path: String) -> Command {
        Command::Store {
            action: StoreCmd::Put { store, path },
        }
    }

    fn store_get(store: String, digest: String, out: Option<String>) -> Command {
        Command::Store {
            action: StoreCmd::Get { store, digest, out },
        }
    }

    #[test]
    fn store_init_default_matches_binary_profile() {
        let store = temp("init-default-profile", "loom");
        trun(store_init(store.clone())).unwrap();
        let fs = FileStore::open_read(&store).unwrap();
        assert_eq!(fs.digest_algo(), default_init_digest_algo());
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn store_init_rejects_conflicting_fips_profile_flags() {
        let store = temp("init-conflicting-fips", "loom");
        let err = trun(Command::Store {
            action: StoreCmd::Init {
                store: store.clone(),
                encrypt: false,
                suite: None,
                identity_profile: Some("default".into()),
                fips: true,
            },
        })
        .unwrap_err();
        assert!(err.contains("--fips requires"));
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn store_copy_with_fips_migrates_committed_workspace() {
        let src = temp("copy-fips-src", "loom");
        let dst = temp("copy-fips-dst", "loom");
        let report = temp("copy-fips-report", "json");
        trun(store_init(src.clone())).unwrap();
        {
            let mut loom = cli_open_loom(&src, &KeyOpts::default()).unwrap();
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Files,
                    Some("work"),
                    WorkspaceId::from_bytes([7; 16]),
                )
                .unwrap();
            loom.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
            loom.commit(ns, "nas", "init", 1).unwrap();
            save_loom(&mut loom).unwrap();
            let config = AuditConfig {
                retention_days: 90,
                legal_hold: true,
            };
            loom.store()
                .save_audit_config_audited(config, None, "audit.config.set", None)
                .unwrap();
            loom.store()
                .save_store_policy_audited(
                    loom_store::StorePolicy {
                        fips_required: true,
                    },
                    None,
                    "store.policy.set",
                    None,
                )
                .unwrap();
            let listener = FileStore::served_listener_record(
                "cas",
                vec!["work".into()],
                "rest",
                "127.0.0.1:0",
                true,
            )
            .unwrap();
            loom.store()
                .save_served_listener_audited(&listener, None, "serve.listener.configure", None)
                .unwrap();
        }
        trun(Command::Store {
            action: StoreCmd::Copy {
                src: src.clone(),
                dst: dst.clone(),
                with: vec!["fips".into()],
                format: "json".into(),
                report_file: Some(report.clone()),
                dry_run: false,
                new_key_source: None,
            },
        })
        .unwrap();

        let copied = cli_open_loom_read(&dst, &KeyOpts::default()).unwrap();
        assert_eq!(copied.store().digest_algo(), Algo::Sha256);
        let ns = copied
            .registry()
            .open(&WsSelector::Name("work".into()))
            .unwrap();
        assert_eq!(copied.read_file(ns, "a.txt").unwrap(), b"alpha");
        assert_eq!(
            copied.store().audit_config().unwrap(),
            AuditConfig {
                retention_days: 90,
                legal_hold: true
            }
        );
        assert!(copied.store().store_policy().unwrap().fips_required);
        let listeners = copied.store().served_listeners().unwrap();
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0].surface, "cas");
        assert_eq!(listeners[0].selectors, vec!["work"]);
        assert!(!listeners[0].enabled);
        let report_json = std::fs::read_to_string(&report).unwrap();
        assert!(report_json.contains("\"destination_identity_profile\":\"sha256\""));
        assert!(report_json.contains("\"audit_policy_imported\":true"));
        assert!(report_json.contains("\"served_listeners_imported_disabled\":1"));
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
        let _ = std::fs::remove_file(&report);
    }

    #[test]
    fn store_copy_with_fips_preserves_encryption() {
        if cfg!(feature = "fips") {
            return;
        }
        let src = temp("copy-fips-encrypted-src", "loom");
        let dst = temp("copy-fips-encrypted-dst", "loom");
        let report = temp("copy-fips-encrypted-report", "json");
        let source_key = temp("copy-fips-source-key", "txt");
        let target_key = temp("copy-fips-target-key", "txt");
        std::fs::write(&source_key, "source-pass\n").unwrap();
        std::fs::write(&target_key, "target-pass\n").unwrap();
        let source_keys = KeyOpts {
            source: KeySource::File(source_key.clone()),
            ..KeyOpts::default()
        };
        run(
            Command::Store {
                action: StoreCmd::Init {
                    store: src.clone(),
                    encrypt: true,
                    suite: None,
                    identity_profile: None,
                    fips: false,
                },
            },
            &source_keys,
        )
        .unwrap();
        {
            let mut loom = cli_open_loom(&src, &source_keys).unwrap();
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Files,
                    Some("work"),
                    WorkspaceId::from_bytes([8; 16]),
                )
                .unwrap();
            loom.write_file(ns, "secret.txt", b"alpha", 0o100644)
                .unwrap();
            loom.commit(ns, "nas", "init", 1).unwrap();
            save_loom(&mut loom).unwrap();
        }
        let copy_keys = KeyOpts {
            source: KeySource::File(source_key.clone()),
            new_source: KeySource::File(target_key.clone()),
            ..KeyOpts::default()
        };
        run(
            Command::Store {
                action: StoreCmd::Copy {
                    src: src.clone(),
                    dst: dst.clone(),
                    with: vec!["fips".into()],
                    format: "json".into(),
                    report_file: Some(report.clone()),
                    dry_run: false,
                    new_key_source: None,
                },
            },
            &copy_keys,
        )
        .unwrap();

        let target_keys = KeyOpts {
            source: KeySource::File(target_key.clone()),
            ..KeyOpts::default()
        };
        let copied = cli_open_loom_read(&dst, &target_keys).unwrap();
        assert!(copied.store().is_encrypted());
        assert_eq!(copied.store().digest_algo(), Algo::Sha256);
        let ns = copied
            .registry()
            .open(&WsSelector::Name("work".into()))
            .unwrap();
        assert_eq!(copied.read_file(ns, "secret.txt").unwrap(), b"alpha");
        let report_json = std::fs::read_to_string(&report).unwrap();
        assert!(report_json.contains("\"source_encrypted\":true"));
        assert!(report_json.contains("\"destination_encrypted\":true"));
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
        let _ = std::fs::remove_file(&report);
        let _ = std::fs::remove_file(&source_key);
        let _ = std::fs::remove_file(&target_key);
    }

    #[test]
    fn store_policy_sets_fips_required_marker() {
        let store = temp("policy-fips-required", "loom");
        trun(store_init(store.clone())).unwrap();
        trun(Command::Store {
            action: StoreCmd::Policy {
                store: store.clone(),
                fips_required: Some(true),
            },
        })
        .unwrap();
        let fs = FileStore::open_read(&store).unwrap();
        assert!(fs.store_policy().unwrap().fips_required);
        assert!(
            fs.audit_records()
                .unwrap()
                .iter()
                .any(|record| record.action == "store.policy.set")
        );
        trun(Command::Store {
            action: StoreCmd::Policy {
                store: store.clone(),
                fips_required: None,
            },
        })
        .unwrap();
        let _ = std::fs::remove_file(&store);
    }

    fn files_write(store: String, workspace: &str, path: &str, input: String) -> Command {
        Command::Files {
            action: FilesCmd::Write {
                store,
                workspace: workspace.into(),
                path: path.into(),
                input,
            },
        }
    }

    fn files_read(store: String, workspace: &str, path: &str, out: Option<String>) -> Command {
        Command::Files {
            action: FilesCmd::Read {
                store,
                workspace: workspace.into(),
                path: path.into(),
                out,
            },
        }
    }

    fn vcs_commit(store: String, workspace: &str, message: &str) -> Command {
        Command::Vcs {
            action: VcsCmd::Commit {
                store,
                workspace: workspace.into(),
                message: message.into(),
                author: "t".into(),
            },
        }
    }

    fn vcs_branch(store: String, workspace: &str, branch: &str) -> Command {
        Command::Vcs {
            action: VcsCmd::Branch {
                store,
                workspace: workspace.into(),
                branch: branch.into(),
            },
        }
    }

    fn vcs_checkout(store: String, workspace: &str, branch: &str) -> Command {
        Command::Vcs {
            action: VcsCmd::Checkout {
                store,
                workspace: workspace.into(),
                branch: branch.into(),
            },
        }
    }

    fn vcs_merge(store: String, workspace: &str, from: &str) -> Command {
        Command::Vcs {
            action: VcsCmd::Merge {
                store,
                workspace: workspace.into(),
                from: from.into(),
                cells: false,
                author: "t".into(),
            },
        }
    }

    fn vcs_diff_cbor(
        store: String,
        workspace: &str,
        from: String,
        to: String,
        out: Option<String>,
    ) -> Command {
        Command::Vcs {
            action: VcsCmd::Diff {
                store,
                workspace: workspace.into(),
                from,
                to,
                format: "cbor".into(),
                out,
            },
        }
    }

    fn sql_exec(store: String, workspace: &str, sql: &str) -> Command {
        Command::Sql {
            action: SqlCmd::Exec {
                store,
                workspace: workspace.into(),
                db: "main".into(),
                sql: sql.into(),
            },
        }
    }

    fn sql_table(action: TableCmd) -> Command {
        Command::Sql {
            action: SqlCmd::Table { action },
        }
    }

    #[test]
    fn daemon_response_is_bound_to_protocol_and_store() {
        let response = "running\tprotocol=1\ttransport=tcp\t123\t/private/tmp/a.loom\n";
        let status = daemon::parse_response(response, "/private/tmp/a.loom").unwrap();
        assert_eq!(status.pid, "123");
        assert_eq!(status.store, "/private/tmp/a.loom");
        assert_eq!(status.store_id, "/private/tmp/a.loom");
        assert!(daemon::parse_response(response, "/private/tmp/b.loom").is_err());
        let response =
            "running\tprotocol=1\ttransport=tcp\t123\t/private/tmp/a.loom\tidentity=unix:1:2\n";
        let status =
            daemon::parse_response_expected(response, "/private/tmp/b.loom", "unix:1:2").unwrap();
        assert_eq!(status.pid, "123");
        assert_eq!(status.store, "/private/tmp/a.loom");
        assert_eq!(status.store_id, "unix:1:2");
        assert!(
            daemon::parse_response_expected(response, "/private/tmp/a.loom", "unix:9:9").is_err()
        );
        assert!(
            daemon::parse_response(
                "running\tprotocol=2\ttransport=tcp\t123\t/private/tmp/a.loom\n",
                "/private/tmp/a.loom"
            )
            .is_err()
        );
        assert!(
            daemon::parse_response(
                "running\tprotocol=1\ttransport=uds\t123\t/private/tmp/a.loom\n",
                "/private/tmp/a.loom"
            )
            .is_err()
        );
    }

    #[test]
    fn daemon_paths_are_scoped_by_canonical_store_path() {
        let a = temp("daemon-a", "loom");
        let b = temp("daemon-b", "loom");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let pa = daemon::paths(&a).unwrap();
        let pb = daemon::paths(&b).unwrap();
        assert_ne!(pa.addr_file, pb.addr_file);
        assert_ne!(pa.pid_file, pb.pid_file);
        assert_ne!(pa.lock_file, pb.lock_file);
        assert_ne!(pa.store_id, pb.store_id);
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    #[test]
    fn daemon_start_rejects_existing_direct_writer() {
        let store = temp("daemon-start-writer", "loom");
        let writer = FileStore::open(&store).unwrap();
        let err = daemon_start_with_transport(&store, "native").unwrap_err();
        assert!(err.contains("open for writing by another process"));
        drop(writer);
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn cli_open_loom_uses_daemon_authorized_open_when_daemon_is_active() {
        let store = temp("daemon-cli-open", "loom");
        FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let (paths, handle) = fake_daemon_for_store(&store, 1);

        let loom = cli_open_loom(&store, &KeyOpts::default()).unwrap();
        assert!(cli_loom_is_daemon_attached(&loom));
        let session = cli_loom_daemon_session_id(&loom).unwrap().to_string();
        drop(loom);
        let requests = handle.join().unwrap();
        assert_eq!(
            requests,
            vec![
                "status\n".to_string(),
                format!("session-attach\t{session}\n"),
                format!("session-detach\t{session}\n")
            ]
        );
        cleanup_fake_daemon(&paths);
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn cli_object_put_uses_daemon_session_when_daemon_is_active() {
        let store = temp("daemon-cli-put", "loom");
        FileStore::create_with_profile(&store, Algo::Blake3).unwrap();
        let (paths, handle) = fake_daemon_for_store(&store, 1);

        let fs = cli_open_store_for_write(&store).unwrap();
        assert!(cli_store_is_daemon_attached(&fs));
        let session = cli_store_daemon_session_id(&fs).unwrap().to_string();
        let digest = fs
            .put(&Object::Blob(b"daemon-routed".to_vec()).canonical())
            .unwrap();
        drop(fs);

        let read = FileStore::open_read(&store).unwrap();
        assert!(read.has(&digest).unwrap());
        let requests = handle.join().unwrap();
        assert_eq!(
            requests,
            vec![
                "status\n".to_string(),
                format!("session-attach\t{session}\n"),
                format!("session-detach\t{session}\n")
            ]
        );
        cleanup_fake_daemon(&paths);
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn key_source_grammar_parses() {
        assert!(matches!(parse_key_source("prompt"), Ok(KeySource::Prompt)));
        assert!(matches!(parse_key_source("file:/x/y"), Ok(KeySource::File(p)) if p == "/x/y"));
        assert!(matches!(parse_key_source("fd:0"), Ok(KeySource::Fd(0))));
        assert!(matches!(parse_key_source("fd:3"), Ok(KeySource::Fd(3))));
        assert!(parse_key_source("fd:nope").is_err());
        assert!(parse_key_source("env").is_err());
        assert!(parse_key_source("LOOM_PASSPHRASE").is_err());
        // Raw-KEK sources.
        assert!(
            matches!(parse_key_source("raw-kek:file:/k"), Ok(KeySource::RawKekFile(p)) if p == "/k")
        );
        assert!(matches!(
            parse_key_source("raw-kek:fd:0"),
            Ok(KeySource::RawKekFd(0))
        ));
        assert!(parse_key_source("raw-kek:nope").is_err());
    }

    #[test]
    fn raw_kek_hex_parses_and_acquires_a_kek_spec() {
        // 64 hex chars decode to a 32-byte KEK; wrong length / non-hex are rejected.
        let hex = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
        let kek = parse_hex_kek(hex).unwrap();
        assert_eq!(kek[0], 0x00);
        assert_eq!(kek[31], 0xff);
        assert!(parse_hex_kek("abcd").is_err());
        assert!(parse_hex_kek(&"zz".repeat(32)).is_err());
        // acquire_key_spec reads a raw-kek file and yields a RawKek credential.
        let p = temp("kek", "hex");
        std::fs::write(&p, format!("{hex}\n")).unwrap();
        let spec = acquire_key_spec(&KeySource::RawKekFile(p.clone()), "KEK", false).unwrap();
        assert!(matches!(spec, KeySpec::RawKek(_)));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn acquire_reads_a_passphrase_file_and_trims_newline() {
        let p = temp("pw", "txt");
        std::fs::write(&p, "swordfish\n").unwrap();
        let src = KeySource::File(p.clone());
        assert_eq!(acquire(&src, "Passphrase", false).unwrap(), "swordfish");
        // An empty file is rejected rather than yielding an unprotected key.
        std::fs::write(&p, "\n").unwrap();
        assert!(acquire(&src, "Passphrase", false).is_err());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn acquire_rejects_nonzero_fd_in_v1() {
        // Only fd:0 (stdin) is supported without `unsafe` in this crate.
        assert!(acquire(&KeySource::Fd(3), "Passphrase", false).is_err());
    }

    fn keys_with_new_passphrase(path: String) -> KeyOpts {
        KeyOpts {
            new_source: KeySource::File(path),
            ..KeyOpts::default()
        }
    }

    fn keys_with_auth(principal: WorkspaceId, path: String) -> KeyOpts {
        KeyOpts {
            auth_principal: Some(principal.to_string()),
            auth_source: KeySource::File(path),
            ..KeyOpts::default()
        }
    }

    fn source_with_signed_authority_handoff(
        store: &str,
        mirror: &str,
        stale: &str,
    ) -> (WorkspaceId, WorkspaceId) {
        trun(store_init(store.to_string())).unwrap();
        trun(store_init(mirror.to_string())).unwrap();
        trun(store_init(stale.to_string())).unwrap();

        let source = FileStore::open(store).unwrap();
        let mirror_store = FileStore::open(mirror).unwrap();
        let stale_store = FileStore::open(stale).unwrap();
        let mut identity = source.identity_store().unwrap().unwrap();
        let acl = source.acl_store().unwrap().unwrap();

        let from = identity.root_principal().unwrap();
        let to = random_workspace_id().unwrap();
        let key_id = random_workspace_id().unwrap();
        let signing_key = p256::ecdsa::SigningKey::from_slice(&[7; 32]).unwrap();
        let verifying_key = signing_key.verifying_key();
        let public_key = verifying_key.to_encoded_point(false).as_bytes().to_vec();
        identity
            .set_passphrase(from, "root-pass", &[3; 16])
            .unwrap();
        mirror_store.save_identity_store(&identity).unwrap();
        mirror_store.save_acl_store(&acl).unwrap();
        stale_store.save_identity_store(&identity).unwrap();
        stale_store.save_acl_store(&acl).unwrap();
        identity
            .add_principal(to, "replica-authority", PrincipalKind::Service)
            .unwrap();
        identity
            .add_public_key(
                from,
                loom_core::IdentityPublicKeySpec {
                    id: key_id,
                    label: "authority-key".to_string(),
                    algorithm: loom_core::IDENTITY_AUTHORITY_HANDOFF_ALG_ES256.to_string(),
                    public_key,
                },
            )
            .unwrap();

        let payload = loom_core::identity_authority_handoff_payload(from, to, 1, None);
        let signature: p256::ecdsa::Signature =
            p256::ecdsa::signature::Signer::sign(&signing_key, &payload);
        let signed_record = loom_core::identity_authority_handoff_record(
            from,
            to,
            1,
            None,
            loom_core::IDENTITY_AUTHORITY_HANDOFF_ALG_ES256,
            key_id.as_bytes(),
            signature.to_bytes().as_slice(),
        )
        .unwrap();
        identity
            .apply_verified_authority_handoff(
                loom_core::IdentityAuthorityHandoff {
                    from,
                    to,
                    generation: 1,
                    head: None,
                    signed_record,
                },
                true,
            )
            .unwrap();
        source.save_identity_store(&identity).unwrap();
        (from, to)
    }

    fn temp(tag: &str, ext: &str) -> String {
        static C: AtomicU64 = AtomicU64::new(0);
        let n = C.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("loomcli-{tag}-{}-{n}.{ext}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn identity_and_acl_management_through_the_cli() {
        let store = temp("identity-acl", "loom");
        let pass = temp("identity-acl-pass", "txt");
        std::fs::write(&pass, "root-pass\n").unwrap();

        trun(store_init(store.clone())).unwrap();
        let fs = FileStore::open(&store).unwrap();
        let identity = fs.identity_store().unwrap().unwrap();
        let root = identity.root_principal().unwrap();
        assert!(!identity.authenticated_mode());
        assert_eq!(fs.acl_store().unwrap().unwrap().grants().len(), 1);
        drop(fs);

        trun(identity_cmd(IdentityCmd::List {
            store: store.clone(),
        }))
        .unwrap();
        run(
            identity_cmd(IdentityCmd::SetPassphrase {
                store: store.clone(),
                principal: root.to_string(),
                new_key_source: None,
            }),
            &keys_with_new_passphrase(pass.clone()),
        )
        .unwrap();

        let err = trun(ns_list(store.clone())).unwrap_err();
        assert!(err.contains("authentication required"));

        let auth = keys_with_auth(root, pass.clone());
        run(
            identity_cmd(IdentityCmd::Add {
                store: store.clone(),
                handle: "alice".into(),
                name: "alice".into(),
                kind: "user".into(),
            }),
            &auth,
        )
        .unwrap();
        let identity = FileStore::open(&store)
            .unwrap()
            .identity_store()
            .unwrap()
            .unwrap();
        let identity_json = identity_list_json(&identity);
        assert!(identity_json.contains("\"authority\""), "{identity_json}");
        assert!(
            identity_json.contains("\"mode\":\"authority\""),
            "{identity_json}"
        );
        let alice = identity
            .principals()
            .find(|p| p.name == "alice")
            .unwrap()
            .id;
        run(
            acl_cmd(AclCmd::Grant {
                store: store.clone(),
                effect: "allow".into(),
                subject: alice.to_string(),
                rights: vec!["read".into(), "write".into()],
                workspace: None,
                domain: None,
                ref_glob: None,
                scopes: Vec::new(),
                predicate_cel: None,
            }),
            &auth,
        )
        .unwrap();
        let acl = FileStore::open(&store)
            .unwrap()
            .acl_store()
            .unwrap()
            .unwrap();
        assert!(acl.grants().iter().any(|grant| {
            grant.subject == AclSubject::Principal(alice)
                && grant.rights.contains(&AclRight::Read)
                && grant.rights.contains(&AclRight::Write)
        }));
        let reader_role = identity
            .roles()
            .find(|role| role.name == "reader")
            .unwrap()
            .id;
        run(
            acl_cmd(AclCmd::Grant {
                store: store.clone(),
                effect: "allow".into(),
                subject: format!("role:{reader_role}"),
                rights: vec!["read".into()],
                workspace: None,
                domain: None,
                ref_glob: None,
                scopes: Vec::new(),
                predicate_cel: None,
            }),
            &auth,
        )
        .unwrap();
        let acl = FileStore::open(&store)
            .unwrap()
            .acl_store()
            .unwrap()
            .unwrap();
        assert!(acl.grants().iter().any(|grant| {
            grant.subject == AclSubject::Role(reader_role) && grant.rights.contains(&AclRight::Read)
        }));
        run(
            acl_cmd(AclCmd::Grant {
                store: store.clone(),
                effect: "allow".into(),
                subject: alice.to_string(),
                rights: vec!["read".into()],
                workspace: None,
                domain: Some("kv".into()),
                ref_glob: Some("branch/main".into()),
                scopes: vec!["key:tenant/a/".into(), "key:tenant/b/".into()],
                predicate_cel: Some("principal == 'alice'".into()),
            }),
            &auth,
        )
        .unwrap();
        let acl = FileStore::open(&store)
            .unwrap()
            .acl_store()
            .unwrap()
            .unwrap();
        assert!(acl.grants().iter().any(|grant| {
            grant.subject == AclSubject::Principal(alice)
                && grant.facet == Some(FacetKind::Kv)
                && grant.ref_glob.as_deref() == Some("branch/main")
                && grant.scopes
                    == vec![
                        AclScope::Prefix {
                            kind: AclScopeKind::Key,
                            prefix: b"tenant/a/".to_vec(),
                        },
                        AclScope::Prefix {
                            kind: AclScopeKind::Key,
                            prefix: b"tenant/b/".to_vec(),
                        },
                    ]
                && grant.predicate.as_ref().is_some_and(|predicate| {
                    predicate.language == "cel" && predicate.expression == "principal == 'alice'"
                })
        }));
        run(
            acl_cmd(AclCmd::Revoke {
                store: store.clone(),
                effect: "allow".into(),
                subject: alice.to_string(),
                rights: vec!["read".into(), "write".into()],
                workspace: None,
                domain: None,
                ref_glob: None,
                scopes: Vec::new(),
                predicate_cel: None,
            }),
            &auth,
        )
        .unwrap();
        let acl = FileStore::open(&store)
            .unwrap()
            .acl_store()
            .unwrap()
            .unwrap();
        assert!(!acl.grants().iter().any(|grant| {
            grant.subject == AclSubject::Principal(alice)
                && grant.rights.contains(&AclRight::Read)
                && grant.rights.contains(&AclRight::Write)
        }));
        run(ns_list(store.clone()), &auth).unwrap();
        run(ns_create(store.clone(), "policy", Some("vcs")), &auth).unwrap();
        let loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let policy_ns = resolve_ns(&loom, "policy").unwrap();
        drop(loom);
        run(
            protected_ref_cmd(ProtectedRefCmd::Set {
                store: store.clone(),
                workspace: "policy".into(),
                ref_name: "branch/main".into(),
                fast_forward_only: true,
                signed_commits_required: false,
                signed_ref_advance_required: false,
                required_review_count: 0,
                retention_lock: true,
                governance_lock: false,
            }),
            &auth,
        )
        .unwrap();
        let loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        let policy = loom
            .protected_ref_policy(policy_ns, "branch/main")
            .unwrap()
            .unwrap();
        assert!(policy.fast_forward_only);
        assert!(policy.retention_lock);
        drop(loom);
        run(
            protected_ref_cmd(ProtectedRefCmd::List {
                store: store.clone(),
                workspace: "policy".into(),
            }),
            &auth,
        )
        .unwrap();
        run(
            protected_ref_cmd(ProtectedRefCmd::Get {
                store: store.clone(),
                workspace: "policy".into(),
                ref_name: "branch/main".into(),
            }),
            &auth,
        )
        .unwrap();
        run(
            protected_ref_cmd(ProtectedRefCmd::Remove {
                store: store.clone(),
                workspace: "policy".into(),
                ref_name: "branch/main".into(),
            }),
            &auth,
        )
        .unwrap();
        let loom = loom_store::open_loom_unlocked(&store, None).unwrap();
        assert!(
            loom.protected_ref_policy(policy_ns, "branch/main")
                .unwrap()
                .is_none()
        );
        let records = loom.store().audit_records().unwrap();
        assert!(records.iter().any(|record| {
            record.action == "protected_ref.set"
                && record
                    .target
                    .as_deref()
                    .is_some_and(|target| target.contains("ref=branch/main"))
        }));
        assert!(
            records
                .iter()
                .any(|record| record.action == "protected_ref.remove")
        );
        drop(loom);
        run(
            identity_cmd(IdentityCmd::ForceDetachAuthority {
                store: store.clone(),
                principal: root.to_string(),
                generation: 1,
                reason: "authority unreachable".into(),
            }),
            &auth,
        )
        .unwrap();
        let fs = FileStore::open(&store).unwrap();
        let identity = fs.identity_store().unwrap().unwrap();
        assert_eq!(
            identity.authority_state().mode,
            loom_core::IdentityAuthorityMode::Detached
        );
        assert_eq!(identity.authority_state().authority, root);
        assert_eq!(identity.authority_state().generation, 1);
        let identity_json = identity_list_json(&identity);
        assert!(
            identity_json.contains("\"forced_detach\""),
            "{identity_json}"
        );
        assert!(
            identity_json.contains("\"reason\":\"authority unreachable\""),
            "{identity_json}"
        );
        assert!(fs.audit_records().unwrap().iter().any(|record| {
            record.action == "identity.authority.force_detach"
                && record
                    .target
                    .as_deref()
                    .is_some_and(|target| target.contains("generation=1"))
        }));

        for p in [&store, &pass] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn identity_authority_replication_through_the_cli() {
        let source = temp("authority-source", "loom");
        let mirror = temp("authority-mirror", "loom");
        let stale = temp("authority-stale", "loom");
        let pass = temp("authority-pass", "txt");
        std::fs::write(&pass, "root-pass\n").unwrap();
        let (root, next_authority) = source_with_signed_authority_handoff(&source, &mirror, &stale);
        let auth = keys_with_auth(root, pass.clone());

        run(
            identity_cmd(IdentityCmd::ReplicateAuthority {
                store: mirror.clone(),
                source: source.clone(),
                become_authority: false,
            }),
            &auth,
        )
        .unwrap();
        let mirror_store = FileStore::open_read(&mirror).unwrap();
        let mirror_identity = mirror_store.identity_store().unwrap().unwrap();
        assert_eq!(
            mirror_identity.authority_state().mode,
            loom_core::IdentityAuthorityMode::Mirror
        );
        assert_eq!(mirror_identity.authority_state().authority, next_authority);
        assert_eq!(mirror_identity.authority_state().generation, 1);
        assert!(mirror_store.audit_records().unwrap().iter().any(|record| {
            record.action == "identity.authority.replicate"
                && record
                    .target
                    .as_deref()
                    .is_some_and(|target| target.contains("applied=true"))
        }));

        run(
            identity_cmd(IdentityCmd::ConfigureAuthorityReplication {
                store: mirror.clone(),
                id: "office".to_string(),
                source: source.clone(),
                disabled: false,
                pull_on_start: true,
                interval_ms: Some(30_000),
                jitter_ms: 1_000,
                backoff_ms: 5_000,
                publish_witness: true,
            }),
            &auth,
        )
        .unwrap();
        run(
            identity_cmd(IdentityCmd::ListAuthorityReplication {
                store: mirror.clone(),
            }),
            &auth,
        )
        .unwrap();
        let mirror_store = FileStore::open_read(&mirror).unwrap();
        let policies = mirror_store.authority_replication_policies().unwrap();
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].id, "office");
        assert_eq!(policies[0].source, source.as_str());
        assert_eq!(policies[0].interval_ms, Some(30_000));
        assert!(mirror_store.audit_records().unwrap().iter().any(|record| {
            record.action == "authority.replication.configure"
                && record
                    .target
                    .as_deref()
                    .is_some_and(|target| target.contains("id=office"))
        }));
        run(
            identity_cmd(IdentityCmd::RemoveAuthorityReplication {
                store: mirror.clone(),
                id: "office".to_string(),
            }),
            &auth,
        )
        .unwrap();
        assert!(
            FileStore::open(&mirror)
                .unwrap()
                .authority_replication_policies()
                .unwrap()
                .is_empty()
        );

        let err = run(
            identity_cmd(IdentityCmd::ReplicateAuthority {
                store: source.clone(),
                source: stale.clone(),
                become_authority: false,
            }),
            &auth,
        )
        .unwrap_err();
        assert!(err.contains("authority source generation is behind destination"));

        for p in [&source, &mirror, &stale, &pass] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn kv_config_through_the_cli_persists() {
        let store = temp("kv-config", "loom");
        trun(store_init(store.clone())).unwrap();
        trun(management_kv_config(ManagementKvConfigCmd::Set {
            store: store.clone(),
            workspace: "cache-ns".into(),
            name: "sessions".into(),
            tier: "ephemeral".into(),
            default_ttl_ms: 100,
            default_idle_ttl_ms: 25,
            read_through: true,
            write_through: true,
        }))
        .unwrap();
        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "cache-ns").unwrap();
        let config = loom.kv_map_config(ns, "sessions");
        assert_eq!(config.tier, KvTier::Ephemeral);
        assert_eq!(config.default_put.ttl_ms, Some(100));
        assert_eq!(config.default_put.idle_ttl_ms, Some(25));
        assert!(config.read_through);
        assert!(config.write_through);
        drop(loom);

        trun(management_kv_config(ManagementKvConfigCmd::Get {
            store: store.clone(),
            workspace: "cache-ns".into(),
            name: "sessions".into(),
        }))
        .unwrap();
        trun(management_kv_config(ManagementKvConfigCmd::Set {
            store: store.clone(),
            workspace: "cache-ns".into(),
            name: "sessions".into(),
            tier: "versioned".into(),
            default_ttl_ms: 0,
            default_idle_ttl_ms: 0,
            read_through: false,
            write_through: false,
        }))
        .unwrap();
        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        assert_eq!(loom.kv_map_config(ns, "sessions"), KvMapConfig::VERSIONED);

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn audit_cli_config_list_view_and_compact() {
        let store = temp("audit-cli", "loom");
        trun(store_init(store.clone())).unwrap();
        trun(audit_cmd(AuditCmd::Config {
            action: AuditConfigCmd::Show {
                store: store.clone(),
            },
        }))
        .unwrap();
        trun(audit_cmd(AuditCmd::Config {
            action: AuditConfigCmd::Set {
                store: store.clone(),
                retention_days: Some(90),
                legal_hold: Some(false),
            },
        }))
        .unwrap();
        trun(audit_cmd(AuditCmd::List {
            store: store.clone(),
        }))
        .unwrap();
        trun(audit_cmd(AuditCmd::View {
            store: store.clone(),
            record: "0".into(),
        }))
        .unwrap();
        trun(audit_cmd(AuditCmd::Compact {
            store: store.clone(),
            through_seq: 1,
        }))
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        assert_eq!(
            loom.store().audit_config().unwrap(),
            AuditConfig {
                retention_days: 90,
                legal_hold: false,
            }
        );
        let records = loom.store().audit_records().unwrap();
        assert!(records.iter().any(|record| record.action == "audit.list"));
        assert!(records.iter().any(|record| record.action == "audit.view"));
        assert!(records.iter().any(|record| record.action == "audit.prune"));

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn serve_command_persists_listener_and_audit_record() {
        let store = temp("serve-cas", "loom");
        trun(store_init(store.clone())).unwrap();
        trun(serve_command(
            store.clone(),
            "cas",
            vec!["main"],
            "127.0.0.1:8001",
        ))
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let listeners = loom.store().served_listeners().unwrap();
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0].surface, "cas");
        assert_eq!(listeners[0].selectors, vec!["main"]);
        assert_eq!(listeners[0].transport, "rest");
        assert_eq!(listeners[0].profile, None);
        assert_eq!(listeners[0].bind, "127.0.0.1:8001");
        assert!(listeners[0].enabled);
        assert_eq!(listeners[0].schema_version, 3);
        assert_eq!(listeners[0].last_modified_audit_seq, Some(0));
        assert_eq!(listeners[0].route_scope, "workspace");
        assert_eq!(listeners[0].auth.mode, "owner-or-passphrase");
        assert_eq!(listeners[0].audit.mode, "management-and-security");
        let records = loom.store().audit_records().unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.action == "serve.listener.configure")
        );

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn serve_command_persists_vector_profile() {
        let store = temp("serve-vector-profile", "loom");
        trun(store_init(store.clone())).unwrap();
        trun(Command::Serve {
            action: ServeCmd::Configure(Box::new(ServeConfigureArgs {
                store: store.clone(),
                surface: "vector".into(),
                selector: vec!["main".into(), "embeddings".into()],
                bind: "127.0.0.1:8011".into(),
                transport: Some("rest".into()),
                profile: Some("qdrant".into()),
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
        let listener = loom.store().served_listeners().unwrap().remove(0);
        assert_eq!(listener.surface, "vector");
        assert_eq!(listener.selectors, vec!["main", "embeddings"]);
        assert_eq!(listener.transport, "rest");
        assert_eq!(listener.profile.as_deref(), Some("qdrant"));
        assert_eq!(listener.bind, "127.0.0.1:8011");
        assert_eq!(listener.route_scope, "workspace-collection");

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn serve_command_requires_vector_profile_for_generic_transports() {
        let store = temp("serve-vector-profile-required", "loom");
        trun(store_init(store.clone())).unwrap();
        let err = trun(Command::Serve {
            action: ServeCmd::Configure(Box::new(ServeConfigureArgs {
                store: store.clone(),
                surface: "vector".into(),
                selector: vec!["main".into(), "embeddings".into()],
                bind: "127.0.0.1:8012".into(),
                transport: Some("rest".into()),
                profile: None,
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
        .unwrap_err();
        assert!(err.contains("requires explicit --profile"));

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn serve_command_rejects_qdrant_as_transport() {
        let store = temp("serve-vector-qdrant-transport", "loom");
        trun(store_init(store.clone())).unwrap();
        let err = trun(Command::Serve {
            action: ServeCmd::Configure(Box::new(ServeConfigureArgs {
                store: store.clone(),
                surface: "vector".into(),
                selector: vec!["main".into(), "embeddings".into()],
                bind: "127.0.0.1:8013".into(),
                transport: Some("qdrant_rest".into()),
                profile: Some("qdrant".into()),
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
        .unwrap_err();
        assert!(err.contains("unsupported served transport"));

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn serve_command_persists_listener_policy_fields() {
        let store = temp("serve-policy", "loom");
        trun(store_init(store.clone())).unwrap();
        let loom = cli_open_loom(&store, &KeyOpts::default()).unwrap();
        let actor = require_global_admin_actor(&loom).unwrap();
        let record = loom
            .store()
            .certificate_bundle_record(
                "admin",
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
                Some("name=admin"),
                true,
            )
            .unwrap();
        drop(loom);
        trun(Command::Serve {
            action: ServeCmd::Configure(Box::new(ServeConfigureArgs {
                store: store.clone(),
                surface: "admin".into(),
                selector: Vec::new(),
                bind: "127.0.0.1:8003".into(),
                transport: Some("rest".into()),
                profile: None,
                disabled: true,
                tls_certificate_bundle: Some("admin".into()),
                tls_mode: None,
                auth_mode: Some("passphrase".into()),
                exposure: Some("read-only".into()),
                audit_mode: Some("all".into()),
                request_size_limit: Some(4096),
                idle_timeout_ms: Some(5000),
                session_timeout_ms: Some(6000),
                network_access_policy: None,
            })),
        })
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let listener = loom.store().served_listeners().unwrap().remove(0);
        assert!(!listener.enabled);
        assert_eq!(listener.surface, "admin");
        assert_eq!(listener.tls.mode, "direct");
        assert_eq!(
            listener.tls.certificate_bundle_ref.as_deref(),
            Some("admin")
        );
        assert_eq!(listener.auth.mode, "passphrase");
        assert_eq!(listener.exposure, "read-only");
        assert_eq!(listener.audit.mode, "all");
        assert_eq!(listener.limits.request_size_limit, 4096);
        assert_eq!(listener.limits.idle_timeout_ms, 5000);
        assert_eq!(listener.limits.session_timeout_ms, 6000);

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn serve_command_persists_smtp_starttls_policy() {
        let store = temp("serve-smtp-starttls", "loom");
        trun(store_init(store.clone())).unwrap();
        let loom = cli_open_loom(&store, &KeyOpts::default()).unwrap();
        let actor = require_global_admin_actor(&loom).unwrap();
        let record = loom
            .store()
            .certificate_bundle_record(
                "smtp",
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
                Some("name=smtp"),
                true,
            )
            .unwrap();
        drop(loom);

        trun(Command::Serve {
            action: ServeCmd::Configure(Box::new(ServeConfigureArgs {
                store: store.clone(),
                surface: "mail".into(),
                selector: vec!["main".into()],
                bind: "127.0.0.1:8025".into(),
                transport: Some("smtp".into()),
                profile: None,
                disabled: false,
                tls_certificate_bundle: Some("smtp".into()),
                tls_mode: Some("starttls".into()),
                auth_mode: Some("passphrase".into()),
                exposure: Some("read-write".into()),
                audit_mode: Some("all".into()),
                request_size_limit: None,
                idle_timeout_ms: None,
                session_timeout_ms: None,
                network_access_policy: None,
            })),
        })
        .unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let listener = loom.store().served_listeners().unwrap().remove(0);
        assert_eq!(listener.surface, "mail");
        assert_eq!(listener.transport, "smtp");
        assert_eq!(listener.tls.mode, "starttls");
        assert_eq!(listener.tls.certificate_bundle_ref.as_deref(), Some("smtp"));
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn serve_listener_management_is_audited() {
        let store = temp("serve-manage", "loom");
        trun(store_init(store.clone())).unwrap();
        trun(serve_command(
            store.clone(),
            "cas",
            vec!["main"],
            "127.0.0.1:8002",
        ))
        .unwrap();
        let id = cli_open_loom_read(&store, &KeyOpts::default())
            .unwrap()
            .store()
            .served_listeners()
            .unwrap()[0]
            .id
            .clone();

        trun(serve_action("list", store.clone(), Vec::new())).unwrap();
        trun(serve_action("disable", store.clone(), vec![id.clone()])).unwrap();
        assert!(
            !cli_open_loom_read(&store, &KeyOpts::default())
                .unwrap()
                .store()
                .served_listener(&id)
                .unwrap()
                .unwrap()
                .enabled
        );
        trun(serve_action("enable", store.clone(), vec![id.clone()])).unwrap();
        assert!(
            cli_open_loom_read(&store, &KeyOpts::default())
                .unwrap()
                .store()
                .served_listener(&id)
                .unwrap()
                .unwrap()
                .enabled
        );
        trun(serve_action("remove", store.clone(), vec![id.clone()])).unwrap();

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        assert!(loom.store().served_listener(&id).unwrap().is_none());
        let actions = loom
            .store()
            .audit_records()
            .unwrap()
            .into_iter()
            .map(|record| record.action)
            .collect::<Vec<_>>();
        for action in [
            "serve.listener.list",
            "serve.listener.disable",
            "serve.listener.enable",
            "serve.listener.remove",
        ] {
            assert!(actions.iter().any(|record| record == action), "{action}");
        }

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn put_then_get_round_trips_through_a_loom_file() {
        let store = temp("store", "loom");
        let infile = temp("in", "bin");
        let outfile = temp("out", "bin");
        let payload = b"hello from the loom cli".to_vec();
        std::fs::write(&infile, &payload).unwrap();

        trun(store_init(store.clone())).unwrap();
        trun(store_put(store.clone(), infile.clone())).unwrap();

        // `put` prints the address; recompute it independently to drive `get`.
        let digest = Object::Blob(payload.clone()).digest().to_string();
        trun(store_get(store.clone(), digest, Some(outfile.clone()))).unwrap();
        assert_eq!(std::fs::read(&outfile).unwrap(), payload);

        // The object persisted to the file; control records may also be present.
        let fs = FileStore::open(&store).unwrap();
        let digest = Object::Blob(payload.clone()).digest();
        assert!(fs.get(&digest).unwrap().is_some());

        for p in [&store, &infile, &outfile] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn get_reports_a_missing_object() {
        let store = temp("missing", "loom");
        trun(store_init(store.clone())).unwrap();
        let absent = Object::Blob(b"nope".to_vec()).digest().to_string();
        let err = trun(store_get(store.clone(), absent, None)).unwrap_err();
        assert!(err.contains("not found"));
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn files_history_through_the_cli() {
        let store = temp("files", "loom");
        let infile = temp("in", "txt");
        let outfile = temp("out", "txt");
        std::fs::write(&infile, b"hello").unwrap();
        trun(ns_create(store.clone(), "proj", Some("files"))).unwrap();
        let proj_id = cli_open_loom(&store, &KeyOpts::default())
            .unwrap()
            .registry()
            .open(&WsSelector::Name("proj".into()))
            .unwrap()
            .to_string();
        trun(files_write(
            store.clone(),
            "proj",
            "README.md",
            infile.clone(),
        ))
        .unwrap();
        trun(vcs_commit(store.clone(), &proj_id, "init")).unwrap();
        trun(vcs_branch(store.clone(), "proj", "feature")).unwrap();
        trun(vcs_checkout(store.clone(), "proj", "feature")).unwrap();
        // Read the committed file back through the CLI after the branch switch.
        trun(files_read(
            store.clone(),
            "proj",
            "README.md",
            Some(outfile.clone()),
        ))
        .unwrap();
        assert_eq!(std::fs::read(&outfile).unwrap(), b"hello");

        // The workspace, its commit, and both branches persisted to the file.
        let loom = cli_open_loom(&store, &KeyOpts::default()).unwrap();
        assert_eq!(loom.registry().list(None).len(), 1);

        for p in [&store, &infile, &outfile] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn generic_write_rejects_reserved_facet_storage() {
        let store = temp("write-sql", "loom");
        let infile = temp("write-sql-in", "txt");
        std::fs::write(&infile, b"raw").unwrap();
        trun(ns_create(store.clone(), "proj", Some("sql"))).unwrap();

        let err = trun(files_write(
            store.clone(),
            "proj",
            ".loom/facets/sql/main/catalog",
            infile.clone(),
        ))
        .unwrap_err();
        assert!(err.contains("reserved"));

        for p in [&store, &infile] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn workspace_lifecycle_through_the_cli() {
        let store = temp("ns-lifecycle", "loom");
        trun(ns_create(store.clone(), "work", None)).unwrap();
        let work_id = cli_open_loom(&store, &KeyOpts::default())
            .unwrap()
            .registry()
            .open(&WsSelector::Name("work".into()))
            .unwrap();

        trun(ns_rename(store.clone(), work_id.to_string(), "client-a")).unwrap();
        {
            let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
            assert_eq!(loom.registry().name(work_id).unwrap(), "client-a");
            assert!(
                loom.registry()
                    .open(&WsSelector::Name("work".into()))
                    .is_err()
            );
        }

        trun(ns_delete(store.clone(), "client-a")).unwrap();
        {
            let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
            assert!(loom.registry().open(&WsSelector::Id(work_id)).is_err());
        }

        trun(ns_create(store.clone(), "client-a", Some("files"))).unwrap();
        assert_eq!(
            cli_open_loom(&store, &KeyOpts::default())
                .unwrap()
                .registry()
                .list(Some(FacetKind::Files))
                .len(),
            1
        );

        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn sql_through_the_cli_persists_into_a_workspace() {
        let store = temp("sql", "loom");
        trun(ns_create(store.clone(), "db", Some("sql"))).unwrap();
        // CREATE then INSERT in separate invocations: state must round-trip through the working tree.
        trun(sql_exec(
            store.clone(),
            "db",
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)",
        ))
        .unwrap();
        trun(sql_exec(
            store.clone(),
            "db",
            "INSERT INTO t VALUES (1,'a'),(2,'b')",
        ))
        .unwrap();

        // The two rows persisted into the workspace's working tree (each `run` opens the store fresh).
        let loom = cli_open_loom(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "db").unwrap();
        let table = loom
            .read_table(ns, ".loom/facets/sql/main/tables/t")
            .unwrap();
        assert_eq!(table.len(), 2);
        let _ = std::fs::remove_file(&store);
    }

    #[test]
    fn promoted_data_facets_through_the_cli_persist() {
        let store = temp("data-facets", "loom");
        let input = temp("data-facets-in", "bin");
        let out = temp("data-facets-out", "bin");
        let out2 = temp("data-facets-out2", "bin");
        let key = temp("data-facets-key", "cbor");
        let key_hi = temp("data-facets-key-hi", "cbor");
        std::fs::write(&input, b"alpha").unwrap();
        std::fs::write(
            &key,
            loom_core::key_to_cbor(&loom_core::Value::Text("a".into())),
        )
        .unwrap();
        std::fs::write(
            &key_hi,
            loom_core::key_to_cbor(&loom_core::Value::Text("z".into())),
        )
        .unwrap();

        trun(Command::Cas {
            action: CasCmd::Put {
                store: store.clone(),
                workspace: "cas".into(),
                input: input.clone(),
            },
        })
        .unwrap();
        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let cas_ns = resolve_ns(&loom, "cas").unwrap();
        let digest = loom_core::cas_list(&loom, cas_ns).unwrap()[0];
        drop(loom);
        trun(Command::Cas {
            action: CasCmd::Get {
                store: store.clone(),
                workspace: "cas".into(),
                digest: digest.to_string(),
                out: Some(out.clone()),
            },
        })
        .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"alpha");

        trun(Command::Document {
            action: DocumentCmd::PutText {
                store: store.clone(),
                workspace: "docs".into(),
                collection: "pages".into(),
                id: "home".into(),
                input: input.clone(),
                expected_entity_tag: None,
            },
        })
        .unwrap();
        trun(Command::Document {
            action: DocumentCmd::GetText {
                store: store.clone(),
                workspace: "docs".into(),
                collection: "pages".into(),
                id: "home".into(),
                out: Some(out.clone()),
            },
        })
        .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"alpha");

        trun(Command::Kv {
            action: KvCmd::Put {
                store: store.clone(),
                workspace: "kv".into(),
                collection: "settings".into(),
                key: key.clone(),
                input: input.clone(),
            },
        })
        .unwrap();
        trun(Command::Kv {
            action: KvCmd::Get {
                store: store.clone(),
                workspace: "kv".into(),
                collection: "settings".into(),
                key: key.clone(),
                out: Some(out.clone()),
            },
        })
        .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"alpha");
        trun(Command::Kv {
            action: KvCmd::Range {
                store: store.clone(),
                workspace: "kv".into(),
                collection: "settings".into(),
                from: key.clone(),
                to: key_hi.clone(),
                out: Some(out2.clone()),
            },
        })
        .unwrap();
        assert!(!std::fs::read(&out2).unwrap().is_empty());

        trun(Command::Queue {
            action: QueueCmd::Append {
                store: store.clone(),
                workspace: "queue".into(),
                stream: "events".into(),
                input: input.clone(),
            },
        })
        .unwrap();
        trun(Command::Queue {
            action: QueueCmd::Get {
                store: store.clone(),
                workspace: "queue".into(),
                stream: "events".into(),
                seq: 0,
                out: Some(out.clone()),
            },
        })
        .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"alpha");
        trun(Command::Queue {
            action: QueueCmd::Advance {
                store: store.clone(),
                workspace: "queue".into(),
                stream: "events".into(),
                consumer: "worker".into(),
                next: 1,
            },
        })
        .unwrap();
        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let queue_ns = resolve_ns(&loom, "queue").unwrap();
        assert_eq!(
            loom_core::log::consumer_position(&loom, queue_ns, "events", "worker").unwrap(),
            1
        );
        drop(loom);

        trun(Command::TimeSeries {
            action: TimeSeriesCmd::Put {
                store: store.clone(),
                workspace: "metrics".into(),
                series: "cpu".into(),
                timestamp: 100,
                input: input.clone(),
            },
        })
        .unwrap();
        trun(Command::TimeSeries {
            action: TimeSeriesCmd::Get {
                store: store.clone(),
                workspace: "metrics".into(),
                series: "cpu".into(),
                timestamp: 100,
                out: Some(out.clone()),
            },
        })
        .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"alpha");

        for p in [&store, &input, &out, &out2, &key, &key_hi] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn pim_data_facets_through_the_cli_persist() {
        let store = temp("pim-facets", "loom");
        let calendar_in = temp("pim-calendar-in", "ics");
        let calendar_out = temp("pim-calendar-out", "ics");
        let calendar_record = temp("pim-calendar-record", "cbor");
        let calendar_list = temp("pim-calendar-list", "cbor");
        let calendar_range = temp("pim-calendar-range", "cbor");
        let contact_in = temp("pim-contact-in", "vcf");
        let contact_out = temp("pim-contact-out", "vcf");
        let contact_record = temp("pim-contact-record", "cbor");
        let contact_list = temp("pim-contact-list", "cbor");
        let mail_in = temp("pim-mail-in", "eml");
        let mail_out = temp("pim-mail-out", "eml");
        let mail_record = temp("pim-mail-record", "cbor");
        let mail_list = temp("pim-mail-list", "cbor");
        let mail_flags = temp("pim-mail-flags", "cbor");

        std::fs::write(
            &calendar_in,
            b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:u1\r\nSUMMARY:Standup\r\nDTSTART:20240101T090000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        std::fs::write(
            &contact_in,
            b"BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c1\r\nFN:Ada Lovelace\r\nEMAIL:ada@example.com\r\nEND:VCARD\r\n",
        )
        .unwrap();
        std::fs::write(
            &mail_in,
            b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Lunch\r\nMessage-ID: <m1@example.com>\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\n\r\nHello",
        )
        .unwrap();

        trun(Command::Calendar {
            action: CalendarCmd::CreateCollection {
                store: store.clone(),
                workspace: "cal".into(),
                principal: "alice".into(),
                collection: "work".into(),
                display_name: "Work".into(),
                component: vec!["event".into()],
            },
        })
        .unwrap();
        trun(Command::Calendar {
            action: CalendarCmd::PutIcs {
                store: store.clone(),
                workspace: "cal".into(),
                principal: "alice".into(),
                collection: "work".into(),
                input: calendar_in.clone(),
            },
        })
        .unwrap();
        trun(Command::Calendar {
            action: CalendarCmd::GetEntry {
                store: store.clone(),
                workspace: "cal".into(),
                principal: "alice".into(),
                collection: "work".into(),
                uid: "u1".into(),
                out: Some(calendar_record.clone()),
            },
        })
        .unwrap();
        let entry =
            loom_core::calendar::CalendarEntry::decode(&std::fs::read(&calendar_record).unwrap())
                .unwrap();
        assert_eq!(entry.summary, "Standup");
        trun(Command::Calendar {
            action: CalendarCmd::ToIcs {
                store: store.clone(),
                workspace: "cal".into(),
                principal: "alice".into(),
                collection: "work".into(),
                uid: "u1".into(),
                out: Some(calendar_out.clone()),
            },
        })
        .unwrap();
        assert!(
            String::from_utf8(std::fs::read(&calendar_out).unwrap())
                .unwrap()
                .contains("UID:u1")
        );
        trun(Command::Calendar {
            action: CalendarCmd::Search {
                store: store.clone(),
                workspace: "cal".into(),
                principal: "alice".into(),
                collection: "work".into(),
                component: Some("event".into()),
                text: Some("stand".into()),
                out: Some(calendar_list.clone()),
            },
        })
        .unwrap();
        assert!(matches!(
            loom_codec::decode(&std::fs::read(&calendar_list).unwrap()).unwrap(),
            loom_codec::Value::Array(items) if items.len() == 1
        ));
        trun(Command::Calendar {
            action: CalendarCmd::Range {
                store: store.clone(),
                workspace: "cal".into(),
                principal: "alice".into(),
                collection: "work".into(),
                from: "20240101T000000".into(),
                to: "20240102T000000".into(),
                out: Some(calendar_range.clone()),
            },
        })
        .unwrap();
        assert!(matches!(
            loom_codec::decode(&std::fs::read(&calendar_range).unwrap()).unwrap(),
            loom_codec::Value::Array(items) if items.len() == 1
        ));

        trun(Command::Contacts {
            action: ContactsCmd::CreateBook {
                store: store.clone(),
                workspace: "contacts".into(),
                principal: "alice".into(),
                book: "people".into(),
                display_name: "People".into(),
            },
        })
        .unwrap();
        trun(Command::Contacts {
            action: ContactsCmd::PutVcard {
                store: store.clone(),
                workspace: "contacts".into(),
                principal: "alice".into(),
                book: "people".into(),
                input: contact_in.clone(),
            },
        })
        .unwrap();
        trun(Command::Contacts {
            action: ContactsCmd::GetEntry {
                store: store.clone(),
                workspace: "contacts".into(),
                principal: "alice".into(),
                book: "people".into(),
                uid: "c1".into(),
                out: Some(contact_record.clone()),
            },
        })
        .unwrap();
        let contact =
            loom_core::contacts::ContactEntry::decode(&std::fs::read(&contact_record).unwrap())
                .unwrap();
        assert_eq!(contact.full_name, "Ada Lovelace");
        trun(Command::Contacts {
            action: ContactsCmd::ToVcard {
                store: store.clone(),
                workspace: "contacts".into(),
                principal: "alice".into(),
                book: "people".into(),
                uid: "c1".into(),
                out: Some(contact_out.clone()),
            },
        })
        .unwrap();
        assert!(
            String::from_utf8(std::fs::read(&contact_out).unwrap())
                .unwrap()
                .contains("UID:c1")
        );
        trun(Command::Contacts {
            action: ContactsCmd::Search {
                store: store.clone(),
                workspace: "contacts".into(),
                principal: "alice".into(),
                book: "people".into(),
                text: "ada".into(),
                out: Some(contact_list.clone()),
            },
        })
        .unwrap();
        assert!(matches!(
            loom_codec::decode(&std::fs::read(&contact_list).unwrap()).unwrap(),
            loom_codec::Value::Array(items) if items.len() == 1
        ));

        trun(Command::Mail {
            action: MailCmd::CreateMailbox {
                store: store.clone(),
                workspace: "mail".into(),
                principal: "alice".into(),
                mailbox: "inbox".into(),
                display_name: "Inbox".into(),
            },
        })
        .unwrap();
        trun(Command::Mail {
            action: MailCmd::IngestMessage {
                store: store.clone(),
                workspace: "mail".into(),
                principal: "alice".into(),
                mailbox: "inbox".into(),
                uid: "m1".into(),
                input: mail_in.clone(),
            },
        })
        .unwrap();
        trun(Command::Mail {
            action: MailCmd::GetMessage {
                store: store.clone(),
                workspace: "mail".into(),
                principal: "alice".into(),
                mailbox: "inbox".into(),
                uid: "m1".into(),
                out: Some(mail_record.clone()),
            },
        })
        .unwrap();
        let message =
            loom_core::mail::MailMessage::decode(&std::fs::read(&mail_record).unwrap()).unwrap();
        assert_eq!(message.subject, "Lunch");
        trun(Command::Mail {
            action: MailCmd::ToEml {
                store: store.clone(),
                workspace: "mail".into(),
                principal: "alice".into(),
                mailbox: "inbox".into(),
                uid: "m1".into(),
                out: Some(mail_out.clone()),
            },
        })
        .unwrap();
        assert_eq!(
            std::fs::read(&mail_out).unwrap(),
            std::fs::read(&mail_in).unwrap()
        );
        trun(Command::Mail {
            action: MailCmd::SetFlags {
                store: store.clone(),
                workspace: "mail".into(),
                principal: "alice".into(),
                mailbox: "inbox".into(),
                uid: "m1".into(),
                flags: vec!["seen".into(), "work".into()],
            },
        })
        .unwrap();
        trun(Command::Mail {
            action: MailCmd::GetFlags {
                store: store.clone(),
                workspace: "mail".into(),
                principal: "alice".into(),
                mailbox: "inbox".into(),
                uid: "m1".into(),
                out: Some(mail_flags.clone()),
            },
        })
        .unwrap();
        assert_eq!(
            loom_codec::decode(&std::fs::read(&mail_flags).unwrap()).unwrap(),
            loom_codec::Value::Array(vec![
                loom_codec::Value::Text("seen".into()),
                loom_codec::Value::Text("work".into())
            ])
        );
        trun(Command::Mail {
            action: MailCmd::Search {
                store: store.clone(),
                workspace: "mail".into(),
                principal: "alice".into(),
                mailbox: "inbox".into(),
                text: "lunch".into(),
                out: Some(mail_list.clone()),
            },
        })
        .unwrap();
        assert!(matches!(
            loom_codec::decode(&std::fs::read(&mail_list).unwrap()).unwrap(),
            loom_codec::Value::Array(items) if items.len() == 1
        ));

        for p in [
            &store,
            &calendar_in,
            &calendar_out,
            &calendar_record,
            &calendar_list,
            &calendar_range,
            &contact_in,
            &contact_out,
            &contact_record,
            &contact_list,
            &mail_in,
            &mail_out,
            &mail_record,
            &mail_list,
            &mail_flags,
        ] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn pim_cli_reports_missing_containers() {
        let store = temp("pim-missing", "loom");
        let calendar_in = temp("pim-missing-calendar", "ics");
        let contact_in = temp("pim-missing-contact", "vcf");
        let mail_in = temp("pim-missing-mail", "eml");
        std::fs::write(
            &calendar_in,
            b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:u1\r\nSUMMARY:Standup\r\nDTSTART:20240101T090000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        )
        .unwrap();
        std::fs::write(
            &contact_in,
            b"BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c1\r\nFN:Ada Lovelace\r\nEND:VCARD\r\n",
        )
        .unwrap();
        std::fs::write(
            &mail_in,
            b"From: alice@example.com\r\nSubject: Hi\r\n\r\nHello",
        )
        .unwrap();
        let is_not_found = |err: &str| {
            let err = err.to_ascii_lowercase();
            err.contains("not_found") || err.contains("not found")
        };

        trun(Command::Calendar {
            action: CalendarCmd::CreateCollection {
                store: store.clone(),
                workspace: "cal".into(),
                principal: "alice".into(),
                collection: "work".into(),
                display_name: "Work".into(),
                component: vec!["event".into()],
            },
        })
        .unwrap();
        let calendar_err = trun(Command::Calendar {
            action: CalendarCmd::PutIcs {
                store: store.clone(),
                workspace: "cal".into(),
                principal: "alice".into(),
                collection: "missing".into(),
                input: calendar_in.clone(),
            },
        })
        .unwrap_err();
        assert!(is_not_found(&calendar_err), "{calendar_err}");

        trun(Command::Contacts {
            action: ContactsCmd::CreateBook {
                store: store.clone(),
                workspace: "contacts".into(),
                principal: "alice".into(),
                book: "people".into(),
                display_name: "People".into(),
            },
        })
        .unwrap();
        let contacts_err = trun(Command::Contacts {
            action: ContactsCmd::PutVcard {
                store: store.clone(),
                workspace: "contacts".into(),
                principal: "alice".into(),
                book: "missing".into(),
                input: contact_in.clone(),
            },
        })
        .unwrap_err();
        assert!(is_not_found(&contacts_err), "{contacts_err}");

        trun(Command::Mail {
            action: MailCmd::CreateMailbox {
                store: store.clone(),
                workspace: "mail".into(),
                principal: "alice".into(),
                mailbox: "inbox".into(),
                display_name: "Inbox".into(),
            },
        })
        .unwrap();
        let mail_err = trun(Command::Mail {
            action: MailCmd::IngestMessage {
                store: store.clone(),
                workspace: "mail".into(),
                principal: "alice".into(),
                mailbox: "missing".into(),
                uid: "m1".into(),
                input: mail_in.clone(),
            },
        })
        .unwrap_err();
        assert!(is_not_found(&mail_err), "{mail_err}");

        for p in [&store, &calendar_in, &contact_in, &mail_in] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn vector_through_the_cli_persists() {
        let store = temp("vector", "loom");
        let va = temp("vector-a", "bin");
        let vb = temp("vector-b", "bin");
        let query = temp("vector-query", "bin");
        let got = temp("vector-get", "cbor");
        let ids = temp("vector-ids", "cbor");
        let index_keys = temp("vector-index-keys", "cbor");
        let hits = temp("vector-hits", "cbor");
        let hits_policy = temp("vector-hits-policy", "cbor");
        let hits_filtered = temp("vector-hits-filtered", "cbor");
        let source = temp("vector-source", "txt");
        let source_out = temp("vector-source-out", "txt");
        let meta_en = temp("vector-meta-en", "cbor");
        let meta_fr = temp("vector-meta-fr", "cbor");
        let filter_en = temp("vector-filter-en", "cbor");
        let f32s = |values: &[f32]| {
            values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>()
        };
        let metadata = |lang: &str| {
            loom_codec::encode(&loom_codec::Value::Map(vec![(
                loom_codec::Value::Text("lang".into()),
                loom_core::tabular::cell_value(&loom_core::tabular::Value::Text(lang.into())),
            )]))
            .unwrap()
        };
        let filter = loom_codec::encode(&loom_codec::Value::Array(vec![
            loom_codec::Value::Uint(1),
            loom_codec::Value::Text("lang".into()),
            loom_core::tabular::cell_value(&loom_core::tabular::Value::Text("en".into())),
        ]))
        .unwrap();
        std::fs::write(&va, f32s(&[1.0, 0.0])).unwrap();
        std::fs::write(&vb, f32s(&[0.0, 1.0])).unwrap();
        std::fs::write(&query, f32s(&[1.0, 0.0])).unwrap();
        std::fs::write(&source, b"alpha document").unwrap();
        std::fs::write(&meta_en, metadata("en")).unwrap();
        std::fs::write(&meta_fr, metadata("fr")).unwrap();
        std::fs::write(&filter_en, filter).unwrap();

        trun(Command::Vector {
            action: VectorCmd::Create {
                store: store.clone(),
                workspace: "vec".into(),
                name: "emb".into(),
                dim: 2,
                metric: "cosine".into(),
            },
        })
        .unwrap();
        for (id, vector, metadata) in [("a", &va, &meta_en), ("b", &vb, &meta_fr)] {
            trun(Command::Vector {
                action: VectorCmd::Upsert {
                    store: store.clone(),
                    workspace: "vec".into(),
                    name: "emb".into(),
                    id: id.into(),
                    vector: vector.clone(),
                    metadata: Some(metadata.clone()),
                },
            })
            .unwrap();
        }
        trun(Command::Vector {
            action: VectorCmd::UpsertSource {
                store: store.clone(),
                workspace: "vec".into(),
                name: "emb".into(),
                id: "a".into(),
                vector: va.clone(),
                source: source.clone(),
                metadata: Some(meta_en.clone()),
                model_id: Some("test-embed@1".into()),
                weights_digest: Some("sha256:test".into()),
            },
        })
        .unwrap();
        trun(Command::Vector {
            action: VectorCmd::Source {
                store: store.clone(),
                workspace: "vec".into(),
                name: "emb".into(),
                id: "a".into(),
                out: Some(source_out.clone()),
            },
        })
        .unwrap();
        assert_eq!(std::fs::read(&source_out).unwrap(), b"alpha document");
        trun(Command::Vector {
            action: VectorCmd::CreateIndex {
                store: store.clone(),
                workspace: "vec".into(),
                name: "emb".into(),
                key: "lang".into(),
            },
        })
        .unwrap();
        trun(Command::Vector {
            action: VectorCmd::IndexKeys {
                store: store.clone(),
                workspace: "vec".into(),
                name: "emb".into(),
                out: Some(index_keys.clone()),
            },
        })
        .unwrap();
        assert_eq!(
            loom_codec::decode(&std::fs::read(&index_keys).unwrap()).unwrap(),
            loom_codec::Value::Array(vec![loom_codec::Value::Text("lang".into())])
        );
        trun(Command::Vector {
            action: VectorCmd::Ids {
                store: store.clone(),
                workspace: "vec".into(),
                name: "emb".into(),
                prefix: Some("b".into()),
                out: Some(ids.clone()),
            },
        })
        .unwrap();
        assert_eq!(
            loom_codec::decode(&std::fs::read(&ids).unwrap()).unwrap(),
            loom_codec::Value::Array(vec![loom_codec::Value::Text("b".into())])
        );

        trun(Command::Vector {
            action: VectorCmd::Search {
                store: store.clone(),
                workspace: "vec".into(),
                name: "emb".into(),
                query: query.clone(),
                k: 2,
                filter: None,
                policy: "exact".into(),
                threshold: 4096,
                ef: 0,
                pq_m: 1,
                pq_k: 16,
                pq_iters: 8,
                out: Some(hits.clone()),
            },
        })
        .unwrap();
        let loom_codec::Value::Array(hit_items) =
            loom_codec::decode(&std::fs::read(&hits).unwrap()).unwrap()
        else {
            panic!("hits must be a CBOR array");
        };
        let loom_codec::Value::Array(first_hit) = &hit_items[0] else {
            panic!("hit must be a CBOR array");
        };
        assert_eq!(first_hit[0], loom_codec::Value::Text("a".into()));
        trun(Command::Vector {
            action: VectorCmd::Search {
                store: store.clone(),
                workspace: "vec".into(),
                name: "emb".into(),
                query: query.clone(),
                k: 2,
                filter: None,
                policy: "approximate-pq".into(),
                threshold: 0,
                ef: 0,
                pq_m: 1,
                pq_k: 16,
                pq_iters: 8,
                out: Some(hits_policy.clone()),
            },
        })
        .unwrap();
        let loom_codec::Value::Array(hit_items) =
            loom_codec::decode(&std::fs::read(&hits_policy).unwrap()).unwrap()
        else {
            panic!("policy hits must be a CBOR array");
        };
        let loom_codec::Value::Array(first_hit) = &hit_items[0] else {
            panic!("policy hit must be a CBOR array");
        };
        assert_eq!(first_hit[0], loom_codec::Value::Text("a".into()));
        trun(Command::Vector {
            action: VectorCmd::Search {
                store: store.clone(),
                workspace: "vec".into(),
                name: "emb".into(),
                query: query.clone(),
                k: 2,
                filter: Some(filter_en.clone()),
                policy: "exact".into(),
                threshold: 4096,
                ef: 0,
                pq_m: 1,
                pq_k: 16,
                pq_iters: 8,
                out: Some(hits_filtered.clone()),
            },
        })
        .unwrap();
        let loom_codec::Value::Array(hit_items) =
            loom_codec::decode(&std::fs::read(&hits_filtered).unwrap()).unwrap()
        else {
            panic!("filtered hits must be a CBOR array");
        };
        assert_eq!(hit_items.len(), 1);
        let loom_codec::Value::Array(first_hit) = &hit_items[0] else {
            panic!("filtered hit must be a CBOR array");
        };
        assert_eq!(first_hit[0], loom_codec::Value::Text("a".into()));

        trun(Command::Vector {
            action: VectorCmd::Get {
                store: store.clone(),
                workspace: "vec".into(),
                name: "emb".into(),
                id: "a".into(),
                out: Some(got.clone()),
            },
        })
        .unwrap();
        let loom_codec::Value::Array(payload) =
            loom_codec::decode(&std::fs::read(&got).unwrap()).unwrap()
        else {
            panic!("vector get must be a CBOR array");
        };
        assert_eq!(payload[0], loom_codec::Value::Bytes(f32s(&[1.0, 0.0])));
        trun(Command::Vector {
            action: VectorCmd::DropIndex {
                store: store.clone(),
                workspace: "vec".into(),
                name: "emb".into(),
                key: "lang".into(),
            },
        })
        .unwrap();
        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "vec").unwrap();
        assert!(
            loom_core::vector_metadata_index_keys(&loom, ns, "emb")
                .unwrap()
                .is_empty()
        );

        trun(Command::Vector {
            action: VectorCmd::Delete {
                store: store.clone(),
                workspace: "vec".into(),
                name: "emb".into(),
                id: "a".into(),
            },
        })
        .unwrap();
        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "vec").unwrap();
        assert!(
            loom_core::vector_get(&loom, ns, "emb", "a")
                .unwrap()
                .is_none()
        );

        for path in [
            &store,
            &va,
            &vb,
            &query,
            &got,
            &ids,
            &index_keys,
            &hits,
            &hits_policy,
            &hits_filtered,
            &source,
            &source_out,
            &meta_en,
            &meta_fr,
            &filter_en,
        ] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn graph_ledger_columnar_and_search_cli_project_facades() {
        let store = temp("facets", "loom");
        let graph_props = temp("graph-props", "cbor");
        let graph_node = temp("graph-node", "cbor");
        let graph_edge = temp("graph-edge", "cbor");
        let graph_neighbors = temp("graph-neighbors", "cbor");
        let graph_path = temp("graph-path", "cbor");
        let ledger_payload = temp("ledger-payload", "bin");
        let ledger_get = temp("ledger-get", "bin");
        let ledger_head = temp("ledger-head", "bin");
        let col_columns = temp("columnar-columns", "cbor");
        let col_row_a = temp("columnar-row-a", "cbor");
        let col_row_b = temp("columnar-row-b", "cbor");
        let col_select_columns = temp("columnar-select-columns", "cbor");
        let col_filter = temp("columnar-filter", "cbor");
        let col_scan = temp("columnar-scan", "cbor");
        let col_select = temp("columnar-select", "cbor");
        let col_arrow = temp("columnar-export", "arrow");
        let col_arrow_scan = temp("columnar-arrow-scan", "cbor");
        let col_parquet = temp("columnar-export", "parquet");
        let col_parquet_scan = temp("columnar-parquet-scan", "cbor");
        let search_mapping = temp("search-mapping", "cbor");
        let search_doc = temp("search-doc", "cbor");
        let search_id_file = temp("search-id", "bin");
        let search_get = temp("search-get", "cbor");
        let search_ids = temp("search-ids", "cbor");
        let search_query = temp("search-query", "cbor");
        let search_hits = temp("search-hits", "cbor");

        let graph_props_cbor = loom_codec::encode(&loom_codec::Value::Map(vec![(
            loom_codec::Value::Text("kind".into()),
            loom_codec::Value::Bytes(b"person".to_vec()),
        )]))
        .unwrap();
        std::fs::write(&graph_props, graph_props_cbor).unwrap();
        trun(Command::Graph {
            action: GraphCmd::UpsertNode {
                store: store.clone(),
                workspace: "graph".into(),
                name: "g".into(),
                id: "a".into(),
                props: Some(graph_props.clone()),
            },
        })
        .unwrap();
        trun(Command::Graph {
            action: GraphCmd::UpsertNode {
                store: store.clone(),
                workspace: "graph".into(),
                name: "g".into(),
                id: "b".into(),
                props: None,
            },
        })
        .unwrap();
        trun(Command::Graph {
            action: GraphCmd::UpsertEdge {
                store: store.clone(),
                workspace: "graph".into(),
                name: "g".into(),
                id: "e1".into(),
                src: "a".into(),
                dst: "b".into(),
                label: "knows".into(),
                props: None,
            },
        })
        .unwrap();
        trun(Command::Graph {
            action: GraphCmd::GetNode {
                store: store.clone(),
                workspace: "graph".into(),
                name: "g".into(),
                id: "a".into(),
                out: Some(graph_node.clone()),
            },
        })
        .unwrap();
        assert_eq!(
            loom_codec::decode(&std::fs::read(&graph_node).unwrap()).unwrap(),
            loom_codec::Value::Map(vec![(
                loom_codec::Value::Text("kind".into()),
                loom_codec::Value::Bytes(b"person".to_vec())
            )])
        );
        trun(Command::Graph {
            action: GraphCmd::GetEdge {
                store: store.clone(),
                workspace: "graph".into(),
                name: "g".into(),
                id: "e1".into(),
                out: Some(graph_edge.clone()),
            },
        })
        .unwrap();
        let loom_codec::Value::Array(edge) =
            loom_codec::decode(&std::fs::read(&graph_edge).unwrap()).unwrap()
        else {
            panic!("graph edge must be an array");
        };
        assert_eq!(edge[0], loom_codec::Value::Text("a".into()));
        assert_eq!(edge[1], loom_codec::Value::Text("b".into()));
        trun(Command::Graph {
            action: GraphCmd::Neighbors {
                store: store.clone(),
                workspace: "graph".into(),
                name: "g".into(),
                id: "a".into(),
                out: Some(graph_neighbors.clone()),
            },
        })
        .unwrap();
        assert_eq!(
            loom_codec::decode(&std::fs::read(&graph_neighbors).unwrap()).unwrap(),
            loom_codec::Value::Array(vec![loom_codec::Value::Text("b".into())])
        );
        trun(Command::Graph {
            action: GraphCmd::ShortestPath {
                store: store.clone(),
                workspace: "graph".into(),
                name: "g".into(),
                from: "a".into(),
                to: "b".into(),
                via_label: None,
                out: Some(graph_path.clone()),
            },
        })
        .unwrap();
        assert_eq!(
            loom_codec::decode(&std::fs::read(&graph_path).unwrap()).unwrap(),
            loom_codec::Value::Array(vec![
                loom_codec::Value::Text("a".into()),
                loom_codec::Value::Text("b".into())
            ])
        );

        std::fs::write(&ledger_payload, b"entry-0").unwrap();
        trun(Command::Ledger {
            action: LedgerCmd::Append {
                store: store.clone(),
                workspace: "ledger".into(),
                collection: "audit".into(),
                payload: ledger_payload.clone(),
            },
        })
        .unwrap();
        trun(Command::Ledger {
            action: LedgerCmd::Get {
                store: store.clone(),
                workspace: "ledger".into(),
                collection: "audit".into(),
                seq: 0,
                out: Some(ledger_get.clone()),
            },
        })
        .unwrap();
        assert_eq!(std::fs::read(&ledger_get).unwrap(), b"entry-0");
        trun(Command::Ledger {
            action: LedgerCmd::Head {
                store: store.clone(),
                workspace: "ledger".into(),
                collection: "audit".into(),
                out: Some(ledger_head.clone()),
            },
        })
        .unwrap();
        assert_eq!(std::fs::read(&ledger_head).unwrap().len(), 32);
        trun(Command::Ledger {
            action: LedgerCmd::Verify {
                store: store.clone(),
                workspace: "ledger".into(),
                collection: "audit".into(),
            },
        })
        .unwrap();

        let col_schema = loom_codec::Value::Array(vec![
            loom_codec::Value::Array(vec![
                loom_codec::Value::Text("id".into()),
                loom_codec::Value::Uint(u64::from(loom_core::tabular::ColumnType::Int.tag())),
            ]),
            loom_codec::Value::Array(vec![
                loom_codec::Value::Text("v".into()),
                loom_codec::Value::Uint(u64::from(loom_core::tabular::ColumnType::Text.tag())),
            ]),
        ]);
        std::fs::write(&col_columns, loom_codec::encode(&col_schema).unwrap()).unwrap();
        let row = |id: i64, text: &str| {
            loom_codec::encode(&loom_codec::Value::Array(vec![
                loom_core::tabular::cell_value(&loom_core::tabular::Value::Int(id)),
                loom_core::tabular::cell_value(&loom_core::tabular::Value::Text(text.into())),
            ]))
            .unwrap()
        };
        std::fs::write(&col_row_a, row(1, "a")).unwrap();
        std::fs::write(&col_row_b, row(2, "b")).unwrap();
        std::fs::write(
            &col_select_columns,
            loom_codec::encode(&loom_codec::Value::Array(vec![loom_codec::Value::Text(
                "v".into(),
            )]))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            &col_filter,
            loom_codec::encode(&loom_codec::Value::Array(vec![
                loom_codec::Value::Text("id".into()),
                loom_codec::Value::Uint(0),
                loom_core::tabular::cell_value(&loom_core::tabular::Value::Int(2)),
            ]))
            .unwrap(),
        )
        .unwrap();
        trun(Command::Columnar {
            action: ColumnarCmd::Create {
                store: store.clone(),
                workspace: "col".into(),
                name: "events".into(),
                columns: col_columns.clone(),
                target_segment_rows: 0,
            },
        })
        .unwrap();
        for row_path in [&col_row_a, &col_row_b] {
            trun(Command::Columnar {
                action: ColumnarCmd::Append {
                    store: store.clone(),
                    workspace: "col".into(),
                    name: "events".into(),
                    row: row_path.clone(),
                },
            })
            .unwrap();
        }
        trun(Command::Columnar {
            action: ColumnarCmd::Scan {
                store: store.clone(),
                workspace: "col".into(),
                name: "events".into(),
                out: Some(col_scan.clone()),
            },
        })
        .unwrap();
        let loom_codec::Value::Array(rows) =
            loom_codec::decode(&std::fs::read(&col_scan).unwrap()).unwrap()
        else {
            panic!("columnar scan must be an array");
        };
        assert_eq!(rows.len(), 2);
        trun(Command::Columnar {
            action: ColumnarCmd::Select {
                store: store.clone(),
                workspace: "col".into(),
                name: "events".into(),
                columns: col_select_columns.clone(),
                filter: Some(col_filter.clone()),
                out: Some(col_select.clone()),
            },
        })
        .unwrap();
        let loom_codec::Value::Array(rows) =
            loom_codec::decode(&std::fs::read(&col_select).unwrap()).unwrap()
        else {
            panic!("columnar select must be an array");
        };
        assert_eq!(rows.len(), 1);
        trun(Command::Columnar {
            action: ColumnarCmd::ExportArrow {
                store: store.clone(),
                workspace: "col".into(),
                name: "events".into(),
                out: Some(col_arrow.clone()),
            },
        })
        .unwrap();
        trun(Command::Columnar {
            action: ColumnarCmd::ImportArrow {
                store: store.clone(),
                workspace: "col".into(),
                name: "events_arrow".into(),
                input: col_arrow.clone(),
                target_segment_rows: 0,
                replace: false,
            },
        })
        .unwrap();
        trun(Command::Columnar {
            action: ColumnarCmd::Scan {
                store: store.clone(),
                workspace: "col".into(),
                name: "events_arrow".into(),
                out: Some(col_arrow_scan.clone()),
            },
        })
        .unwrap();
        assert_eq!(
            loom_codec::decode(&std::fs::read(&col_arrow_scan).unwrap()).unwrap(),
            loom_codec::decode(&std::fs::read(&col_scan).unwrap()).unwrap()
        );
        trun(Command::Columnar {
            action: ColumnarCmd::ExportParquet {
                store: store.clone(),
                workspace: "col".into(),
                name: "events".into(),
                out: Some(col_parquet.clone()),
            },
        })
        .unwrap();
        trun(Command::Columnar {
            action: ColumnarCmd::ImportParquet {
                store: store.clone(),
                workspace: "col".into(),
                name: "events_parquet".into(),
                input: col_parquet.clone(),
                target_segment_rows: 0,
                replace: false,
            },
        })
        .unwrap();
        trun(Command::Columnar {
            action: ColumnarCmd::Scan {
                store: store.clone(),
                workspace: "col".into(),
                name: "events_parquet".into(),
                out: Some(col_parquet_scan.clone()),
            },
        })
        .unwrap();
        assert_eq!(
            loom_codec::decode(&std::fs::read(&col_parquet_scan).unwrap()).unwrap(),
            loom_codec::decode(&std::fs::read(&col_scan).unwrap()).unwrap()
        );

        std::fs::write(
            &search_mapping,
            loom_codec::encode(&loom_codec::Value::Map(vec![
                (
                    loom_codec::Value::Text("title".into()),
                    loom_codec::Value::Array(vec![
                        loom_codec::Value::Uint(0),
                        loom_codec::Value::Bool(true),
                        loom_codec::Value::Bool(false),
                    ]),
                ),
                (
                    loom_codec::Value::Text("lang".into()),
                    loom_codec::Value::Array(vec![
                        loom_codec::Value::Uint(1),
                        loom_codec::Value::Bool(true),
                        loom_codec::Value::Bool(false),
                    ]),
                ),
            ]))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            &search_doc,
            loom_codec::encode(&loom_codec::Value::Map(vec![
                (
                    loom_codec::Value::Text("title".into()),
                    loom_codec::Value::Text("quick brown fox".into()),
                ),
                (
                    loom_codec::Value::Text("lang".into()),
                    loom_codec::Value::Bytes(b"en".to_vec()),
                ),
            ]))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            &search_query,
            loom_codec::encode(&loom_codec::Value::Array(vec![
                loom_codec::Value::Array(vec![
                    loom_codec::Value::Uint(0),
                    loom_codec::Value::Text("title".into()),
                    loom_codec::Value::Text("quick".into()),
                ]),
                loom_codec::Value::Uint(10),
                loom_codec::Value::Uint(0),
            ]))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(&search_id_file, b"d1").unwrap();
        trun(Command::Fts {
            action: SearchCmd::Create {
                store: store.clone(),
                workspace: "search".into(),
                name: "docs".into(),
                mapping: search_mapping.clone(),
            },
        })
        .unwrap();
        trun(Command::Fts {
            action: SearchCmd::Index {
                store: store.clone(),
                workspace: "search".into(),
                name: "docs".into(),
                id: None,
                id_file: Some(search_id_file.clone()),
                doc: Some(search_doc.clone()),
            },
        })
        .unwrap();
        trun(Command::Fts {
            action: SearchCmd::Get {
                store: store.clone(),
                workspace: "search".into(),
                name: "docs".into(),
                id: Some("d1".into()),
                id_file: None,
                out: Some(search_get.clone()),
            },
        })
        .unwrap();
        assert_eq!(
            loom_codec::decode(&std::fs::read(&search_get).unwrap()).unwrap(),
            loom_codec::decode(&std::fs::read(&search_doc).unwrap()).unwrap()
        );
        trun(Command::Fts {
            action: SearchCmd::Ids {
                store: store.clone(),
                workspace: "search".into(),
                name: "docs".into(),
                prefix: Some("d".into()),
                prefix_file: None,
                out: Some(search_ids.clone()),
            },
        })
        .unwrap();
        assert_eq!(
            loom_codec::decode(&std::fs::read(&search_ids).unwrap()).unwrap(),
            loom_codec::Value::Array(vec![loom_codec::Value::Bytes(b"d1".to_vec())])
        );
        trun(Command::Fts {
            action: SearchCmd::Query {
                store: store.clone(),
                workspace: "search".into(),
                name: "docs".into(),
                request: search_query.clone(),
                out: Some(search_hits.clone()),
            },
        })
        .unwrap();
        let loom_codec::Value::Array(response) =
            loom_codec::decode(&std::fs::read(&search_hits).unwrap()).unwrap()
        else {
            panic!("search response must be an array");
        };
        assert_eq!(response[0], loom_codec::Value::Bool(true));
        let loom_codec::Value::Array(hits) = &response[1] else {
            panic!("search hits must be an array");
        };
        let loom_codec::Value::Array(hit) = &hits[0] else {
            panic!("search hit must be an array");
        };
        assert_eq!(hit[0], loom_codec::Value::Bytes(b"d1".to_vec()));

        let loom = cli_open_loom_read(&store, &KeyOpts::default()).unwrap();
        let ledger_ns = resolve_ns(&loom, "ledger").unwrap();
        assert_eq!(loom_core::ledger_len(&loom, ledger_ns, "audit").unwrap(), 1);
        let search_ns = resolve_ns(&loom, "search").unwrap();
        assert!(
            loom_core::search_get(&loom, search_ns, "docs", b"d1")
                .unwrap()
                .is_some()
        );

        for path in [
            &store,
            &graph_props,
            &graph_node,
            &graph_edge,
            &graph_neighbors,
            &graph_path,
            &ledger_payload,
            &ledger_get,
            &ledger_head,
            &col_columns,
            &col_row_a,
            &col_row_b,
            &col_select_columns,
            &col_filter,
            &col_scan,
            &col_select,
            &col_arrow,
            &col_arrow_scan,
            &col_parquet,
            &col_parquet_scan,
            &search_mapping,
            &search_doc,
            &search_id_file,
            &search_get,
            &search_ids,
            &search_query,
            &search_hits,
        ] {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn table_blame_and_diff_through_the_cli() {
        let store = temp("tbl", "loom");
        let envelope = temp("tbl-diff", "cbor");
        let sql = |s: &str| sql_exec(store.clone(), "db", s);
        let commit = |m: &str| vcs_commit(store.clone(), "db", m);

        trun(ns_create(store.clone(), "db", Some("sql"))).unwrap();
        trun(sql("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")).unwrap();
        trun(sql("INSERT INTO t VALUES (1,'a'),(2,'b')")).unwrap();
        trun(commit("base")).unwrap();
        trun(sql("INSERT INTO t VALUES (3,'c')")).unwrap();
        trun(commit("add row")).unwrap();

        // Commits from the log (newest first): c2 = "add row", c1 = "base".
        let loom = cli_open_loom(&store, &KeyOpts::default()).unwrap();
        let ns = resolve_ns(&loom, "db").unwrap();
        let head = loom.registry().head_branch(ns).unwrap();
        let log = loom.log(ns, &head).unwrap();
        let (c2, c1) = (log[0], log[1]);

        // Blame shows the real projected columns (`__key`, id, v), each row tagged with the commit
        // that last set it: rows 1 and 2 from `base` (c1), row 3 from `add row` (c2), in key order.
        let blame = blame_output(&loom, ns, &head, ".loom/facets/sql/main/tables/t").unwrap();
        assert_eq!(blame.len(), 3);
        assert!(blame[0].starts_with(&c1.to_string()) && blame[0].ends_with("\t1\t1\ta"));
        assert!(blame[1].starts_with(&c1.to_string()) && blame[1].ends_with("\t2\t2\tb"));
        assert!(blame[2].starts_with(&c2.to_string()) && blame[2].ends_with("\t3\t3\tc"));

        // Diff base -> add: exactly one added row, rendered with typed cells.
        let diff = diff_output(&loom, ns, ".loom/facets/sql/main/tables/t", c1, c2).unwrap();
        assert_eq!(diff, vec!["+ 3\t3\tc".to_string()]);
        drop(loom);

        // The CLI dispatch path also runs end-to-end.
        trun(sql_table(TableCmd::Blame {
            store: store.clone(),
            workspace: "db".into(),
            table: ".loom/facets/sql/main/tables/t".into(),
        }))
        .unwrap();
        trun(vcs_diff_cbor(
            store.clone(),
            "db",
            c1.to_string(),
            c2.to_string(),
            Some(envelope.clone()),
        ))
        .unwrap();
        let bytes = std::fs::read(&envelope).unwrap();
        assert!(bytes.windows(b"LMDIFF".len()).any(|w| w == b"LMDIFF"));

        for p in [&store, &envelope] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn merge_through_the_cli() {
        let store = temp("merge", "loom");
        let base = temp("base", "txt");
        let feat = temp("feat", "txt");
        let main = temp("main", "txt");
        let out = temp("out", "txt");
        std::fs::write(&base, b"base").unwrap();
        std::fs::write(&feat, b"feat").unwrap();
        std::fs::write(&main, b"main").unwrap();

        let write =
            |path: &str, input: &str| files_write(store.clone(), "proj", path, input.into());
        let commit = |m: &str| vcs_commit(store.clone(), "proj", m);
        let checkout = |b: &str| vcs_checkout(store.clone(), "proj", b);

        trun(ns_create(store.clone(), "proj", Some("files"))).unwrap();
        trun(write("README.md", &base)).unwrap();
        trun(commit("base")).unwrap();
        trun(vcs_branch(store.clone(), "proj", "feature")).unwrap();
        // feature adds feature.txt; main adds main.txt - disjoint, so the merge is clean.
        trun(checkout("feature")).unwrap();
        trun(write("feature.txt", &feat)).unwrap();
        trun(commit("feat")).unwrap();
        trun(checkout("main")).unwrap();
        trun(write("main.txt", &main)).unwrap();
        trun(commit("main")).unwrap();
        trun(vcs_merge(store.clone(), "proj", "feature")).unwrap();

        // HEAD is now the merge commit, with both branches' files in the working tree.
        for (path, want) in [("feature.txt", &b"feat"[..]), ("main.txt", &b"main"[..])] {
            trun(files_read(store.clone(), "proj", path, Some(out.clone()))).unwrap();
            assert_eq!(std::fs::read(&out).unwrap(), want);
        }

        for p in [&store, &base, &feat, &main, &out] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn bundle_export_import_across_two_looms() {
        let src = temp("src", "loom");
        let dst = temp("dst", "loom");
        let bundle = temp("b", "bundle");
        let infile = temp("in", "txt");
        let outfile = temp("out", "txt");
        std::fs::write(&infile, b"hello").unwrap();

        // Build a files workspace with one commit in the source store.
        trun(ns_create(src.clone(), "proj", Some("files"))).unwrap();
        trun(files_write(src.clone(), "proj", "f.txt", infile.clone())).unwrap();
        trun(vcs_commit(src.clone(), "proj", "c")).unwrap();

        // Export to a .bundle, import into a fresh store, then check out and read it back.
        trun(Command::Store {
            action: StoreCmd::BundleExport {
                store: src.clone(),
                workspace: "proj".into(),
                out: bundle.clone(),
            },
        })
        .unwrap();
        trun(Command::Store {
            action: StoreCmd::BundleImport {
                store: dst.clone(),
                input: bundle.clone(),
            },
        })
        .unwrap();
        trun(vcs_checkout(dst.clone(), "proj", "main")).unwrap();
        trun(files_read(
            dst.clone(),
            "proj",
            "f.txt",
            Some(outfile.clone()),
        ))
        .unwrap();
        assert_eq!(std::fs::read(&outfile).unwrap(), b"hello");

        for p in [&src, &dst, &bundle, &infile, &outfile] {
            let _ = std::fs::remove_file(p);
        }
    }
}
