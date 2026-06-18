//! End-to-end HTTP carrier parity: a `RemoteLoomClient` whose transport bridges the client HTTP mapping
//! to the server `RemoteHttpService` over a live `RemoteRuntime`, proving discovery and unary calls round
//! trip through HTTP request/response semantics without a socket.
//!
//! Licensed under BUSL-1.1.

use loom_client::LocalLoomClient;
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
use loom_remote_protocol::generated_api::{Kv, Queue, Store, Tickets, Workspaces};
use loom_store::save_loom;
use loom_types::LoomError;
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
