//! End-to-end HTTP carrier parity: a `RemoteLoomClient` whose transport bridges the client HTTP mapping
//! to the server `RemoteHttpService` over a live `RemoteRuntime`, proving discovery and unary calls round
//! trip through HTTP request/response semantics without a socket.
//!
//! Licensed under BUSL-1.1.

use loom_client::LocalLoomClient;
use loom_core::acl::{AclResource, AclResourceScope, AclRight, AclScopeKind, AclStore, AclSubject};
use loom_core::digest::{Algo, Digest};
use loom_core::identity::{IdentityStore, PrincipalKind};
use loom_core::{AclDomain, FacetKind, Loom, WorkspaceId};
use loom_hosted_core::remote::{
    RemoteAuth, RemoteAuthMode, RemoteRuntime, RemoteServerConfig, RemoteTlsTrust,
};
use loom_hosted_core::remote_http::RemoteHttpService;
use loom_locator::{ContextResolver, Layer};
use loom_remote_client::http::{
    call_request, discovery_request, parse_response, parse_stream_response,
};
use loom_remote_client::transport::FrameSource;
use loom_remote_client::{RemoteConnection, RemoteLoomClient, Transport};
use loom_remote_protocol::discovery::{DiscoveryMode, DiscoveryRoutes};
use loom_remote_protocol::generated_api::{
    Audit, Drive, Exec, Kv, Meetings, Queue, ServeConfig, Store, StoreAdmin, Tickets, Workspaces,
};
use loom_store::{FileStore, save_loom};
use loom_types::{Code, LoomError};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const CALL_PATH: &str = "/apps/loom/v1/call";

/// A transport that routes the client's HTTP request parts into the server HTTP service and maps the
/// response back, exercising the real carrier semantics without a network socket.
struct HttpBridge {
    service: Arc<RemoteHttpService>,
    call_path: String,
}

impl Transport for HttpBridge {
    fn discover(&self, path: &str) -> impl Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let parts = discovery_request(path);
        let response = self.service.handle(parts.method, &parts.path, &parts.body);
        let out = parse_response(response.status, response.body);
        async move { out }
    }

    fn call(&self, request: Vec<u8>) -> impl Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let parts = call_request(&self.call_path, request);
        let response = self.service.handle(parts.method, &parts.path, &parts.body);
        let out = parse_response(response.status, response.body);
        async move { out }
    }

    fn open_session(
        &self,
        request: Vec<u8>,
    ) -> impl Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let path = loom_remote_protocol::session::session_route(&self.call_path);
        let response = self.service.handle("POST", &path, &request);
        let out = parse_response(response.status, response.body);
        async move { out }
    }

    fn open_stream(
        &self,
        request: Vec<u8>,
    ) -> impl Future<Output = Result<FrameSource, LoomError>> + Send {
        let parts = call_request(&self.call_path, request);
        let response = self.service.handle(parts.method, &parts.path, &parts.body);
        // The in-process bridge keeps the buffered CBOR-array body (the incremental length-delimited path
        // is exercised by the real HTTP/2-over-TLS carrier); adapt the collected frames into a source.
        let out =
            parse_stream_response(response.status, response.body).map(FrameSource::from_frames);
        async move { out }
    }
}

fn block<F: Future>(f: F) -> F::Output {
    futures::executor::block_on(f)
}

fn temp_store() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "loom-remote-http-carrier-{}-{}.loom",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::remove_dir_all(&path).ok();
    LocalLoomClient::new(&path).create().expect("create store");
    path
}

fn temp_store_with_site_workspace(workspace: WorkspaceId) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "loom-remote-http-carrier-site-{}-{}.loom",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::remove_dir_all(&path).ok();
    let store = FileStore::create_with_profile(&path, Algo::Blake3).expect("create store");
    let mut loom = Loom::new(store);
    loom.registry_mut()
        .create(FacetKind::Files, Some("site"), workspace)
        .expect("seed fixed site workspace");
    save_loom(&mut loom).expect("save seeded site workspace");
    path
}

fn temp_store_with_files_workspace(workspace: WorkspaceId) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "loom-remote-http-carrier-drive-{}-{}.loom",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::remove_dir_all(&path).ok();
    let store = FileStore::create_with_profile(&path, Algo::Blake3).expect("create store");
    let mut loom = Loom::new(store);
    loom.registry_mut()
        .create(FacetKind::Files, Some("files"), workspace)
        .expect("seed fixed files workspace");
    save_loom(&mut loom).expect("save seeded files workspace");
    path
}

fn temp_authenticated_files_store(
    workspace: WorkspaceId,
    admin: WorkspaceId,
    user: WorkspaceId,
    tag: &str,
) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "loom-remote-http-carrier-drive-auth-{tag}-{}-{}.loom",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::remove_dir_all(&path).ok();
    let client = LocalLoomClient::new(&path);
    client.create().expect("create store");
    let session = client.open().expect("open seeded authenticated store");
    client
        .with_session(&session, |loom| {
            loom.registry_mut()
                .create(FacetKind::Files, Some("files"), workspace)?;
            let mut identity = IdentityStore::new(admin);
            identity.add_principal_with_handle(user, "user", "User", PrincipalKind::User)?;
            identity.set_passphrase(admin, "adminpw", b"admin-salt")?;
            identity.set_passphrase(user, "userpw", b"user-salt")?;
            loom.store().save_identity_store(&identity)?;
            loom.set_identity_store(identity);
            let mut acl = AclStore::new();
            acl.allow(AclSubject::Principal(admin), None, None, [AclRight::Admin])?;
            loom.store().save_acl_store(&acl)?;
            loom.set_acl_store(acl);
            save_loom(loom)
        })
        .expect("seed authenticated files workspace");
    assert!(client.close(&session));
    assert_eq!(client.session_count(), 0);
    path
}

fn temp_authenticated_two_files_store(
    workspace: WorkspaceId,
    other_workspace: WorkspaceId,
    admin: WorkspaceId,
    user: WorkspaceId,
    tag: &str,
) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "loom-remote-http-carrier-drive-two-{tag}-{}-{}.loom",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::remove_dir_all(&path).ok();
    let client = LocalLoomClient::new(&path);
    client.create().expect("create store");
    let session = client.open().expect("open seeded two-workspace store");
    client
        .with_session(&session, |loom| {
            loom.registry_mut()
                .create(FacetKind::Files, Some("files"), workspace)?;
            loom.registry_mut()
                .create(FacetKind::Files, Some("other-files"), other_workspace)?;
            let mut identity = IdentityStore::new(admin);
            identity.add_principal_with_handle(user, "user", "User", PrincipalKind::User)?;
            identity.set_passphrase(admin, "adminpw", b"admin-salt")?;
            identity.set_passphrase(user, "userpw", b"user-salt")?;
            loom.store().save_identity_store(&identity)?;
            loom.set_identity_store(identity);
            let mut acl = AclStore::new();
            acl.allow(AclSubject::Principal(admin), None, None, [AclRight::Admin])?;
            loom.store().save_acl_store(&acl)?;
            loom.set_acl_store(acl);
            save_loom(loom)
        })
        .expect("seed authenticated two-workspace store");
    assert!(client.close(&session));
    assert_eq!(client.session_count(), 0);
    path
}

fn temp_authenticated_admin_store(admin: WorkspaceId, user: WorkspaceId, tag: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "loom-remote-http-carrier-admin-{tag}-{}-{}.loom",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::remove_dir_all(&path).ok();
    let client = LocalLoomClient::new(&path);
    client.create().expect("create authenticated admin store");
    let session = client.open().expect("open authenticated admin store");
    client
        .with_session(&session, |loom| {
            let mut identity = IdentityStore::new(admin);
            identity.add_principal_with_handle(user, "user", "User", PrincipalKind::User)?;
            identity.set_passphrase(admin, "adminpw", b"admin-salt")?;
            identity.set_passphrase(user, "userpw", b"user-salt")?;
            loom.store().save_identity_store(&identity)?;
            loom.set_identity_store(identity);
            let mut acl = AclStore::new();
            acl.allow(AclSubject::Principal(admin), None, None, [AclRight::Admin])?;
            loom.store().save_acl_store(&acl)?;
            loom.set_acl_store(acl);
            save_loom(loom)
        })
        .expect("seed authenticated admin store");
    assert!(client.close(&session));
    assert_eq!(client.session_count(), 0);
    path
}

fn config() -> RemoteServerConfig {
    RemoteServerConfig {
        service_root: "https://remote.host/apps/loom".to_string(),
        call_endpoint: "https://remote.host/apps/loom/v1/call".to_string(),
        auth_modes: vec![RemoteAuthMode::Interactive],
        tls: vec![RemoteTlsTrust::System],
        discovery: DiscoveryRoutes {
            mode: DiscoveryMode::Default,
            service_root_path: "/apps/loom".to_string(),
            custom_path: None,
        },
        session_lease_ms: 60_000,
    }
}

fn seed_ticket(path: &PathBuf) -> (String, String, String) {
    let local = LocalLoomClient::new(path);
    let session = LocalLoomClient::open(&local).expect("open seed session");
    let workspace = local
        .workspace_create(&session, Some("repo"), Some(loom_core::FacetKind::Document))
        .expect("seed workspace");
    let workspace_id = workspace.to_string();
    let ticket = local
        .with_session(&session, |loom| {
            loom_tickets::create_project(
                loom,
                workspace,
                &workspace_id,
                "matrix",
                "MX",
                "Matrix",
                None,
            )?;
            let ticket = loom_tickets::create_ticket(
                loom,
                workspace,
                loom_tickets::TicketCreateRequest {
                    workspace_id: &workspace_id,
                    project_id: "matrix",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({"status": "open"}),
                    policy_labels: &[],
                    expected_root: None,
                },
            )?;
            save_loom(loom)?;
            Ok(ticket)
        })
        .expect("seed ticket");
    local.close(&session);
    (workspace_id, ticket.ticket_id, ticket.profile_root)
}

fn seed_ticket_pair(path: &PathBuf) -> (String, String, String, String) {
    let local = LocalLoomClient::new(path);
    let session = LocalLoomClient::open(&local).expect("open seed session");
    let workspace = local
        .workspace_create(&session, Some("repo"), Some(loom_core::FacetKind::Document))
        .expect("seed workspace");
    let workspace_id = workspace.to_string();
    let (source, target) = local
        .with_session(&session, |loom| {
            loom_tickets::create_project(
                loom,
                workspace,
                &workspace_id,
                "matrix",
                "MX",
                "Matrix",
                None,
            )?;
            let source = loom_tickets::create_ticket(
                loom,
                workspace,
                loom_tickets::TicketCreateRequest {
                    workspace_id: &workspace_id,
                    project_id: "matrix",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({"status": "planned", "priority": "P2"}),
                    policy_labels: &[],
                    expected_root: None,
                },
            )?;
            let target = loom_tickets::create_ticket(
                loom,
                workspace,
                loom_tickets::TicketCreateRequest {
                    workspace_id: &workspace_id,
                    project_id: "matrix",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({"status": "planned"}),
                    policy_labels: &[],
                    expected_root: Some(&source.profile_root),
                },
            )?;
            save_loom(loom)?;
            Ok((source, target))
        })
        .expect("seed tickets");
    local.close(&session);
    (
        workspace_id,
        source.ticket_id,
        target.ticket_id,
        target.profile_root,
    )
}

fn resolver() -> ContextResolver {
    ContextResolver::from_layers(&[Layer::new(
        "test",
        "[contexts.prod]\ntarget = \"https://remote.host/apps/loom\"\n",
    )])
    .unwrap()
}

fn exec_apply_request(workspace: &str, base: &str, fork: &str) -> Vec<u8> {
    loom_codec::encode(&loom_codec::Value::Map(vec![
        (
            loom_codec::Value::Text("workspace".to_string()),
            loom_codec::Value::Text(workspace.to_string()),
        ),
        (
            loom_codec::Value::Text("base".to_string()),
            loom_codec::Value::Text(base.to_string()),
        ),
        (
            loom_codec::Value::Text("fork".to_string()),
            loom_codec::Value::Text(fork.to_string()),
        ),
        (
            loom_codec::Value::Text("author".to_string()),
            loom_codec::Value::Text("alice".to_string()),
        ),
        (
            loom_codec::Value::Text("timestamp_ms".to_string()),
            loom_codec::Value::Uint(3_000),
        ),
    ]))
    .expect("encode exec apply request")
}

fn seed_exec_apply_fixture(client: &LocalLoomClient, session: &loom_client::types::LoomSession) {
    client
        .write_file(session, "repo", "base.txt", b"base", 0)
        .expect("write base");
    client.stage_all(session, "repo").expect("stage base");
    client
        .commit(session, "repo", "alice", "base", 1_000)
        .expect("commit base");
    client
        .branch(session, "repo", "feature")
        .expect("create feature branch");
    client
        .checkout(session, "repo", "feature")
        .expect("checkout feature");
    client
        .write_file(session, "repo", "feature.txt", b"feature", 0)
        .expect("write feature");
    client.stage_all(session, "repo").expect("stage feature");
    client
        .commit(session, "repo", "alice", "feature", 2_000)
        .expect("commit feature");
    client
        .checkout(session, "repo", "main")
        .expect("checkout main");
}

fn meetings_import_input(profile: &str, source_id: &str, title: &str) -> Vec<u8> {
    let source_digest =
        Digest::hash(Algo::Blake3, format!("{source_id}:{title}").as_bytes()).to_string();
    serde_json::to_vec(&serde_json::json!({
        "snapshot_version": 1,
        "profile": profile,
        "source_system": profile,
        "source_scope": "local-cache",
        "observed_at": 500,
        "coverage": "complete",
        "items": [{
            "source_entity_id": source_id,
            "source_digest": source_digest,
            "source_sidecar": {"id": source_id, "raw": true},
            "title": title,
            "summary_text": format!("{title} summary"),
            "transcript_spans": [{"text": format!("{title} transcript")}],
            "decisions": [{"label": format!("{title} decision")}]
        }]
    }))
    .expect("meetings import json")
}

fn drive_root_for_path(path: &PathBuf, workspace: WorkspaceId, drive_workspace_id: &str) -> String {
    let store = FileStore::open_read(path).expect("open drive fixture");
    let loom = Loom::new(store);
    loom_drive::list_folder(&loom, workspace, drive_workspace_id, "root")
        .expect("drive root")
        .profile_root
}

fn drive_folder_value_for_path(
    path: &PathBuf,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
    folder_id: &str,
) -> serde_json::Value {
    let store = FileStore::open_read(path).expect("open drive fixture");
    let loom = Loom::new(store);
    serde_json::to_value(
        loom_drive::list_folder(&loom, workspace, drive_workspace_id, folder_id)
            .expect("drive folder"),
    )
    .expect("drive folder json")
}

fn drive_conflicts_value_for_path(
    path: &PathBuf,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
) -> serde_json::Value {
    let store = FileStore::open_read(path).expect("open drive fixture");
    let loom = Loom::new(store);
    serde_json::to_value(
        loom_drive::list_conflicts(&loom, workspace, drive_workspace_id).expect("drive conflicts"),
    )
    .expect("drive conflicts json")
}

fn drive_file_bytes_from_client(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
    file_id: &str,
) -> Vec<u8> {
    client
        .with_session(session, |loom| {
            loom_drive::read_file(loom, workspace, drive_workspace_id, file_id)
        })
        .expect("drive file bytes")
}

fn drive_file_bytes_from_client_optional(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
    file_id: &str,
) -> Option<Vec<u8>> {
    match client.with_session(session, |loom| {
        loom_drive::read_file(loom, workspace, drive_workspace_id, file_id)
    }) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.code == Code::NotFound => None,
        Err(err) => panic!("drive file bytes failed: {err:?}"),
    }
}

fn drive_root_from_client(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
) -> String {
    client
        .with_session(session, |loom| {
            loom_drive::list_folder(loom, workspace, drive_workspace_id, "root")
        })
        .expect("drive root")
        .profile_root
}

fn drive_folder_value_from_client(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
    folder_id: &str,
) -> Option<serde_json::Value> {
    match client.with_session(session, |loom| {
        loom_drive::list_folder(loom, workspace, drive_workspace_id, folder_id)
    }) {
        Ok(folder) => Some(serde_json::to_value(folder).expect("drive folder json")),
        Err(err) if err.code == Code::NotFound => None,
        Err(err) => panic!("drive folder failed: {err:?}"),
    }
}

fn drive_conflicts_from_client(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
) -> serde_json::Value {
    client
        .with_session(session, |loom| {
            serde_json::to_value(loom_drive::list_conflicts(
                loom,
                workspace,
                drive_workspace_id,
            )?)
            .map_err(|err| LoomError::new(Code::Internal, err.to_string()))
        })
        .expect("drive conflicts")
}

fn drive_shares_from_client(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
) -> serde_json::Value {
    client
        .with_session(session, |loom| {
            serde_json::to_value(loom_drive::list_shares(
                loom,
                workspace,
                drive_workspace_id,
            )?)
            .map_err(|err| LoomError::new(Code::Internal, err.to_string()))
        })
        .expect("drive shares")
}

fn drive_retention_from_client(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
) -> serde_json::Value {
    client
        .with_session(session, |loom| {
            serde_json::to_value(loom_drive::list_retention(
                loom,
                workspace,
                drive_workspace_id,
            )?)
            .map_err(|err| LoomError::new(Code::Internal, err.to_string()))
        })
        .expect("drive retention")
}

fn drive_share_read_allowed_from_client(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
    target_kind: &str,
    target_id: &str,
    principal: WorkspaceId,
) -> bool {
    client
        .with_session(session, |loom| {
            let target = format!("{drive_workspace_id}/{target_kind}/{target_id}");
            let acl = loom.store().acl_store()?.unwrap_or_default();
            Ok(acl
                .authorize_resource_with_roles(
                    true,
                    principal,
                    [],
                    AclResource::scoped(
                        workspace,
                        AclDomain::Files,
                        None,
                        AclResourceScope::Prefix {
                            kind: AclScopeKind::Collection,
                            value: target.as_bytes(),
                        },
                    ),
                    AclRight::Read,
                )
                .is_ok())
        })
        .expect("drive share acl check")
}

fn audit_tuples_from_client(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
) -> Vec<(Option<String>, String, Option<String>)> {
    client
        .with_session(session, |loom| loom.store().audit_records())
        .expect("audit records")
        .into_iter()
        .map(|record| {
            (
                record.principal.map(|principal| principal.to_string()),
                record.action,
                record.target,
            )
        })
        .collect()
}

fn audit_records_from_client(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
) -> Vec<loom_store::AuditRecord> {
    client
        .with_session(session, |loom| loom.store().audit_records())
        .expect("audit records")
}

fn seed_audit_records_for_path(path: &PathBuf, actions: &[&str]) {
    let client = LocalLoomClient::new(path);
    let session = client.open().expect("open audit seed session");
    client
        .with_session(&session, |loom| {
            for action in actions {
                loom.store().audit_append(None, action, None)?;
            }
            save_loom(loom)
        })
        .expect("seed audit records");
    assert!(client.close(&session));
}

fn seed_audit_legal_hold_for_path(path: &PathBuf) {
    let client = LocalLoomClient::new(path);
    let session = client.open().expect("open audit legal hold seed session");
    client
        .with_session(&session, |loom| {
            loom.store().save_audit_config_audited(
                loom_store::AuditConfig {
                    retention_days: 365,
                    legal_hold: true,
                },
                None,
                "audit.config.set",
                Some("legal_hold=true"),
            )?;
            save_loom(loom)
        })
        .expect("seed audit legal hold");
    assert!(client.close(&session));
}

fn audit_compact_result(bytes: &[u8]) -> loom_wire::audit::AuditCompactResult {
    loom_wire::audit::audit_compact_result_from_cbor(bytes).expect("decode audit compact result")
}

fn maintenance_status_request(include_live_root_diagnostics: bool) -> Vec<u8> {
    loom_wire::store_admin::store_maintenance_status_request_to_cbor(
        &loom_wire::store_admin::StoreMaintenanceStatusRequest {
            include_live_root_diagnostics,
        },
    )
}

fn maintenance_status_result(bytes: &[u8]) -> loom_wire::store_admin::StoreMaintenanceStatusResult {
    loom_wire::store_admin::store_maintenance_status_result_from_cbor(bytes)
        .expect("decode maintenance status result")
}

fn scrub_volatile_maintenance_status(
    mut result: loom_wire::store_admin::StoreMaintenanceStatusResult,
) -> loom_wire::store_admin::StoreMaintenanceStatusResult {
    result.report.status.group_commit.group_commit_batches_total = 0;
    result
        .report
        .status
        .group_commit
        .group_commit_transactions_total = 0;
    result.report.status.group_commit.group_commit_records_total = 0;
    result.report.status.group_commit.fsync_total_micros = 0;
    result.report.status.group_commit.fsync_count = 0;
    result
        .report
        .status
        .group_commit
        .write_lock_wait_total_micros = 0;
    result.report.status.group_commit.write_lock_wait_count = 0;
    result
}

fn assert_maintenance_status_parity(
    remote: loom_wire::store_admin::StoreMaintenanceStatusResult,
    local: loom_wire::store_admin::StoreMaintenanceStatusResult,
) {
    assert_eq!(
        scrub_volatile_maintenance_status(remote),
        scrub_volatile_maintenance_status(local)
    );
}

fn empty_policy_update() -> loom_wire::store_admin::StoreMaintenancePolicyUpdate {
    loom_wire::store_admin::StoreMaintenancePolicyUpdate {
        min_candidate_pages: None,
        min_reusable_pages: None,
        interval_ms: None,
        backoff_ms: None,
        max_segments: None,
        max_pages: None,
        full_compaction_enabled: None,
        tail_trim_enabled: None,
        tail_compaction_enabled: None,
        tail_compaction_max_pages: None,
        tail_compaction_max_objects: None,
        tail_compaction_max_bytes: None,
        tail_compaction_interval_ms: None,
        tail_compaction_backoff_ms: None,
    }
}

fn maintenance_policy_update(
    update: &loom_wire::store_admin::StoreMaintenancePolicyUpdate,
) -> Vec<u8> {
    loom_wire::store_admin::store_maintenance_policy_update_to_cbor(update)
}

fn maintenance_run_request(max_segments: Option<u64>, max_pages: Option<u64>) -> Vec<u8> {
    loom_wire::store_admin::store_maintenance_run_request_to_cbor(
        &loom_wire::store_admin::StoreMaintenanceRunRequest {
            max_segments,
            max_pages,
        },
    )
}

fn maintenance_run_result(bytes: &[u8]) -> loom_wire::store_admin::StoreMaintenanceRunResult {
    loom_wire::store_admin::store_maintenance_run_result_from_cbor(bytes)
        .expect("decode maintenance run result")
}

fn scrub_volatile_maintenance_run(
    mut result: loom_wire::store_admin::StoreMaintenanceRunResult,
) -> loom_wire::store_admin::StoreMaintenanceRunResult {
    result.elapsed_ms = None;
    result.run_state.last_run_ms = None;
    result.run_state.next_eligible_ms = 0;
    result.report.run_state.last_run_ms = None;
    result.report.run_state.next_eligible_ms = 0;
    result.report.status.group_commit.group_commit_batches_total = 0;
    result
        .report
        .status
        .group_commit
        .group_commit_transactions_total = 0;
    result.report.status.group_commit.group_commit_records_total = 0;
    result.report.status.group_commit.fsync_total_micros = 0;
    result.report.status.group_commit.fsync_count = 0;
    result
        .report
        .status
        .group_commit
        .write_lock_wait_total_micros = 0;
    result.report.status.group_commit.write_lock_wait_count = 0;
    result
}

fn assert_maintenance_run_parity(
    remote: loom_wire::store_admin::StoreMaintenanceRunResult,
    local: loom_wire::store_admin::StoreMaintenanceRunResult,
) {
    assert_eq!(
        scrub_volatile_maintenance_run(remote),
        scrub_volatile_maintenance_run(local)
    );
}

fn malformed_cbor() -> Vec<u8> {
    vec![0xff]
}

fn unknown_session(
    session: &loom_remote_protocol::api_types::LoomSession,
) -> loom_remote_protocol::api_types::LoomSession {
    let mut handle = session.0.clone();
    handle.id = 999_999u64.to_be_bytes().to_vec();
    loom_remote_protocol::api_types::LoomSession(handle)
}

fn drive_write_root(output: &str) -> String {
    serde_json::from_str::<serde_json::Value>(output).expect("drive write json")["profile_root"]
        .as_str()
        .expect("profile root")
        .to_string()
}

fn drive_profile_root_from_output(output: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(output).ok()?;
    value
        .get("profile_root")
        .and_then(|root| root.as_str())
        .or_else(|| {
            value
                .get("operation")
                .and_then(|operation| operation.get("profile_root"))
                .and_then(|root| root.as_str())
        })
        .map(str::to_string)
}

fn latest_drive_profile_root(outputs: &[(&'static str, String)]) -> String {
    outputs
        .iter()
        .rev()
        .find_map(|(_, output)| drive_profile_root_from_output(output))
        .expect("latest drive profile root")
}

fn assert_error_parity(local: LoomError, remote: LoomError) {
    assert_eq!(remote.code, local.code);
    assert_eq!(remote.message, local.message);
    assert_eq!(remote.details, local.details);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DriveParityState {
    selected_root: String,
    selected_root_folder: Option<serde_json::Value>,
    selected_folder_a: Option<serde_json::Value>,
    selected_folder_b: Option<serde_json::Value>,
    selected_conflicts: serde_json::Value,
    selected_shares: serde_json::Value,
    selected_retention: serde_json::Value,
    selected_file_a: Option<Vec<u8>>,
    selected_share_acl: bool,
    audit: Vec<(Option<String>, String, Option<String>)>,
    unrelated_root: String,
    unrelated_root_folder: Option<serde_json::Value>,
    unrelated_conflicts: serde_json::Value,
    unrelated_shares: serde_json::Value,
    unrelated_retention: serde_json::Value,
}

fn drive_parity_state_from_client(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
    unrelated_workspace: WorkspaceId,
    unrelated_drive_workspace_id: &str,
    grantee: WorkspaceId,
) -> DriveParityState {
    DriveParityState {
        selected_root: drive_root_from_client(client, session, workspace, drive_workspace_id),
        selected_root_folder: drive_folder_value_from_client(
            client,
            session,
            workspace,
            drive_workspace_id,
            "root",
        ),
        selected_folder_a: drive_folder_value_from_client(
            client,
            session,
            workspace,
            drive_workspace_id,
            "folder-a",
        ),
        selected_folder_b: drive_folder_value_from_client(
            client,
            session,
            workspace,
            drive_workspace_id,
            "folder-b",
        ),
        selected_conflicts: drive_conflicts_from_client(
            client,
            session,
            workspace,
            drive_workspace_id,
        ),
        selected_shares: drive_shares_from_client(client, session, workspace, drive_workspace_id),
        selected_retention: drive_retention_from_client(
            client,
            session,
            workspace,
            drive_workspace_id,
        ),
        selected_file_a: drive_file_bytes_from_client_optional(
            client,
            session,
            workspace,
            drive_workspace_id,
            "file-a",
        ),
        selected_share_acl: drive_share_read_allowed_from_client(
            client,
            session,
            workspace,
            drive_workspace_id,
            "folder",
            "folder-a",
            grantee,
        ),
        audit: audit_tuples_from_client(client, session),
        unrelated_root: drive_root_from_client(
            client,
            session,
            unrelated_workspace,
            unrelated_drive_workspace_id,
        ),
        unrelated_root_folder: drive_folder_value_from_client(
            client,
            session,
            unrelated_workspace,
            unrelated_drive_workspace_id,
            "root",
        ),
        unrelated_conflicts: drive_conflicts_from_client(
            client,
            session,
            unrelated_workspace,
            unrelated_drive_workspace_id,
        ),
        unrelated_shares: drive_shares_from_client(
            client,
            session,
            unrelated_workspace,
            unrelated_drive_workspace_id,
        ),
        unrelated_retention: drive_retention_from_client(
            client,
            session,
            unrelated_workspace,
            unrelated_drive_workspace_id,
        ),
    }
}

fn open_authenticated_local(
    path: &PathBuf,
    principal: WorkspaceId,
    passphrase: &[u8],
) -> (
    LocalLoomClient,
    loom_remote_protocol::api_types::LoomSession,
) {
    let client = LocalLoomClient::new(path);
    let session = client.open().expect("open authenticated local");
    client
        .authenticate_passphrase(&session, principal, passphrase)
        .expect("authenticate local");
    (client, session)
}

#[derive(Clone, Copy)]
enum DriveParityGroup {
    HierarchyConflict,
    Upload,
    Sharing,
    Retention,
}

impl DriveParityGroup {
    fn name(self) -> &'static str {
        match self {
            DriveParityGroup::HierarchyConflict => "hierarchy/conflict",
            DriveParityGroup::Upload => "upload",
            DriveParityGroup::Sharing => "sharing",
            DriveParityGroup::Retention => "retention",
        }
    }

    fn slug(self) -> &'static str {
        match self {
            DriveParityGroup::HierarchyConflict => "hierarchy-conflict",
            DriveParityGroup::Upload => "upload",
            DriveParityGroup::Sharing => "sharing",
            DriveParityGroup::Retention => "retention",
        }
    }
}

fn remote_client_for_store(
    path: &PathBuf,
    connection_name: &str,
) -> (
    Arc<RemoteRuntime>,
    RemoteLoomClient<HttpBridge>,
    loom_remote_protocol::api_types::LoomSession,
) {
    let runtime = Arc::new(RemoteRuntime::start(path, config()).expect("start"));
    let connection = runtime.register_connection(connection_name);
    let session = runtime
        .open_session(connection, RemoteAuth::Unauthenticated)
        .expect("session");
    let service = Arc::new(RemoteHttpService::new(runtime.clone(), CALL_PATH));
    let transport = HttpBridge {
        service,
        call_path: CALL_PATH.to_string(),
    };
    let conn = block(RemoteConnection::connect(
        transport,
        "prod",
        &resolver(),
        DiscoveryMode::Default,
    ))
    .expect("connect over http carrier");
    let client = RemoteLoomClient::new(conn);
    client.bind_session(session.id.clone());
    let handle = block(client.open()).expect("open store session");
    (runtime, client, handle)
}

fn remote_client_for_store_with_auth(
    path: &PathBuf,
    connection_name: &str,
    principal: WorkspaceId,
    passphrase: &[u8],
) -> (
    Arc<RemoteRuntime>,
    RemoteLoomClient<HttpBridge>,
    loom_remote_protocol::api_types::LoomSession,
) {
    let runtime = Arc::new(RemoteRuntime::start(path, config()).expect("start"));
    let connection = runtime.register_connection(connection_name);
    let session = runtime
        .open_session(
            connection,
            RemoteAuth::Passphrase {
                principal,
                passphrase: passphrase.to_vec(),
            },
        )
        .expect("authenticated session");
    let service = Arc::new(RemoteHttpService::new(runtime.clone(), CALL_PATH));
    let transport = HttpBridge {
        service,
        call_path: CALL_PATH.to_string(),
    };
    let conn = block(RemoteConnection::connect(
        transport,
        "prod",
        &resolver(),
        DiscoveryMode::Default,
    ))
    .expect("connect over http carrier");
    let client = RemoteLoomClient::new(conn);
    client.bind_session(session.id.clone());
    let handle = block(client.open()).expect("open store session");
    (runtime, client, handle)
}

fn assert_failed_drive_result_parity(
    group: DriveParityGroup,
    local: Result<String, LoomError>,
    remote: Result<String, LoomError>,
) {
    match (local, remote) {
        (Err(local), Err(remote)) => assert_error_parity(local, remote),
        (Ok(local), Ok(remote)) => {
            assert_eq!(
                remote,
                local,
                "unexpected successful result for {}",
                group.name()
            );
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&remote)
                    .expect("remote successful json")
                    .as_object()
                    .map(|object| object.keys().cloned().collect::<Vec<_>>()),
                serde_json::from_str::<serde_json::Value>(&local)
                    .expect("local successful json")
                    .as_object()
                    .map(|object| object.keys().cloned().collect::<Vec<_>>()),
                "successful JSON shape mismatch for {}",
                group.name()
            );
            panic!(
                "{} representative failure unexpectedly succeeded",
                group.name()
            );
        }
        (local, remote) => panic!(
            "{} result kind mismatch: local={local:?} remote={remote:?}",
            group.name()
        ),
    }
}

fn seed_drive_failure_group(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
    group: DriveParityGroup,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
    grantee: WorkspaceId,
) {
    let root = drive_root_from_client(client, session, workspace, drive_workspace_id);
    match group {
        DriveParityGroup::HierarchyConflict => {
            let clock = loom_chat::set_test_now_ms(10);
            client
                .drive_create_folder_json(
                    session,
                    "files",
                    drive_workspace_id,
                    "root",
                    "folder-a",
                    "A",
                    &root,
                )
                .expect("seed hierarchy folder");
            drop(clock);
        }
        DriveParityGroup::Upload => {}
        DriveParityGroup::Sharing => {
            let clock = loom_chat::set_test_now_ms(20);
            let output = client
                .drive_create_folder_json(
                    session,
                    "files",
                    drive_workspace_id,
                    "root",
                    "folder-a",
                    "A",
                    &root,
                )
                .expect("seed share target");
            drop(clock);
            let clock = loom_chat::set_test_now_ms(21);
            client
                .drive_grant_share_json(
                    session,
                    "files",
                    drive_workspace_id,
                    "grant-duplicate",
                    "folder",
                    "folder-a",
                    &grantee.to_string(),
                    "viewer",
                    21,
                    None,
                )
                .expect("seed duplicate share");
            drop(clock);
            assert!(!drive_write_root(&output).is_empty());
        }
        DriveParityGroup::Retention => {
            let clock = loom_chat::set_test_now_ms(30);
            let output = client
                .drive_create_folder_json(
                    session,
                    "files",
                    drive_workspace_id,
                    "root",
                    "folder-a",
                    "A",
                    &root,
                )
                .expect("seed retention target");
            drop(clock);
            let root = drive_write_root(&output);
            let clock = loom_chat::set_test_now_ms(31);
            client
                .drive_pin_retention_json(
                    session,
                    "files",
                    drive_workspace_id,
                    "pin-duplicate",
                    "current_root",
                    &root,
                    Some("folder:folder-a"),
                    31,
                    None,
                )
                .expect("seed duplicate retention");
            drop(clock);
        }
    }
}

fn run_local_drive_failure_group(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
    group: DriveParityGroup,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
    grantee: WorkspaceId,
) -> Result<String, LoomError> {
    match group {
        DriveParityGroup::HierarchyConflict => client.drive_rename_json(
            session,
            "files",
            drive_workspace_id,
            "root",
            "folder-a",
            "A-stale",
            "stale-root",
        ),
        DriveParityGroup::Upload => client.drive_create_upload_json(
            session,
            "files",
            drive_workspace_id,
            "upload-fail",
            "root",
            "fail.bin",
            "file-fail",
            "stale-root",
            40,
            false,
        ),
        DriveParityGroup::Sharing => client.drive_grant_share_json(
            session,
            "files",
            drive_workspace_id,
            "grant-duplicate",
            "folder",
            "folder-a",
            &grantee.to_string(),
            "viewer",
            41,
            None,
        ),
        DriveParityGroup::Retention => client.drive_pin_retention_json(
            session,
            "files",
            drive_workspace_id,
            "pin-duplicate",
            "current_root",
            &drive_root_from_client(client, session, workspace, drive_workspace_id),
            Some("folder:folder-a"),
            42,
            None,
        ),
    }
}

fn run_remote_drive_failure_group(
    remote: &RemoteLoomClient<HttpBridge>,
    handle: &loom_remote_protocol::api_types::LoomSession,
    group: DriveParityGroup,
    drive_workspace_id: &str,
    grantee: WorkspaceId,
    retention_root: &str,
) -> Result<String, LoomError> {
    match group {
        DriveParityGroup::HierarchyConflict => block(Drive::drive_rename_json(
            remote,
            handle.clone(),
            "files".to_string(),
            drive_workspace_id.to_string(),
            "root".to_string(),
            "folder-a".to_string(),
            "A-stale".to_string(),
            "stale-root".to_string(),
        )),
        DriveParityGroup::Upload => block(Drive::drive_create_upload_json(
            remote,
            handle.clone(),
            "files".to_string(),
            drive_workspace_id.to_string(),
            "upload-fail".to_string(),
            "root".to_string(),
            "fail.bin".to_string(),
            "file-fail".to_string(),
            "stale-root".to_string(),
            40,
            false,
        )),
        DriveParityGroup::Sharing => block(Drive::drive_grant_share_json(
            remote,
            handle.clone(),
            "files".to_string(),
            drive_workspace_id.to_string(),
            "grant-duplicate".to_string(),
            "folder".to_string(),
            "folder-a".to_string(),
            grantee.to_string(),
            "viewer".to_string(),
            41,
            None,
        )),
        DriveParityGroup::Retention => block(Drive::drive_pin_retention_json(
            remote,
            handle.clone(),
            "files".to_string(),
            drive_workspace_id.to_string(),
            "pin-duplicate".to_string(),
            "current_root".to_string(),
            retention_root.to_string(),
            Some("folder:folder-a".to_string()),
            42,
            None,
        )),
    }
}

fn run_local_drive_success_sequence(
    client: &LocalLoomClient,
    session: &loom_remote_protocol::api_types::LoomSession,
    workspace: WorkspaceId,
    drive_workspace_id: &str,
    grantee: WorkspaceId,
) -> Vec<(&'static str, String)> {
    let mut outputs = Vec::new();
    let mut root = drive_root_from_client(client, session, workspace, drive_workspace_id);
    let clock = loom_chat::set_test_now_ms(100);
    let output = client
        .drive_create_folder_json(
            session,
            "files",
            drive_workspace_id,
            "root",
            "folder-a",
            "A",
            &root,
        )
        .expect("local create folder a");
    drop(clock);
    root = drive_write_root(&output);
    outputs.push(("create-folder-a", output));

    let clock = loom_chat::set_test_now_ms(110);
    let output = client
        .drive_create_folder_json(
            session,
            "files",
            drive_workspace_id,
            "root",
            "folder-b",
            "B",
            &root,
        )
        .expect("local create folder b");
    drop(clock);
    let stale_for_delete = root;
    root = drive_write_root(&output);
    outputs.push(("create-folder-b", output));

    let clock = loom_chat::set_test_now_ms(120);
    let output = client
        .drive_rename_json(
            session,
            "files",
            drive_workspace_id,
            "root",
            "folder-a",
            "A-renamed",
            &root,
        )
        .expect("local rename");
    drop(clock);
    root = drive_write_root(&output);
    outputs.push(("rename", output));

    let clock = loom_chat::set_test_now_ms(130);
    let output = client
        .drive_move_json(
            session,
            "files",
            drive_workspace_id,
            "root",
            "folder-b",
            "folder-a",
            &root,
        )
        .expect("local move");
    drop(clock);
    outputs.push(("move", output));

    let clock = loom_chat::set_test_now_ms(140);
    let output = client
        .drive_delete_json(
            session,
            "files",
            drive_workspace_id,
            "folder-b",
            "folder-a",
            &stale_for_delete,
        )
        .expect("local stale delete");
    drop(clock);
    let conflict_id =
        serde_json::from_str::<serde_json::Value>(&output).expect("delete json")["conflict_id"]
            .as_str()
            .expect("conflict id")
            .to_string();
    outputs.push(("delete-held-conflict", output));

    let clock = loom_chat::set_test_now_ms(150);
    let output = client
        .drive_resolve_conflict_json(
            session,
            "files",
            drive_workspace_id,
            &conflict_id,
            "keep_current",
        )
        .expect("local resolve conflict");
    drop(clock);
    root = drive_write_root(&output);
    outputs.push(("resolve-conflict", output));

    let output = client
        .drive_create_upload_json(
            session,
            "files",
            drive_workspace_id,
            "upload-a",
            "root",
            "raw.bin",
            "file-a",
            &root,
            160,
            false,
        )
        .expect("local create upload");
    outputs.push(("create-upload", output));

    let raw_chunk = vec![0, 159, 146, 150, 255, b'L', b'O', b'O', b'M'];
    let output = client
        .drive_upload_chunk_json(session, "files", drive_workspace_id, "upload-a", &raw_chunk)
        .expect("local upload chunk");
    outputs.push(("upload-chunk", output));

    let clock = loom_chat::set_test_now_ms(170);
    let output = client
        .drive_commit_upload_json(session, "files", drive_workspace_id, "upload-a")
        .expect("local commit upload");
    drop(clock);
    outputs.push(("commit-upload", output));

    let clock = loom_chat::set_test_now_ms(180);
    let output = client
        .drive_grant_share_json(
            session,
            "files",
            drive_workspace_id,
            "grant-live",
            "file",
            "file-a",
            &grantee.to_string(),
            "viewer",
            180,
            None,
        )
        .expect("local grant");
    drop(clock);
    outputs.push(("grant-share", output));

    let clock = loom_chat::set_test_now_ms(190);
    let output = client
        .drive_revoke_share_json(session, "files", drive_workspace_id, "grant-live")
        .expect("local revoke");
    drop(clock);
    outputs.push(("revoke-share", output));

    let clock = loom_chat::set_test_now_ms(200);
    let output = client
        .drive_grant_share_json(
            session,
            "files",
            drive_workspace_id,
            "grant-expiring",
            "file",
            "file-a",
            &grantee.to_string(),
            "viewer",
            200,
            Some(205),
        )
        .expect("local expiring grant");
    drop(clock);
    outputs.push(("grant-share-expiring", output));

    let clock = loom_chat::set_test_now_ms(210);
    let output = client
        .drive_apply_share_expiry_json(session, "files", drive_workspace_id, 205)
        .expect("local share expiry");
    drop(clock);
    outputs.push(("apply-share-expiry", output));

    root = drive_root_from_client(client, session, workspace, drive_workspace_id);
    let clock = loom_chat::set_test_now_ms(220);
    let output = client
        .drive_pin_retention_json(
            session,
            "files",
            drive_workspace_id,
            "pin-live",
            "current_root",
            &root,
            Some("file:file-a"),
            220,
            None,
        )
        .expect("local pin");
    drop(clock);
    outputs.push(("pin-retention", output));

    let clock = loom_chat::set_test_now_ms(230);
    let output = client
        .drive_unpin_retention_json(session, "files", drive_workspace_id, "pin-live")
        .expect("local unpin");
    drop(clock);
    outputs.push(("unpin-retention", output));

    root = drive_root_from_client(client, session, workspace, drive_workspace_id);
    let clock = loom_chat::set_test_now_ms(240);
    let output = client
        .drive_pin_retention_json(
            session,
            "files",
            drive_workspace_id,
            "pin-expiring",
            "current_root",
            &root,
            Some("file:file-a"),
            240,
            Some(245),
        )
        .expect("local expiring pin");
    drop(clock);
    outputs.push(("pin-retention-expiring", output));

    let clock = loom_chat::set_test_now_ms(250);
    let output = client
        .drive_apply_retention_json(session, "files", drive_workspace_id, 245)
        .expect("local apply retention");
    drop(clock);
    outputs.push(("apply-retention", output));
    outputs
}

fn run_remote_drive_success_sequence(
    remote: &RemoteLoomClient<HttpBridge>,
    handle: &loom_remote_protocol::api_types::LoomSession,
    initial_root: String,
    drive_workspace_id: &str,
    grantee: WorkspaceId,
) -> Vec<(&'static str, String)> {
    let mut outputs = Vec::new();
    let mut root = initial_root;
    let clock = loom_chat::set_test_now_ms(100);
    let output = block(Drive::drive_create_folder_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "root".to_string(),
        "folder-a".to_string(),
        "A".to_string(),
        root,
    ))
    .expect("remote create folder a");
    drop(clock);
    root = drive_write_root(&output);
    outputs.push(("create-folder-a", output));

    let clock = loom_chat::set_test_now_ms(110);
    let output = block(Drive::drive_create_folder_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "root".to_string(),
        "folder-b".to_string(),
        "B".to_string(),
        root.clone(),
    ))
    .expect("remote create folder b");
    drop(clock);
    let stale_for_delete = root;
    root = drive_write_root(&output);
    outputs.push(("create-folder-b", output));

    let clock = loom_chat::set_test_now_ms(120);
    let output = block(Drive::drive_rename_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "root".to_string(),
        "folder-a".to_string(),
        "A-renamed".to_string(),
        root,
    ))
    .expect("remote rename");
    drop(clock);
    root = drive_write_root(&output);
    outputs.push(("rename", output));

    let clock = loom_chat::set_test_now_ms(130);
    let output = block(Drive::drive_move_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "root".to_string(),
        "folder-b".to_string(),
        "folder-a".to_string(),
        root,
    ))
    .expect("remote move");
    drop(clock);
    outputs.push(("move", output));

    let clock = loom_chat::set_test_now_ms(140);
    let output = block(Drive::drive_delete_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "folder-b".to_string(),
        "folder-a".to_string(),
        stale_for_delete,
    ))
    .expect("remote stale delete");
    drop(clock);
    let conflict_id =
        serde_json::from_str::<serde_json::Value>(&output).expect("delete json")["conflict_id"]
            .as_str()
            .expect("conflict id")
            .to_string();
    outputs.push(("delete-held-conflict", output));

    let clock = loom_chat::set_test_now_ms(150);
    let output = block(Drive::drive_resolve_conflict_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        conflict_id,
        "keep_current".to_string(),
    ))
    .expect("remote resolve");
    drop(clock);
    root = drive_write_root(&output);
    outputs.push(("resolve-conflict", output));

    let output = block(Drive::drive_create_upload_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "upload-a".to_string(),
        "root".to_string(),
        "raw.bin".to_string(),
        "file-a".to_string(),
        root,
        160,
        false,
    ))
    .expect("remote create upload");
    outputs.push(("create-upload", output));

    let raw_chunk = vec![0, 159, 146, 150, 255, b'L', b'O', b'O', b'M'];
    let output = block(Drive::drive_upload_chunk_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "upload-a".to_string(),
        raw_chunk,
    ))
    .expect("remote upload chunk");
    outputs.push(("upload-chunk", output));

    let clock = loom_chat::set_test_now_ms(170);
    let output = block(Drive::drive_commit_upload_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "upload-a".to_string(),
    ))
    .expect("remote commit upload");
    drop(clock);
    outputs.push(("commit-upload", output));

    let clock = loom_chat::set_test_now_ms(180);
    let output = block(Drive::drive_grant_share_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "grant-live".to_string(),
        "file".to_string(),
        "file-a".to_string(),
        grantee.to_string(),
        "viewer".to_string(),
        180,
        None,
    ))
    .expect("remote grant");
    drop(clock);
    outputs.push(("grant-share", output));

    let clock = loom_chat::set_test_now_ms(190);
    let output = block(Drive::drive_revoke_share_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "grant-live".to_string(),
    ))
    .expect("remote revoke");
    drop(clock);
    outputs.push(("revoke-share", output));

    let clock = loom_chat::set_test_now_ms(200);
    let output = block(Drive::drive_grant_share_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "grant-expiring".to_string(),
        "file".to_string(),
        "file-a".to_string(),
        grantee.to_string(),
        "viewer".to_string(),
        200,
        Some(205),
    ))
    .expect("remote expiring grant");
    drop(clock);
    outputs.push(("grant-share-expiring", output));

    let clock = loom_chat::set_test_now_ms(210);
    let output = block(Drive::drive_apply_share_expiry_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        205,
    ))
    .expect("remote share expiry");
    drop(clock);
    outputs.push(("apply-share-expiry", output));

    let root = latest_drive_profile_root(&outputs);
    let clock = loom_chat::set_test_now_ms(220);
    let output = block(Drive::drive_pin_retention_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "pin-live".to_string(),
        "current_root".to_string(),
        root,
        Some("file:file-a".to_string()),
        220,
        None,
    ))
    .expect("remote pin");
    drop(clock);
    outputs.push(("pin-retention", output));

    let clock = loom_chat::set_test_now_ms(230);
    let output = block(Drive::drive_unpin_retention_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "pin-live".to_string(),
    ))
    .expect("remote unpin");
    drop(clock);
    outputs.push(("unpin-retention", output));

    let root = latest_drive_profile_root(&outputs);
    let clock = loom_chat::set_test_now_ms(240);
    let output = block(Drive::drive_pin_retention_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        "pin-expiring".to_string(),
        "current_root".to_string(),
        root,
        Some("file:file-a".to_string()),
        240,
        Some(245),
    ))
    .expect("remote expiring pin");
    drop(clock);
    outputs.push(("pin-retention-expiring", output));

    let clock = loom_chat::set_test_now_ms(250);
    let output = block(Drive::drive_apply_retention_json(
        remote,
        handle.clone(),
        "files".to_string(),
        drive_workspace_id.to_string(),
        245,
    ))
    .expect("remote apply retention");
    drop(clock);
    outputs.push(("apply-retention", output));
    outputs
}

fn serve_audit_actions(path: &PathBuf) -> Vec<String> {
    FileStore::open_read(path)
        .expect("open store for audit")
        .audit_records()
        .expect("audit records")
        .into_iter()
        .map(|record| record.action)
        .collect()
}

#[test]
fn remote_client_round_trips_over_http_carrier_semantics() {
    let path = temp_store();
    let runtime = Arc::new(RemoteRuntime::start(&path, config()).expect("start"));
    let connection = runtime.register_connection("http-client");
    let session = runtime
        .open_session(connection, RemoteAuth::Unauthenticated)
        .expect("session");
    let service = Arc::new(RemoteHttpService::new(runtime.clone(), CALL_PATH));
    let transport = HttpBridge {
        service,
        call_path: CALL_PATH.to_string(),
    };

    let resolver = ContextResolver::from_layers(&[Layer::new(
        "test",
        "[contexts.prod]\ntarget = \"https://remote.host/apps/loom\"\n",
    )])
    .unwrap();
    let conn = block(RemoteConnection::connect(
        transport,
        "prod",
        &resolver,
        DiscoveryMode::Default,
    ))
    .expect("connect over http carrier");
    let client = RemoteLoomClient::new(conn);
    client.bind_session(session.id.clone());

    // Discovery negotiated version 1 through the GET route.
    assert_eq!(client.connection().version(), 1);

    // store_version parity with the engine, via the generated `Store::version` stub over the HTTP call
    // route.
    assert_eq!(
        block(client.version()).expect("version"),
        LocalLoomClient::new(&path).store_version()
    );

    // The generated stubs send the IDL `LoomSession handle` as arg 0; the generated server dispatch
    // decodes-and-discards it and substitutes its resolved engine session. `Store::open` resolves to the
    // runtime-owned session and yields the handle the other stubs thread through.
    let handle = block(client.open()).expect("open store session");

    // KV round trip through the generated `Kv` stubs (end-to-end generated-client-to-generated-server).
    let key = loom_core::kv::key_to_cbor(&loom_core::tabular::Value::Text("k".to_string()));
    block(Kv::put(
        &client,
        handle.clone(),
        "app".to_string(),
        "c".to_string(),
        key.clone(),
        b"v".to_vec(),
    ))
    .expect("kv put");
    assert_eq!(
        block(Kv::get(
            &client,
            handle.clone(),
            "app".to_string(),
            "c".to_string(),
            key,
        ))
        .expect("kv get"),
        Some(b"v".to_vec())
    );

    // Queue append (unary) then range (unary list), both through the generated `Queue` stubs.
    for entry in [b"a".as_slice(), b"b".as_slice()] {
        block(Queue::append(
            &client,
            handle.clone(),
            "jobs".to_string(),
            "in".to_string(),
            entry.to_vec(),
        ))
        .expect("queue append");
    }
    assert_eq!(
        block(Queue::range(
            &client,
            handle.clone(),
            "jobs".to_string(),
            "in".to_string(),
            0,
            2,
        ))
        .expect("queue range"),
        vec![b"a".to_vec(), b"b".to_vec()]
    );

    // Workspace create + list through the generated `Workspaces` stubs.
    block(client.workspace_create(handle.clone(), Some("proj".to_string()), None))
        .expect("workspace create");
    assert!(
        !block(client.workspace_list(handle))
            .expect("workspace list")
            .is_empty()
    );

    // Note: SQL-family end-to-end through the generated client stubs is exercised in the HTTP/2-over-TLS
    // carrier test, whose server runs on its own thread. The SQL engine drives its executor with
    // `block_on`, which cannot nest inside this in-process single-thread bridge that runs the server
    // dispatch synchronously on the client's `block_on`. SQL through the generated *server* dispatch is
    // covered directly by the `remote` unit tests.

    runtime.shutdown();
    std::fs::remove_dir_all(&path).ok();
}

#[test]
fn remote_serve_config_round_trips_over_http_carrier() {
    let site_workspace = WorkspaceId::from_bytes([91; 16]);
    let local_path = temp_store_with_site_workspace(site_workspace);
    let local = LocalLoomClient::new(&local_path);
    let local_session = LocalLoomClient::open(&local).expect("open seed session");
    let local_configured = block(ServeConfig::serve_listener_configure_json(
        &local,
        local_session.clone(),
        serde_json::json!({
            "surface": "web",
            "selectors": ["site"],
            "bind": "127.0.0.1:19100",
            "transport": "rest",
            "enabled": true
        })
        .to_string(),
    ))
    .expect("local configure");
    let local_configured: serde_json::Value =
        serde_json::from_str(&local_configured).expect("local configured json");
    let local_listener = local_configured["id"]
        .as_str()
        .expect("local listener id")
        .to_string();
    let local_listed = block(ServeConfig::serve_listener_list_json(
        &local,
        local_session.clone(),
    ))
    .expect("local listener list");
    let local_disabled = block(ServeConfig::serve_listener_set_enabled_json(
        &local,
        local_session.clone(),
        local_listener.clone(),
        false,
    ))
    .expect("local disable");
    let local_enabled = block(ServeConfig::serve_listener_set_enabled_json(
        &local,
        local_session.clone(),
        local_listener.clone(),
        true,
    ))
    .expect("local enable");
    let local_routed = block(ServeConfig::serve_web_route_set_json(
        &local,
        local_session.clone(),
        serde_json::json!({
            "listener": local_listener,
            "route": "docs",
            "prefix": "docs",
            "workspace": "site",
            "root": "/docs"
        })
        .to_string(),
    ))
    .expect("local route set");
    let local_route_listed = block(ServeConfig::serve_web_route_list_json(
        &local,
        local_session.clone(),
        local_listener.clone(),
    ))
    .expect("local route list");
    let local_route_removed = block(ServeConfig::serve_web_route_remove_json(
        &local,
        local_session.clone(),
        local_listener.clone(),
        "docs".to_string(),
    ))
    .expect("local route remove");
    let local_listener_removed = block(ServeConfig::serve_listener_remove_json(
        &local,
        local_session.clone(),
        local_listener.clone(),
    ))
    .expect("local listener remove");
    assert!(local.close(&local_session));

    let path = temp_store_with_site_workspace(site_workspace);

    let (runtime, client, handle) = remote_client_for_store(&path, "http-serve-config-client");
    let configured = block(ServeConfig::serve_listener_configure_json(
        &client,
        handle.clone(),
        serde_json::json!({
            "surface": "web",
            "selectors": ["site"],
            "bind": "127.0.0.1:19100",
            "transport": "rest",
            "enabled": true
        })
        .to_string(),
    ))
    .expect("remote configure");
    let configured: serde_json::Value = serde_json::from_str(&configured).expect("configured json");
    assert_eq!(configured, local_configured);
    let listener = configured["id"].as_str().expect("listener id").to_string();
    let listener_listed = block(ServeConfig::serve_listener_list_json(
        &client,
        handle.clone(),
    ))
    .expect("remote listener list");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&listener_listed).expect("listener list json"),
        serde_json::from_str::<serde_json::Value>(&local_listed).expect("local listener list json")
    );
    let disabled = block(ServeConfig::serve_listener_set_enabled_json(
        &client,
        handle.clone(),
        listener.clone(),
        false,
    ))
    .expect("remote disable");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&disabled).expect("disable json"),
        serde_json::from_str::<serde_json::Value>(&local_disabled).expect("local disable json")
    );
    let enabled = block(ServeConfig::serve_listener_set_enabled_json(
        &client,
        handle.clone(),
        listener.clone(),
        true,
    ))
    .expect("remote enable");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&enabled).expect("enable json"),
        serde_json::from_str::<serde_json::Value>(&local_enabled).expect("local enable json")
    );
    let routed = block(ServeConfig::serve_web_route_set_json(
        &client,
        handle.clone(),
        serde_json::json!({
            "listener": listener,
            "route": "docs",
            "prefix": "docs",
            "workspace": "site",
            "root": "/docs"
        })
        .to_string(),
    ))
    .expect("remote route set");
    let routed: serde_json::Value = serde_json::from_str(&routed).expect("route json");
    assert_eq!(
        routed,
        serde_json::from_str::<serde_json::Value>(&local_routed).expect("local route json")
    );
    assert_eq!(routed["routes"][0]["route_id"], "docs");
    assert_eq!(routed["routes"][0]["path_prefix"], "/docs");
    let listed = block(ServeConfig::serve_web_route_list_json(
        &client,
        handle.clone(),
        listener.clone(),
    ))
    .expect("remote route list");
    let listed: serde_json::Value = serde_json::from_str(&listed).expect("list json");
    assert_eq!(
        listed,
        serde_json::from_str::<serde_json::Value>(&local_route_listed)
            .expect("local route list json")
    );
    assert_eq!(listed["routes"][0]["route_id"], "docs");
    let baseline_audit = serve_audit_actions(&path);
    let failed = block(ServeConfig::serve_web_route_set_json(
        &client,
        handle.clone(),
        serde_json::json!({
            "listener": listener,
            "route": "missing",
            "prefix": "/missing",
            "workspace": "missing",
            "root": "/missing"
        })
        .to_string(),
    ))
    .expect_err("missing workspace");
    assert_eq!(failed.code, Code::NotFound);
    assert_eq!(serve_audit_actions(&path), baseline_audit);
    let route_removed = block(ServeConfig::serve_web_route_remove_json(
        &client,
        handle.clone(),
        listener.clone(),
        "docs".to_string(),
    ))
    .expect("remote route remove");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&route_removed).expect("route remove json"),
        serde_json::from_str::<serde_json::Value>(&local_route_removed)
            .expect("local route remove json")
    );
    let listener_removed = block(ServeConfig::serve_listener_remove_json(
        &client,
        handle.clone(),
        listener.clone(),
    ))
    .expect("remote listener remove");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&listener_removed).expect("listener remove json"),
        serde_json::from_str::<serde_json::Value>(&local_listener_removed)
            .expect("local listener remove json")
    );
    assert_eq!(
        serve_audit_actions(&path),
        vec![
            "serve.listener.configure".to_string(),
            "serve.listener.list".to_string(),
            "serve.listener.disable".to_string(),
            "serve.listener.enable".to_string(),
            "serve.web.route.set".to_string(),
            "serve.web.route.list".to_string(),
            "serve.web.route.remove".to_string(),
            "serve.listener.remove".to_string(),
        ]
    );
    block(Store::close(&client, handle)).expect("remote close");
    runtime.shutdown();

    let reopened = LocalLoomClient::new(&path);
    let reopened_session = LocalLoomClient::open(&reopened).expect("reopen");
    let reopened_listeners = block(ServeConfig::serve_listener_list_json(
        &reopened,
        reopened_session.clone(),
    ))
    .expect("reopened listener list");
    let reopened_listeners: serde_json::Value =
        serde_json::from_str(&reopened_listeners).expect("reopened listeners json");
    assert!(
        reopened_listeners["listeners"]
            .as_array()
            .expect("listeners")
            .is_empty()
    );
    let reopened_local = LocalLoomClient::new(&local_path);
    let reopened_local_session = LocalLoomClient::open(&reopened_local).expect("reopen local");
    let reopened_local_listeners = block(ServeConfig::serve_listener_list_json(
        &reopened_local,
        reopened_local_session.clone(),
    ))
    .expect("reopened local listener list");
    assert_eq!(
        reopened_listeners,
        serde_json::from_str::<serde_json::Value>(&reopened_local_listeners)
            .expect("reopened local listeners json")
    );
    assert!(reopened_local.close(&reopened_local_session));
    assert!(reopened.close(&reopened_session));
    std::fs::remove_dir_all(&local_path).ok();
    std::fs::remove_dir_all(&path).ok();
}

#[test]
fn mu_6h_i_d_remote_exec_apply_matches_local_and_preserves_stable_errors() {
    let local_path = temp_store();
    let local = LocalLoomClient::new(&local_path);
    let local_session = LocalLoomClient::open(&local).expect("open local session");
    seed_exec_apply_fixture(&local, &local_session);

    let remote_path = temp_store();
    let remote_seed = LocalLoomClient::new(&remote_path);
    let remote_seed_session =
        LocalLoomClient::open(&remote_seed).expect("open remote seed session");
    seed_exec_apply_fixture(&remote_seed, &remote_seed_session);
    remote_seed.close(&remote_seed_session);

    let request = exec_apply_request("repo", "main", "feature");
    let local_output = local
        .apply_cbor(&local_session, &request)
        .expect("local apply");
    let (runtime, remote, remote_handle) =
        remote_client_for_store(&remote_path, "http-exec-apply-client");
    let remote_output =
        block(Exec::apply_cbor(&remote, remote_handle.clone(), request)).expect("remote apply");
    assert_eq!(remote_output, local_output);

    let err = block(Exec::apply_cbor(
        &remote,
        remote_handle.clone(),
        b"not cbor".to_vec(),
    ))
    .expect_err("malformed exec apply");
    assert_eq!(err.code, Code::InvalidArgument);

    runtime.shutdown();
    local.close(&local_session);
    std::fs::remove_dir_all(local_path).ok();
    std::fs::remove_dir_all(remote_path).ok();
}

#[test]
fn mu_6h_i_d_remote_meetings_import_matches_local_and_preserves_stable_errors() {
    let local_path = temp_store();
    let local = LocalLoomClient::new(&local_path);
    let local_session = LocalLoomClient::open(&local).expect("open local session");

    let remote_path = temp_store();
    let input = meetings_import_input("granola-app", "note-1", "Planning");
    let local_output = local
        .meetings_import_snapshot(&local_session, "studio", "granola-app", &input, false)
        .expect("local meetings import");
    let (runtime, remote, remote_handle) =
        remote_client_for_store(&remote_path, "http-meetings-import-client");
    let remote_output = block(Meetings::meetings_import_snapshot(
        &remote,
        remote_handle.clone(),
        "studio".to_string(),
        "granola-app".to_string(),
        input,
        false,
    ))
    .expect("remote meetings import");
    assert_eq!(remote_output, local_output);

    let err = block(Meetings::meetings_import_snapshot(
        &remote,
        remote_handle.clone(),
        "studio".to_string(),
        "granola-app".to_string(),
        b"not json".to_vec(),
        false,
    ))
    .expect_err("malformed meetings import");
    assert_eq!(err.code, Code::InvalidArgument);

    runtime.shutdown();
    local.close(&local_session);
    std::fs::remove_dir_all(local_path).ok();
    std::fs::remove_dir_all(remote_path).ok();
}

#[test]
fn mu_6i_d5_remote_audit_and_store_admin_success_parity_and_durability() {
    let admin = WorkspaceId::from_bytes([41; 16]);
    let user = WorkspaceId::from_bytes([42; 16]);
    let local_path = temp_authenticated_admin_store(admin, user, "success-local");
    let remote_path = temp_authenticated_admin_store(admin, user, "success-remote");
    seed_audit_records_for_path(&local_path, &["seed.one", "seed.two"]);
    seed_audit_records_for_path(&remote_path, &["seed.one", "seed.two"]);

    let (local, local_session) = open_authenticated_local(&local_path, admin, b"adminpw");
    let (runtime, remote, remote_handle) =
        remote_client_for_store_with_auth(&remote_path, "http-admin-success", admin, b"adminpw");

    for include_live_root_diagnostics in [false, true] {
        let request = maintenance_status_request(include_live_root_diagnostics);
        let local_status = maintenance_status_result(
            &block(StoreAdmin::store_maintenance_status(
                &local,
                local_session.clone(),
                request.clone(),
            ))
            .expect("local maintenance status"),
        );
        let remote_status = maintenance_status_result(
            &block(StoreAdmin::store_maintenance_status(
                &remote,
                remote_handle.clone(),
                request,
            ))
            .expect("remote maintenance status"),
        );
        assert_maintenance_status_parity(remote_status.clone(), local_status);
        assert_eq!(
            remote_status.live_root_diagnostics.is_some(),
            include_live_root_diagnostics
        );
    }

    let before_policy = local
        .with_session(&local_session, |loom| {
            loom.store().store_maintenance_policy()
        })
        .expect("local policy before");
    let mut policy = empty_policy_update();
    policy.max_pages = Some(77);
    policy.tail_trim_enabled = Some(false);
    let update = maintenance_policy_update(&policy);
    let local_policy = maintenance_status_result(
        &block(StoreAdmin::store_maintenance_policy_set(
            &local,
            local_session.clone(),
            update.clone(),
        ))
        .expect("local policy update"),
    );
    let remote_policy = maintenance_status_result(
        &block(StoreAdmin::store_maintenance_policy_set(
            &remote,
            remote_handle.clone(),
            update,
        ))
        .expect("remote policy update"),
    );
    assert_maintenance_status_parity(remote_policy, local_policy);

    let request = maintenance_run_request(Some(1), Some(1));
    let local_run = maintenance_run_result(
        &block(StoreAdmin::store_maintenance_run(
            &local,
            local_session.clone(),
            request.clone(),
        ))
        .expect("local maintenance run"),
    );
    let remote_run = maintenance_run_result(
        &block(StoreAdmin::store_maintenance_run(
            &remote,
            remote_handle.clone(),
            request,
        ))
        .expect("remote maintenance run"),
    );
    assert_maintenance_run_parity(remote_run, local_run);

    let local_compact = audit_compact_result(
        &block(Audit::audit_compact(&local, local_session.clone(), 2))
            .expect("local audit compact"),
    );
    let remote_compact = audit_compact_result(
        &block(Audit::audit_compact(&remote, remote_handle.clone(), 2))
            .expect("remote audit compact"),
    );
    assert_eq!(remote_compact, local_compact);
    assert_eq!(remote_compact.checkpoint_seq, Some(2));
    let checkpoint_seq = remote_compact
        .checkpoint_seq
        .expect("audit compact checkpoint sequence");
    let checkpoint_hash = remote_compact
        .checkpoint_hash
        .expect("audit compact checkpoint hash");
    let expected_retained_audit_records = remote_compact
        .audit_seq
        .checked_sub(checkpoint_seq)
        .expect("audit compact sequence should retain records after checkpoint");

    block(Store::close(&remote, remote_handle)).expect("remote close");
    runtime.shutdown();
    assert!(local.close(&local_session));

    let (reopened_local, reopened_local_session) =
        open_authenticated_local(&local_path, admin, b"adminpw");
    let (reopened_remote, reopened_remote_session) =
        open_authenticated_local(&remote_path, admin, b"adminpw");
    let reopened_local_policy = reopened_local
        .with_session(&reopened_local_session, |loom| {
            loom.store().store_maintenance_policy()
        })
        .expect("reopened local policy");
    let reopened_remote_policy = reopened_remote
        .with_session(&reopened_remote_session, |loom| {
            loom.store().store_maintenance_policy()
        })
        .expect("reopened remote policy");
    assert_eq!(reopened_remote_policy, reopened_local_policy);
    assert_eq!(reopened_remote_policy.max_pages, 77);
    assert!(!reopened_remote_policy.tail_trim_enabled);
    assert_eq!(
        reopened_remote_policy.min_candidate_pages,
        before_policy.min_candidate_pages
    );
    let remote_audit = audit_tuples_from_client(&reopened_remote, &reopened_remote_session);
    assert!(
        remote_audit
            .iter()
            .any(|(_, action, _)| action == "audit.prune")
    );
    let reopened_local_audit_records =
        audit_records_from_client(&reopened_local, &reopened_local_session);
    let reopened_remote_audit_records =
        audit_records_from_client(&reopened_remote, &reopened_remote_session);
    assert_eq!(reopened_remote_audit_records, reopened_local_audit_records);
    assert_eq!(
        reopened_remote_audit_records.len() as u64,
        expected_retained_audit_records
    );
    let first_retained = reopened_remote_audit_records
        .first()
        .expect("retained audit record after checkpoint");
    assert_eq!(first_retained.seq, checkpoint_seq + 1);
    assert_eq!(first_retained.prev_hash, Some(checkpoint_hash));
    let last_retained = reopened_remote_audit_records
        .last()
        .expect("last retained audit record");
    assert_eq!(last_retained.seq, remote_compact.audit_seq);
    assert_eq!(last_retained.action, "audit.prune");

    assert!(reopened_local.close(&reopened_local_session));
    assert!(reopened_remote.close(&reopened_remote_session));
    std::fs::remove_dir_all(local_path).ok();
    std::fs::remove_dir_all(remote_path).ok();
}

#[test]
fn mu_6i_d5_remote_audit_and_store_admin_authorization_parity() {
    let admin = WorkspaceId::from_bytes([43; 16]);
    let user = WorkspaceId::from_bytes([44; 16]);
    let local_path = temp_authenticated_admin_store(admin, user, "auth-local");
    let remote_path = temp_authenticated_admin_store(admin, user, "auth-remote");
    let (local, local_session) = open_authenticated_local(&local_path, user, b"userpw");
    let (runtime, remote, remote_handle) =
        remote_client_for_store_with_auth(&remote_path, "http-admin-denied", user, b"userpw");

    let status_request = maintenance_status_request(false);
    let local_status = maintenance_status_result(
        &block(StoreAdmin::store_maintenance_status(
            &local,
            local_session.clone(),
            status_request.clone(),
        ))
        .expect("local status does not require admin"),
    );
    let remote_status = maintenance_status_result(
        &block(StoreAdmin::store_maintenance_status(
            &remote,
            remote_handle.clone(),
            status_request,
        ))
        .expect("remote status does not require admin"),
    );
    assert_maintenance_status_parity(remote_status, local_status);

    assert_error_parity(
        block(Audit::audit_compact(&local, local_session.clone(), 0))
            .expect_err("local audit compact denied"),
        block(Audit::audit_compact(&remote, remote_handle.clone(), 0))
            .expect_err("remote audit compact denied"),
    );
    assert_error_parity(
        block(StoreAdmin::store_maintenance_policy_set(
            &local,
            local_session.clone(),
            maintenance_policy_update(&empty_policy_update()),
        ))
        .expect_err("local policy denied"),
        block(StoreAdmin::store_maintenance_policy_set(
            &remote,
            remote_handle.clone(),
            maintenance_policy_update(&empty_policy_update()),
        ))
        .expect_err("remote policy denied"),
    );
    assert_error_parity(
        block(StoreAdmin::store_maintenance_run(
            &local,
            local_session.clone(),
            maintenance_run_request(Some(1), Some(1)),
        ))
        .expect_err("local run denied"),
        block(StoreAdmin::store_maintenance_run(
            &remote,
            remote_handle.clone(),
            maintenance_run_request(Some(1), Some(1)),
        ))
        .expect_err("remote run denied"),
    );
    assert_error_parity(
        block(StoreAdmin::store_maintenance_policy_set(
            &local,
            local_session.clone(),
            malformed_cbor(),
        ))
        .expect_err("local malformed policy denied before decode"),
        block(StoreAdmin::store_maintenance_policy_set(
            &remote,
            remote_handle.clone(),
            malformed_cbor(),
        ))
        .expect_err("remote malformed policy denied before decode"),
    );
    assert_error_parity(
        block(StoreAdmin::store_maintenance_run(
            &local,
            local_session.clone(),
            malformed_cbor(),
        ))
        .expect_err("local malformed run denied before decode"),
        block(StoreAdmin::store_maintenance_run(
            &remote,
            remote_handle.clone(),
            malformed_cbor(),
        ))
        .expect_err("remote malformed run denied before decode"),
    );

    block(Store::close(&remote, remote_handle)).expect("remote close");
    runtime.shutdown();
    assert!(local.close(&local_session));
    std::fs::remove_dir_all(local_path).ok();
    std::fs::remove_dir_all(remote_path).ok();
}

#[test]
fn mu_6i_d5_remote_audit_and_store_admin_stable_error_parity() {
    let admin = WorkspaceId::from_bytes([45; 16]);
    let user = WorkspaceId::from_bytes([46; 16]);
    let local_path = temp_authenticated_admin_store(admin, user, "errors-local");
    let remote_path = temp_authenticated_admin_store(admin, user, "errors-remote");
    let (local, local_session) = open_authenticated_local(&local_path, admin, b"adminpw");
    let (runtime, remote, remote_handle) =
        remote_client_for_store_with_auth(&remote_path, "http-admin-errors", admin, b"adminpw");

    assert_error_parity(
        block(StoreAdmin::store_maintenance_run(
            &local,
            local_session.clone(),
            maintenance_run_request(Some(0), None),
        ))
        .expect_err("local zero maintenance limit"),
        block(StoreAdmin::store_maintenance_run(
            &remote,
            remote_handle.clone(),
            maintenance_run_request(Some(0), None),
        ))
        .expect_err("remote zero maintenance limit"),
    );
    assert_error_parity(
        block(StoreAdmin::store_maintenance_status(
            &local,
            local_session.clone(),
            malformed_cbor(),
        ))
        .expect_err("local malformed status"),
        block(StoreAdmin::store_maintenance_status(
            &remote,
            remote_handle.clone(),
            malformed_cbor(),
        ))
        .expect_err("remote malformed status"),
    );
    assert_error_parity(
        block(StoreAdmin::store_maintenance_policy_set(
            &local,
            local_session.clone(),
            malformed_cbor(),
        ))
        .expect_err("local malformed policy"),
        block(StoreAdmin::store_maintenance_policy_set(
            &remote,
            remote_handle.clone(),
            malformed_cbor(),
        ))
        .expect_err("remote malformed policy"),
    );
    assert_error_parity(
        block(StoreAdmin::store_maintenance_run(
            &local,
            local_session.clone(),
            malformed_cbor(),
        ))
        .expect_err("local malformed run"),
        block(StoreAdmin::store_maintenance_run(
            &remote,
            remote_handle.clone(),
            malformed_cbor(),
        ))
        .expect_err("remote malformed run"),
    );

    block(Store::close(&remote, remote_handle.clone())).expect("remote close");
    assert_error_parity(
        block(StoreAdmin::store_maintenance_status(
            &local,
            unknown_session(&local_session),
            maintenance_status_request(false),
        ))
        .expect_err("local unknown session"),
        block(StoreAdmin::store_maintenance_status(
            &remote,
            remote_handle,
            maintenance_status_request(false),
        ))
        .expect_err("remote unknown session"),
    );
    runtime.shutdown();
    assert!(local.close(&local_session));

    let hold_local_path = temp_authenticated_admin_store(admin, user, "legal-hold-local");
    let hold_remote_path = temp_authenticated_admin_store(admin, user, "legal-hold-remote");
    seed_audit_legal_hold_for_path(&hold_local_path);
    seed_audit_legal_hold_for_path(&hold_remote_path);
    let (hold_local, hold_local_session) =
        open_authenticated_local(&hold_local_path, admin, b"adminpw");
    let (hold_runtime, hold_remote, hold_remote_handle) = remote_client_for_store_with_auth(
        &hold_remote_path,
        "http-admin-legal-hold",
        admin,
        b"adminpw",
    );
    assert_error_parity(
        block(Audit::audit_compact(
            &hold_local,
            hold_local_session.clone(),
            0,
        ))
        .expect_err("local legal hold compact"),
        block(Audit::audit_compact(
            &hold_remote,
            hold_remote_handle.clone(),
            0,
        ))
        .expect_err("remote legal hold compact"),
    );
    block(Store::close(&hold_remote, hold_remote_handle)).expect("remote legal hold close");
    hold_runtime.shutdown();
    assert!(hold_local.close(&hold_local_session));

    std::fs::remove_dir_all(local_path).ok();
    std::fs::remove_dir_all(remote_path).ok();
    std::fs::remove_dir_all(hold_local_path).ok();
    std::fs::remove_dir_all(hold_remote_path).ok();
}

#[test]
fn mu_6h_j_f_remote_drive_create_folder_routes_over_http_carrier() {
    let workspace = WorkspaceId::from_bytes([77; 16]);
    let drive_workspace_id = workspace.to_string();

    let local_path = temp_store_with_files_workspace(workspace);
    let local = LocalLoomClient::new(&local_path);
    let local_session = LocalLoomClient::open(&local).expect("open local session");
    let local_root = drive_root_for_path(&local_path, workspace, &drive_workspace_id);
    let local_output = local
        .drive_create_folder_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "root",
            "folder-a",
            "A",
            &local_root,
        )
        .expect("local drive create folder");

    let remote_path = temp_store_with_files_workspace(workspace);
    let remote_root = drive_root_for_path(&remote_path, workspace, &drive_workspace_id);
    let (runtime, remote, remote_handle) =
        remote_client_for_store(&remote_path, "http-drive-create-folder-client");
    let remote_output = block(Drive::drive_create_folder_json(
        &remote,
        remote_handle,
        "files".to_string(),
        drive_workspace_id,
        "root".to_string(),
        "folder-a".to_string(),
        "A".to_string(),
        remote_root,
    ))
    .expect("remote drive create folder");
    assert_eq!(remote_output, local_output);

    runtime.shutdown();
    local.close(&local_session);
    std::fs::remove_dir_all(local_path).ok();
    std::fs::remove_dir_all(remote_path).ok();
}

#[test]
fn mu_6h_j_g_a_remote_drive_hierarchy_conflict_and_upload_match_direct_local() {
    let workspace = WorkspaceId::from_bytes([78; 16]);
    let drive_workspace_id = workspace.to_string();

    let local_path = temp_store_with_files_workspace(workspace);
    let local = LocalLoomClient::new(&local_path);
    let local_session = LocalLoomClient::open(&local).expect("open local session");
    let remote_path = temp_store_with_files_workspace(workspace);
    let (runtime, remote, remote_handle) =
        remote_client_for_store(&remote_path, "http-drive-parity-client");

    let local_initial_root = drive_root_for_path(&local_path, workspace, &drive_workspace_id);
    let remote_initial_root = drive_root_for_path(&remote_path, workspace, &drive_workspace_id);
    assert_eq!(remote_initial_root, local_initial_root);

    let local_create_a = local
        .drive_create_folder_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "root",
            "folder-a",
            "A",
            &local_initial_root,
        )
        .expect("local create folder a");
    let remote_create_a = block(Drive::drive_create_folder_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "root".to_string(),
        "folder-a".to_string(),
        "A".to_string(),
        remote_initial_root,
    ))
    .expect("remote create folder a");
    assert_eq!(remote_create_a, local_create_a);
    let root_after_a = drive_write_root(&local_create_a);

    let local_create_b = local
        .drive_create_folder_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "root",
            "folder-b",
            "B",
            &root_after_a,
        )
        .expect("local create folder b");
    let remote_create_b = block(Drive::drive_create_folder_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "root".to_string(),
        "folder-b".to_string(),
        "B".to_string(),
        root_after_a,
    ))
    .expect("remote create folder b");
    assert_eq!(remote_create_b, local_create_b);
    let root_after_b = drive_write_root(&local_create_b);

    let local_stale_rename = local
        .drive_rename_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "root",
            "folder-a",
            "A2",
            &local_initial_root,
        )
        .expect_err("local stale rename conflicts");
    let remote_stale_rename = block(Drive::drive_rename_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "root".to_string(),
        "folder-a".to_string(),
        "A2".to_string(),
        local_initial_root,
    ))
    .expect_err("remote stale rename conflicts");
    assert_error_parity(local_stale_rename, remote_stale_rename);

    let local_rename = local
        .drive_rename_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "root",
            "folder-a",
            "A2",
            &root_after_b,
        )
        .expect("local rename folder");
    let remote_rename = block(Drive::drive_rename_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "root".to_string(),
        "folder-a".to_string(),
        "A2".to_string(),
        root_after_b.clone(),
    ))
    .expect("remote rename folder");
    assert_eq!(remote_rename, local_rename);
    let root_after_rename = drive_write_root(&local_rename);

    let local_move = local
        .drive_move_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "root",
            "folder-b",
            "folder-a",
            &root_after_rename,
        )
        .expect("local move folder");
    let remote_move = block(Drive::drive_move_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "root".to_string(),
        "folder-b".to_string(),
        "folder-a".to_string(),
        root_after_rename,
    ))
    .expect("remote move folder");
    assert_eq!(remote_move, local_move);
    assert_eq!(
        drive_folder_value_for_path(&remote_path, workspace, &drive_workspace_id, "root"),
        drive_folder_value_for_path(&local_path, workspace, &drive_workspace_id, "root")
    );
    assert_eq!(
        drive_folder_value_for_path(&remote_path, workspace, &drive_workspace_id, "folder-b"),
        drive_folder_value_for_path(&local_path, workspace, &drive_workspace_id, "folder-b")
    );

    let clock = loom_chat::set_test_now_ms(800);
    let local_delete = local
        .drive_delete_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "folder-b",
            "folder-a",
            &root_after_b,
        )
        .expect("local stale delete creates held conflict");
    let remote_delete = block(Drive::drive_delete_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "folder-b".to_string(),
        "folder-a".to_string(),
        root_after_b,
    ))
    .expect("remote stale delete creates held conflict");
    drop(clock);
    assert_eq!(remote_delete, local_delete);
    let conflict_id = serde_json::from_str::<serde_json::Value>(&local_delete)
        .expect("delete json")["conflict_id"]
        .as_str()
        .expect("delete conflict id")
        .to_string();
    assert_eq!(
        drive_conflicts_value_for_path(&remote_path, workspace, &drive_workspace_id),
        drive_conflicts_value_for_path(&local_path, workspace, &drive_workspace_id)
    );

    let local_resolve = local
        .drive_resolve_conflict_json(
            &local_session,
            "files",
            &drive_workspace_id,
            &conflict_id,
            "keep_current",
        )
        .expect("local resolve conflict");
    let remote_resolve = block(Drive::drive_resolve_conflict_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        conflict_id,
        "keep_current".to_string(),
    ))
    .expect("remote resolve conflict");
    assert_eq!(remote_resolve, local_resolve);
    assert_eq!(
        drive_conflicts_value_for_path(&remote_path, workspace, &drive_workspace_id),
        drive_conflicts_value_for_path(&local_path, workspace, &drive_workspace_id)
    );
    let upload_root = drive_write_root(&local_resolve);

    let local_upload = local
        .drive_create_upload_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "upload-a",
            "root",
            "raw.bin",
            "file-a",
            &upload_root,
            700,
            false,
        )
        .expect("local create upload");
    let remote_upload = block(Drive::drive_create_upload_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "upload-a".to_string(),
        "root".to_string(),
        "raw.bin".to_string(),
        "file-a".to_string(),
        upload_root,
        700,
        false,
    ))
    .expect("remote create upload");
    assert_eq!(remote_upload, local_upload);

    let raw_chunk = vec![0, 159, 146, 150, 255, b'L', b'O', b'O', b'M'];
    let local_chunk = local
        .drive_upload_chunk_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "upload-a",
            &raw_chunk,
        )
        .expect("local upload raw chunk");
    let remote_chunk = block(Drive::drive_upload_chunk_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "upload-a".to_string(),
        raw_chunk.clone(),
    ))
    .expect("remote upload raw chunk");
    assert_eq!(remote_chunk, local_chunk);

    let clock = loom_chat::set_test_now_ms(900);
    let local_commit = local
        .drive_commit_upload_json(&local_session, "files", &drive_workspace_id, "upload-a")
        .expect("local commit upload");
    let remote_commit = block(Drive::drive_commit_upload_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "upload-a".to_string(),
    ))
    .expect("remote commit upload");
    drop(clock);
    assert_eq!(remote_commit, local_commit);
    block(Store::close(&remote, remote_handle)).expect("close remote generated session");
    assert!(local.close(&local_session));
    assert_eq!(local.session_count(), 0);
    drop(local_session);
    drop(local);
    runtime.shutdown();
    drop(remote);
    drop(runtime);
    let reopened_local = LocalLoomClient::new(&local_path);
    let reopened_local_session = LocalLoomClient::open(&reopened_local).expect("reopen local");
    let reopened_remote = LocalLoomClient::new(&remote_path);
    let reopened_remote_session = LocalLoomClient::open(&reopened_remote).expect("reopen remote");
    assert_eq!(
        drive_file_bytes_from_client(
            &reopened_local,
            &reopened_local_session,
            workspace,
            &drive_workspace_id,
            "file-a"
        ),
        raw_chunk
    );
    assert_eq!(
        drive_file_bytes_from_client(
            &reopened_remote,
            &reopened_remote_session,
            workspace,
            &drive_workspace_id,
            "file-a"
        ),
        drive_file_bytes_from_client(
            &reopened_local,
            &reopened_local_session,
            workspace,
            &drive_workspace_id,
            "file-a"
        )
    );
    assert_eq!(
        drive_file_bytes_from_client(
            &reopened_local,
            &reopened_local_session,
            workspace,
            &drive_workspace_id,
            "file-a"
        ),
        raw_chunk
    );
    assert_eq!(
        drive_file_bytes_from_client(
            &reopened_remote,
            &reopened_remote_session,
            workspace,
            &drive_workspace_id,
            "file-a"
        ),
        raw_chunk
    );
    assert_eq!(
        drive_folder_value_for_path(&remote_path, workspace, &drive_workspace_id, "root"),
        drive_folder_value_for_path(&local_path, workspace, &drive_workspace_id, "root")
    );
    assert!(reopened_local.close(&reopened_local_session));
    assert_eq!(reopened_local.session_count(), 0);
    assert!(reopened_remote.close(&reopened_remote_session));
    assert_eq!(reopened_remote.session_count(), 0);
    drop(reopened_local_session);
    drop(reopened_remote_session);
    drop(reopened_local);
    drop(reopened_remote);
    std::fs::remove_dir_all(local_path).ok();
    std::fs::remove_dir_all(remote_path).ok();
}

#[test]
fn mu_6h_j_g_b_remote_drive_share_and_retention_auth_order_matches_direct_local() {
    let workspace = WorkspaceId::from_bytes([79; 16]);
    let drive_workspace_id = workspace.to_string();
    let admin = WorkspaceId::from_bytes([80; 16]);
    let user = WorkspaceId::from_bytes([81; 16]);

    let local_path = temp_authenticated_files_store(workspace, admin, user, "auth-local");
    let local = LocalLoomClient::new(&local_path);
    let local_session = local.open().expect("open local");
    local
        .authenticate_passphrase(&local_session, user, b"userpw")
        .expect("authenticate local user");
    let remote_path = temp_authenticated_files_store(workspace, admin, user, "auth-remote");
    let (runtime, remote, remote_handle) =
        remote_client_for_store_with_auth(&remote_path, "http-drive-auth-user", user, b"userpw");

    let local_err = local
        .drive_grant_share_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "grant-bad",
            "not-a-target-kind",
            "bad-target",
            "not-a-principal",
            "not-a-role",
            1,
            Some(2),
        )
        .expect_err("local grant denied before parsing caller fields");
    let remote_err = block(Drive::drive_grant_share_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "grant-bad".to_string(),
        "not-a-target-kind".to_string(),
        "bad-target".to_string(),
        "not-a-principal".to_string(),
        "not-a-role".to_string(),
        1,
        Some(2),
    ))
    .expect_err("remote grant denied before parsing caller fields");
    assert_error_parity(local_err, remote_err);

    let local_err = local
        .drive_revoke_share_json(&local_session, "files", &drive_workspace_id, "missing")
        .expect_err("local revoke denied before lookup");
    let remote_err = block(Drive::drive_revoke_share_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "missing".to_string(),
    ))
    .expect_err("remote revoke denied before lookup");
    assert_error_parity(local_err, remote_err);

    let local_err = local
        .drive_apply_share_expiry_json(&local_session, "files", &drive_workspace_id, 55)
        .expect_err("local share expiry denied before scan");
    let remote_err = block(Drive::drive_apply_share_expiry_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        55,
    ))
    .expect_err("remote share expiry denied before scan");
    assert_error_parity(local_err, remote_err);

    let local_err = local
        .drive_pin_retention_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "pin-bad",
            "not-a-kind",
            "not-a-digest",
            Some("not-a-target"),
            1,
            Some(2),
        )
        .expect_err("local pin denied before parsing caller fields");
    let remote_err = block(Drive::drive_pin_retention_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "pin-bad".to_string(),
        "not-a-kind".to_string(),
        "not-a-digest".to_string(),
        Some("not-a-target".to_string()),
        1,
        Some(2),
    ))
    .expect_err("remote pin denied before parsing caller fields");
    assert_error_parity(local_err, remote_err);

    let local_err = local
        .drive_unpin_retention_json(&local_session, "files", &drive_workspace_id, "missing")
        .expect_err("local unpin denied before lookup");
    let remote_err = block(Drive::drive_unpin_retention_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "missing".to_string(),
    ))
    .expect_err("remote unpin denied before lookup");
    assert_error_parity(local_err, remote_err);

    let local_err = local
        .drive_apply_retention_json(&local_session, "files", &drive_workspace_id, 55)
        .expect_err("local retention apply denied before scan");
    let remote_err = block(Drive::drive_apply_retention_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        55,
    ))
    .expect_err("remote retention apply denied before scan");
    assert_error_parity(local_err, remote_err);

    block(Store::close(&remote, remote_handle)).expect("close remote generated session");
    assert!(local.close(&local_session));
    assert_eq!(local.session_count(), 0);
    drop(local_session);
    drop(local);
    runtime.shutdown();
    drop(remote);
    drop(runtime);
    std::fs::remove_dir_all(local_path).ok();
    std::fs::remove_dir_all(remote_path).ok();
}

#[test]
fn mu_6h_j_g_b_remote_drive_share_and_retention_match_direct_local() {
    let workspace = WorkspaceId::from_bytes([82; 16]);
    let drive_workspace_id = workspace.to_string();
    let admin = WorkspaceId::from_bytes([83; 16]);
    let user = WorkspaceId::from_bytes([84; 16]);
    let grantee = WorkspaceId::from_bytes([85; 16]);

    let local_path = temp_authenticated_files_store(workspace, admin, user, "parity-local");
    let local = LocalLoomClient::new(&local_path);
    let local_session = local.open().expect("open local");
    local
        .authenticate_passphrase(&local_session, admin, b"adminpw")
        .expect("authenticate local admin");
    let remote_path = temp_authenticated_files_store(workspace, admin, user, "parity-remote");
    let remote_seed = LocalLoomClient::new(&remote_path);
    let remote_seed_session = remote_seed.open().expect("open remote seed");
    remote_seed
        .authenticate_passphrase(&remote_seed_session, admin, b"adminpw")
        .expect("authenticate remote seed admin");
    let remote_initial_root = drive_root_from_client(
        &remote_seed,
        &remote_seed_session,
        workspace,
        &drive_workspace_id,
    );
    assert!(remote_seed.close(&remote_seed_session));
    assert_eq!(remote_seed.session_count(), 0);
    drop(remote_seed_session);
    drop(remote_seed);
    let (runtime, remote, remote_handle) = remote_client_for_store_with_auth(
        &remote_path,
        "http-drive-share-retention-admin",
        admin,
        b"adminpw",
    );

    let local_initial_root =
        drive_root_from_client(&local, &local_session, workspace, &drive_workspace_id);
    assert_eq!(remote_initial_root, local_initial_root);

    let clock = loom_chat::set_test_now_ms(1_000);
    let local_create = local
        .drive_create_folder_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "root",
            "folder-a",
            "A",
            &local_initial_root,
        )
        .expect("local create share target");
    let remote_create = block(Drive::drive_create_folder_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "root".to_string(),
        "folder-a".to_string(),
        "A".to_string(),
        remote_initial_root,
    ))
    .expect("remote create share target");
    drop(clock);
    assert_eq!(remote_create, local_create);
    let current_root = drive_write_root(&local_create);

    let clock = loom_chat::set_test_now_ms(1_010);
    let local_grant = local
        .drive_grant_share_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "grant-live",
            "folder",
            "folder-a",
            &grantee.to_string(),
            "viewer",
            10,
            Some(100),
        )
        .expect("local grant share");
    let remote_grant = block(Drive::drive_grant_share_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "grant-live".to_string(),
        "folder".to_string(),
        "folder-a".to_string(),
        grantee.to_string(),
        "viewer".to_string(),
        10,
        Some(100),
    ))
    .expect("remote grant share");
    drop(clock);
    assert_eq!(remote_grant, local_grant);
    assert!(drive_share_read_allowed_from_client(
        &local,
        &local_session,
        workspace,
        &drive_workspace_id,
        "folder",
        "folder-a",
        grantee
    ));

    let local_no_op = local
        .drive_apply_share_expiry_json(&local_session, "files", &drive_workspace_id, 50)
        .expect("local no-op share expiry");
    let remote_no_op = block(Drive::drive_apply_share_expiry_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        50,
    ))
    .expect("remote no-op share expiry");
    assert_eq!(remote_no_op, local_no_op);
    assert!(serde_json::from_str::<serde_json::Value>(&local_no_op).expect("share no-op json")
        ["operation"]
        .is_null());

    let clock = loom_chat::set_test_now_ms(1_020);
    let local_revoke = local
        .drive_revoke_share_json(&local_session, "files", &drive_workspace_id, "grant-live")
        .expect("local revoke share");
    let remote_revoke = block(Drive::drive_revoke_share_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "grant-live".to_string(),
    ))
    .expect("remote revoke share");
    drop(clock);
    assert_eq!(remote_revoke, local_revoke);
    assert!(!drive_share_read_allowed_from_client(
        &local,
        &local_session,
        workspace,
        &drive_workspace_id,
        "folder",
        "folder-a",
        grantee
    ));

    let clock = loom_chat::set_test_now_ms(1_030);
    let local_expiring = local
        .drive_grant_share_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "grant-expiring",
            "folder",
            "folder-a",
            &grantee.to_string(),
            "editor",
            20,
            Some(30),
        )
        .expect("local expiring grant");
    let remote_expiring = block(Drive::drive_grant_share_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "grant-expiring".to_string(),
        "folder".to_string(),
        "folder-a".to_string(),
        grantee.to_string(),
        "editor".to_string(),
        20,
        Some(30),
    ))
    .expect("remote expiring grant");
    drop(clock);
    assert_eq!(remote_expiring, local_expiring);

    let clock = loom_chat::set_test_now_ms(1_040);
    let local_expired = local
        .drive_apply_share_expiry_json(&local_session, "files", &drive_workspace_id, 30)
        .expect("local expire share");
    let remote_expired = block(Drive::drive_apply_share_expiry_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        30,
    ))
    .expect("remote expire share");
    drop(clock);
    assert_eq!(remote_expired, local_expired);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&local_expired).expect("share expired json")["expired_grant_ids"],
        serde_json::json!(["grant-expiring"])
    );

    let clock = loom_chat::set_test_now_ms(1_050);
    let local_pin = local
        .drive_pin_retention_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "pin-live",
            "trash_subtree",
            &current_root,
            Some("folder:folder-a"),
            40,
            Some(100),
        )
        .expect("local pin retention");
    let remote_pin = block(Drive::drive_pin_retention_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "pin-live".to_string(),
        "trash_subtree".to_string(),
        current_root.clone(),
        Some("folder:folder-a".to_string()),
        40,
        Some(100),
    ))
    .expect("remote pin retention");
    drop(clock);
    assert_eq!(remote_pin, local_pin);

    let local_retention_no_op = local
        .drive_apply_retention_json(&local_session, "files", &drive_workspace_id, 50)
        .expect("local no-op retention");
    let remote_retention_no_op = block(Drive::drive_apply_retention_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        50,
    ))
    .expect("remote no-op retention");
    assert_eq!(remote_retention_no_op, local_retention_no_op);
    assert!(
        serde_json::from_str::<serde_json::Value>(&local_retention_no_op)
            .expect("retention no-op json")["operation"]
            .is_null()
    );

    let clock = loom_chat::set_test_now_ms(1_060);
    let local_retention_expired = local
        .drive_apply_retention_json(&local_session, "files", &drive_workspace_id, 100)
        .expect("local apply retention");
    let remote_retention_expired = block(Drive::drive_apply_retention_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        100,
    ))
    .expect("remote apply retention");
    drop(clock);
    assert_eq!(remote_retention_expired, local_retention_expired);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&local_retention_expired)
            .expect("retention expired json")["expired_pin_ids"],
        serde_json::json!(["pin-live"])
    );

    let clock = loom_chat::set_test_now_ms(1_070);
    let local_pin_remove = local
        .drive_pin_retention_json(
            &local_session,
            "files",
            &drive_workspace_id,
            "pin-remove",
            "current_root",
            &current_root,
            None,
            110,
            None,
        )
        .expect("local pin for unpin");
    let remote_pin_remove = block(Drive::drive_pin_retention_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "pin-remove".to_string(),
        "current_root".to_string(),
        current_root,
        None,
        110,
        None,
    ))
    .expect("remote pin for unpin");
    drop(clock);
    assert_eq!(remote_pin_remove, local_pin_remove);

    let clock = loom_chat::set_test_now_ms(1_080);
    let local_unpin = local
        .drive_unpin_retention_json(&local_session, "files", &drive_workspace_id, "pin-remove")
        .expect("local unpin retention");
    let remote_unpin = block(Drive::drive_unpin_retention_json(
        &remote,
        remote_handle.clone(),
        "files".to_string(),
        drive_workspace_id.clone(),
        "pin-remove".to_string(),
    ))
    .expect("remote unpin retention");
    drop(clock);
    assert_eq!(remote_unpin, local_unpin);

    assert_eq!(
        audit_tuples_from_client(&local, &local_session),
        vec![
            (
                Some(admin.to_string()),
                "drive.share_acl.grant".to_string(),
                Some(format!("drive:{drive_workspace_id};share:grant-live"))
            ),
            (
                Some(admin.to_string()),
                "drive.share_acl.revoke".to_string(),
                Some(format!("drive:{drive_workspace_id};share:grant-live"))
            ),
            (
                Some(admin.to_string()),
                "drive.share_acl.grant".to_string(),
                Some(format!("drive:{drive_workspace_id};share:grant-expiring"))
            ),
            (
                Some(admin.to_string()),
                "drive.share_acl.expire".to_string(),
                Some(format!("drive:{drive_workspace_id};shares:expired"))
            ),
        ]
    );

    block(Store::close(&remote, remote_handle)).expect("close remote generated session");
    assert!(local.close(&local_session));
    assert_eq!(local.session_count(), 0);
    drop(local_session);
    drop(local);
    runtime.shutdown();
    drop(remote);
    drop(runtime);

    let reopened_local = LocalLoomClient::new(&local_path);
    let reopened_local_session = reopened_local.open().expect("reopen local");
    reopened_local
        .authenticate_passphrase(&reopened_local_session, admin, b"adminpw")
        .expect("authenticate reopened local");
    let reopened_remote = LocalLoomClient::new(&remote_path);
    let reopened_remote_session = reopened_remote.open().expect("reopen remote");
    reopened_remote
        .authenticate_passphrase(&reopened_remote_session, admin, b"adminpw")
        .expect("authenticate reopened remote");
    assert_eq!(
        drive_shares_from_client(
            &reopened_remote,
            &reopened_remote_session,
            workspace,
            &drive_workspace_id
        ),
        serde_json::json!([])
    );
    assert_eq!(
        drive_shares_from_client(
            &reopened_remote,
            &reopened_remote_session,
            workspace,
            &drive_workspace_id
        ),
        drive_shares_from_client(
            &reopened_local,
            &reopened_local_session,
            workspace,
            &drive_workspace_id
        )
    );
    assert!(!drive_share_read_allowed_from_client(
        &reopened_remote,
        &reopened_remote_session,
        workspace,
        &drive_workspace_id,
        "folder",
        "folder-a",
        grantee
    ));
    assert_eq!(
        drive_retention_from_client(
            &reopened_remote,
            &reopened_remote_session,
            workspace,
            &drive_workspace_id
        ),
        serde_json::json!([])
    );
    assert_eq!(
        drive_retention_from_client(
            &reopened_remote,
            &reopened_remote_session,
            workspace,
            &drive_workspace_id
        ),
        drive_retention_from_client(
            &reopened_local,
            &reopened_local_session,
            workspace,
            &drive_workspace_id
        )
    );
    assert_eq!(
        audit_tuples_from_client(&reopened_remote, &reopened_remote_session),
        audit_tuples_from_client(&reopened_local, &reopened_local_session)
    );
    assert!(reopened_local.close(&reopened_local_session));
    assert_eq!(reopened_local.session_count(), 0);
    assert!(reopened_remote.close(&reopened_remote_session));
    assert_eq!(reopened_remote.session_count(), 0);
    drop(reopened_local_session);
    drop(reopened_remote_session);
    drop(reopened_local);
    drop(reopened_remote);
    std::fs::remove_dir_all(local_path).ok();
    std::fs::remove_dir_all(remote_path).ok();
}

#[test]
fn mu_6h_j_g_c_remote_drive_generated_mutation_parity_table() {
    let workspace = WorkspaceId::from_bytes([86; 16]);
    let other_workspace = WorkspaceId::from_bytes([87; 16]);
    let drive_workspace_id = workspace.to_string();
    let other_drive_workspace_id = other_workspace.to_string();
    let admin = WorkspaceId::from_bytes([88; 16]);
    let user = WorkspaceId::from_bytes([89; 16]);
    let grantee = WorkspaceId::from_bytes([90; 16]);

    let groups = [
        DriveParityGroup::HierarchyConflict,
        DriveParityGroup::Upload,
        DriveParityGroup::Sharing,
        DriveParityGroup::Retention,
    ];

    for group in groups {
        let local_path = temp_authenticated_two_files_store(
            workspace,
            other_workspace,
            admin,
            user,
            group.slug(),
        );
        let (local, local_session) = open_authenticated_local(&local_path, admin, b"adminpw");
        seed_drive_failure_group(
            &local,
            &local_session,
            group,
            workspace,
            &drive_workspace_id,
            grantee,
        );
        let local_before = drive_parity_state_from_client(
            &local,
            &local_session,
            workspace,
            &drive_workspace_id,
            other_workspace,
            &other_drive_workspace_id,
            grantee,
        );
        let local_result = run_local_drive_failure_group(
            &local,
            &local_session,
            group,
            workspace,
            &drive_workspace_id,
            grantee,
        );
        let local_after = drive_parity_state_from_client(
            &local,
            &local_session,
            workspace,
            &drive_workspace_id,
            other_workspace,
            &other_drive_workspace_id,
            grantee,
        );
        assert_eq!(
            local_after,
            local_before,
            "{} direct-local failed mutation changed state",
            group.name()
        );
        assert!(local.close(&local_session));
        assert_eq!(local.session_count(), 0);
        drop(local_session);
        drop(local);

        let remote_path = temp_authenticated_two_files_store(
            workspace,
            other_workspace,
            admin,
            user,
            group.slug(),
        );
        let (remote_seed, remote_seed_session) =
            open_authenticated_local(&remote_path, admin, b"adminpw");
        seed_drive_failure_group(
            &remote_seed,
            &remote_seed_session,
            group,
            workspace,
            &drive_workspace_id,
            grantee,
        );
        let remote_before = drive_parity_state_from_client(
            &remote_seed,
            &remote_seed_session,
            workspace,
            &drive_workspace_id,
            other_workspace,
            &other_drive_workspace_id,
            grantee,
        );
        assert_eq!(
            remote_before,
            local_before,
            "{} seed state differs before remote failure",
            group.name()
        );
        assert!(remote_seed.close(&remote_seed_session));
        assert_eq!(remote_seed.session_count(), 0);
        drop(remote_seed_session);
        drop(remote_seed);

        let (runtime, remote, remote_handle) = remote_client_for_store_with_auth(
            &remote_path,
            &format!("http-drive-g-c-failure-{}", group.slug()),
            admin,
            b"adminpw",
        );
        let remote_result = run_remote_drive_failure_group(
            &remote,
            &remote_handle,
            group,
            &drive_workspace_id,
            grantee,
            &remote_before.selected_root,
        );
        assert_failed_drive_result_parity(group, local_result, remote_result);
        block(Store::close(&remote, remote_handle)).expect("close remote generated session");
        runtime.shutdown();
        drop(remote);
        drop(runtime);

        let (reopened_remote, reopened_remote_session) =
            open_authenticated_local(&remote_path, admin, b"adminpw");
        let remote_after = drive_parity_state_from_client(
            &reopened_remote,
            &reopened_remote_session,
            workspace,
            &drive_workspace_id,
            other_workspace,
            &other_drive_workspace_id,
            grantee,
        );
        assert_eq!(
            remote_after,
            remote_before,
            "{} hosted/remote failed mutation changed state",
            group.name()
        );
        assert!(reopened_remote.close(&reopened_remote_session));
        assert_eq!(reopened_remote.session_count(), 0);
        drop(reopened_remote_session);
        drop(reopened_remote);
        std::fs::remove_dir_all(local_path).ok();
        std::fs::remove_dir_all(remote_path).ok();
    }

    let local_path = temp_authenticated_two_files_store(
        workspace,
        other_workspace,
        admin,
        user,
        "success-local",
    );
    let remote_path = temp_authenticated_two_files_store(
        workspace,
        other_workspace,
        admin,
        user,
        "success-remote",
    );
    let (local, local_session) = open_authenticated_local(&local_path, admin, b"adminpw");
    let local_before = drive_parity_state_from_client(
        &local,
        &local_session,
        workspace,
        &drive_workspace_id,
        other_workspace,
        &other_drive_workspace_id,
        grantee,
    );

    let (remote_seed, remote_seed_session) =
        open_authenticated_local(&remote_path, admin, b"adminpw");
    let remote_before = drive_parity_state_from_client(
        &remote_seed,
        &remote_seed_session,
        workspace,
        &drive_workspace_id,
        other_workspace,
        &other_drive_workspace_id,
        grantee,
    );
    assert_eq!(remote_before, local_before);
    let remote_initial_root = remote_before.selected_root.clone();
    assert!(remote_seed.close(&remote_seed_session));
    assert_eq!(remote_seed.session_count(), 0);
    drop(remote_seed_session);
    drop(remote_seed);

    let (runtime, remote, remote_handle) = remote_client_for_store_with_auth(
        &remote_path,
        "http-drive-g-c-success",
        admin,
        b"adminpw",
    );
    let local_outputs = run_local_drive_success_sequence(
        &local,
        &local_session,
        workspace,
        &drive_workspace_id,
        grantee,
    );
    let remote_outputs = run_remote_drive_success_sequence(
        &remote,
        &remote_handle,
        remote_initial_root,
        &drive_workspace_id,
        grantee,
    );
    assert_eq!(remote_outputs, local_outputs);

    let local_after = drive_parity_state_from_client(
        &local,
        &local_session,
        workspace,
        &drive_workspace_id,
        other_workspace,
        &other_drive_workspace_id,
        grantee,
    );
    assert_eq!(local_after.unrelated_root, local_before.unrelated_root);
    assert_eq!(
        local_after.unrelated_root_folder,
        local_before.unrelated_root_folder
    );
    assert_eq!(
        local_after.unrelated_conflicts,
        local_before.unrelated_conflicts
    );
    assert_eq!(local_after.unrelated_shares, local_before.unrelated_shares);
    assert_eq!(
        local_after.unrelated_retention,
        local_before.unrelated_retention
    );

    block(Store::close(&remote, remote_handle)).expect("close remote generated session");
    assert!(local.close(&local_session));
    assert_eq!(local.session_count(), 0);
    drop(local_session);
    drop(local);
    runtime.shutdown();
    drop(remote);
    drop(runtime);

    let (reopened_local, reopened_local_session) =
        open_authenticated_local(&local_path, admin, b"adminpw");
    let (reopened_remote, reopened_remote_session) =
        open_authenticated_local(&remote_path, admin, b"adminpw");
    let reopened_local_state = drive_parity_state_from_client(
        &reopened_local,
        &reopened_local_session,
        workspace,
        &drive_workspace_id,
        other_workspace,
        &other_drive_workspace_id,
        grantee,
    );
    let reopened_remote_state = drive_parity_state_from_client(
        &reopened_remote,
        &reopened_remote_session,
        workspace,
        &drive_workspace_id,
        other_workspace,
        &other_drive_workspace_id,
        grantee,
    );
    assert_eq!(reopened_remote_state, reopened_local_state);
    assert_eq!(
        reopened_local_state.unrelated_root,
        local_before.unrelated_root
    );
    assert_eq!(
        reopened_local_state.unrelated_root_folder,
        local_before.unrelated_root_folder
    );
    assert_eq!(
        reopened_local_state.unrelated_conflicts,
        local_before.unrelated_conflicts
    );
    assert_eq!(
        reopened_local_state.unrelated_shares,
        local_before.unrelated_shares
    );
    assert_eq!(
        reopened_local_state.unrelated_retention,
        local_before.unrelated_retention
    );
    assert_eq!(
        drive_file_bytes_from_client(
            &reopened_local,
            &reopened_local_session,
            workspace,
            &drive_workspace_id,
            "file-a"
        ),
        vec![0, 159, 146, 150, 255, b'L', b'O', b'O', b'M']
    );
    assert!(reopened_local.close(&reopened_local_session));
    assert_eq!(reopened_local.session_count(), 0);
    assert!(reopened_remote.close(&reopened_remote_session));
    assert_eq!(reopened_remote.session_count(), 0);
    drop(reopened_local_session);
    drop(reopened_remote_session);
    drop(reopened_local);
    drop(reopened_remote);
    std::fs::remove_dir_all(local_path).ok();
    std::fs::remove_dir_all(remote_path).ok();
}

#[test]
fn remote_ticket_comment_json_round_trips_over_http_carrier_semantics() {
    let path = temp_store();
    let (ticket_workspace_id, ticket_id, ticket_root) = seed_ticket(&path);
    let runtime = Arc::new(RemoteRuntime::start(&path, config()).expect("start"));
    let connection = runtime.register_connection("http-ticket-client");
    let session = runtime
        .open_session(connection, RemoteAuth::Unauthenticated)
        .expect("session");
    let service = Arc::new(RemoteHttpService::new(runtime.clone(), CALL_PATH));
    let transport = HttpBridge {
        service,
        call_path: CALL_PATH.to_string(),
    };
    let resolver = ContextResolver::from_layers(&[Layer::new(
        "test",
        "[contexts.prod]\ntarget = \"https://remote.host/apps/loom\"\n",
    )])
    .unwrap();
    let conn = block(RemoteConnection::connect(
        transport,
        "prod",
        &resolver,
        DiscoveryMode::Default,
    ))
    .expect("connect over http carrier");
    let client = RemoteLoomClient::new(conn);
    client.bind_session(session.id.clone());
    let handle = block(client.open()).expect("open store session");

    let add = block(Tickets::tickets_comment_add_json(
        &client,
        handle.clone(),
        "repo".to_string(),
        ticket_workspace_id.clone(),
        ticket_id.clone(),
        Some("c1".to_string()),
        Some("review_request".to_string()),
        "Ready for review".to_string(),
        None,
        Some(ticket_root),
    ))
    .expect("add comment");
    let add: serde_json::Value = serde_json::from_str(&add).expect("add json");
    assert_eq!(add["receipt"]["operation"], "ticket.comment_added");
    let add_root = add["resource"]["profile_root"].as_str().expect("add root");

    let comments = block(Tickets::tickets_comments_json(
        &client,
        handle.clone(),
        "repo".to_string(),
        ticket_workspace_id.clone(),
        ticket_id.clone(),
    ))
    .expect("list comments");
    let comments: serde_json::Value = serde_json::from_str(&comments).expect("comments json");
    assert_eq!(comments[0]["comment_id"], "c1");
    assert_eq!(comments[0]["body"], "Ready for review");

    let update = block(Tickets::tickets_comment_update_json(
        &client,
        handle.clone(),
        "repo".to_string(),
        ticket_workspace_id.clone(),
        ticket_id.clone(),
        "c1".to_string(),
        Some("review_feedback".to_string()),
        Some("Needs evidence".to_string()),
        None,
        Some(add_root.to_string()),
    ))
    .expect("update comment");
    let update: serde_json::Value = serde_json::from_str(&update).expect("update json");
    assert_eq!(update["receipt"]["operation"], "ticket.comment_updated");
    let update_root = update["resource"]["profile_root"]
        .as_str()
        .expect("update root");

    let delete = block(Tickets::tickets_comment_delete_json(
        &client,
        handle,
        "repo".to_string(),
        ticket_workspace_id.clone(),
        ticket_id.clone(),
        "c1".to_string(),
        Some(update_root.to_string()),
    ))
    .expect("delete comment");
    let delete: serde_json::Value = serde_json::from_str(&delete).expect("delete json");
    assert_eq!(delete["receipt"]["operation"], "ticket.comment_deleted");

    runtime.shutdown();
    std::fs::remove_dir_all(&path).ok();
}

#[test]
fn remote_ticket_update_json_composes_fields_status_comments_and_relations() {
    let path = temp_store();
    let (ticket_workspace_id, source_ticket_id, target_ticket_id, target_root) =
        seed_ticket_pair(&path);
    let runtime = Arc::new(RemoteRuntime::start(&path, config()).expect("start"));
    let connection = runtime.register_connection("http-ticket-update-client");
    let session = runtime
        .open_session(connection, RemoteAuth::Unauthenticated)
        .expect("session");
    let service = Arc::new(RemoteHttpService::new(runtime.clone(), CALL_PATH));
    let transport = HttpBridge {
        service,
        call_path: CALL_PATH.to_string(),
    };
    let resolver = ContextResolver::from_layers(&[Layer::new(
        "test",
        "[contexts.prod]\ntarget = \"https://remote.host/apps/loom\"\n",
    )])
    .unwrap();
    let conn = block(RemoteConnection::connect(
        transport,
        "prod",
        &resolver,
        DiscoveryMode::Default,
    ))
    .expect("connect over http carrier");
    let client = RemoteLoomClient::new(conn);
    client.bind_session(session.id.clone());
    let handle = block(client.open()).expect("open store session");

    let update = block(Tickets::tickets_update_json(
        &client,
        handle,
        "repo".to_string(),
        ticket_workspace_id,
        source_ticket_id,
        Some(serde_json::json!({"priority": "P1"}).to_string()),
        "[]".to_string(),
        None,
        Some("blocked".to_string()),
        Some("planned".to_string()),
        None,
        None,
        Some("single-comment".to_string()),
        Some("blocker".to_string()),
        Some("Blocked on dependency".to_string()),
        None,
        Some(target_root),
        Some(
            serde_json::json!([
                {"comment_id": "array-comment", "comment_type": "progress", "body": "Investigated root cause"}
            ])
            .to_string(),
        ),
        Some(
            serde_json::json!([
                {"relation_id": "dependency", "kind": "depends_on", "target_id": target_ticket_id}
            ])
            .to_string(),
        ),
        None,
    ))
    .expect("update ticket");
    let update: serde_json::Value = serde_json::from_str(&update).expect("update json");
    assert_eq!(update["receipt"]["operation"], "ticket.updated");
    assert_eq!(update["resource"]["fields"]["status"], "blocked");
    assert_eq!(update["resource"]["fields"]["priority"], "P1");
    assert_eq!(update["resource"]["comments"].as_array().unwrap().len(), 2);
    assert_eq!(
        update["resource"]["relations"][0]["relation_id"],
        "dependency"
    );

    runtime.shutdown();
    std::fs::remove_dir_all(&path).ok();
}
