#![cfg(feature = "mcp-daemon-cli-tests")]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use loom_core::{
    AclEffect, AclGrant, AclRight, AclScope, AclStore, AclSubject, Algo, FacetKind, IdentityStore,
    Loom, PrincipalKind, WorkspaceId,
};
use loom_remote_protocol::api_types::LoomSession;
use loom_remote_protocol::codec::{FromValue, ToValue};
use loom_remote_protocol::envelope::{Compression, Request, Response, ResponsePayload};
use loom_remote_protocol::session::{SessionAuth, SessionOpenReply};
use loom_store::{FileStore, daemon};
use serde_json::{Value, json};

const WORKSPACE: &str = "repo";
const PROJECT_ID: &str = "core";
const DAEMON_GENERATED_CALL_MAGIC: &[u8] = b"loom-daemon-generated-call-v1\0";
static NEXT_GENERATED_REQUEST: AtomicU64 = AtomicU64::new(1);

struct DaemonStore {
    path: String,
}

impl DaemonStore {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-daemon-mcp-authority-{tag}-{}-{}.loom",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = path.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&path);
        Self { path }
    }

    fn init(&self) {
        let store = FileStore::create_with_profile(&self.path, Algo::Blake3).unwrap();
        let workspace = WorkspaceId::v4_from_bytes([51; 16]);
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(FacetKind::Files, Some(WORKSPACE), workspace)
            .unwrap();
        loom.registry_mut()
            .add_facet(workspace, FacetKind::Vcs)
            .unwrap();
        loom.registry_mut()
            .add_facet(workspace, FacetKind::Graph)
            .unwrap();
        let workspace_id = workspace.to_string();
        loom_tickets::create_project(
            &mut loom,
            workspace,
            &workspace_id,
            PROJECT_ID,
            "CORE",
            "Core",
            None,
        )
        .unwrap();
        loom_store::save_loom(&mut loom).unwrap();
    }

    fn start(&mut self) {
        loom(["daemon", "start", &self.path, "--transport", "tcp"]).unwrap();
    }

    fn stop(&mut self) {
        loom(["daemon", "stop", "--hard", &self.path]).unwrap();
    }

    fn stop_with_auth(&mut self, principal: WorkspaceId, passphrase: &str) {
        let passphrase_path = temp_text_path("daemon-auth-passphrase", passphrase);
        let auth_source = format!("file:{}", passphrase_path.display());
        let principal = principal.to_string();
        loom([
            "--auth-principal",
            &principal,
            "--auth-key-source",
            &auth_source,
            "daemon",
            "stop",
            "--hard",
            &self.path,
        ])
        .unwrap();
        let _ = std::fs::remove_file(passphrase_path);
    }

    fn status(&self) -> String {
        loom(["daemon", "status", &self.path]).unwrap()
    }

    fn configure_cas_listener(&self, addr: SocketAddr) {
        let mut loom = loom_store::open_loom_unlocked(&self.path, None).unwrap();
        loom.registry_mut()
            .create(
                FacetKind::Cas,
                Some("main"),
                WorkspaceId::v4_from_bytes([6; 16]),
            )
            .unwrap();
        loom_store::save_loom(&mut loom).unwrap();
        drop(loom);
        let record = FileStore::served_listener_record(
            "cas",
            vec!["main".to_string()],
            "rest",
            &addr.to_string(),
            true,
        )
        .unwrap();
        let fs = FileStore::open(&self.path).unwrap();
        fs.save_served_listener_audited(
            &record,
            None,
            "serve.listener.configure",
            Some(&format!("id={}", record.id)),
        )
        .unwrap();
    }

    fn seed_graph_edge(&self, collection: &str, edge_id: &str) {
        let mut loom = loom_store::open_loom_unlocked(&self.path, None).unwrap();
        let workspace = WorkspaceId::v4_from_bytes([51; 16]);
        loom_core::graph_upsert_node(&mut loom, workspace, collection, "source", BTreeMap::new())
            .unwrap();
        loom_core::graph_upsert_node(&mut loom, workspace, collection, "target", BTreeMap::new())
            .unwrap();
        loom_reference::upsert_graph_edge_indexed(
            &mut loom,
            workspace,
            collection,
            edge_id,
            "source",
            "target",
            "relates",
            BTreeMap::new(),
        )
        .unwrap();
        loom_store::save_loom(&mut loom).unwrap();
    }

    fn configure_identity_store(&self) -> WorkspaceId {
        let mut loom = loom_store::open_loom_unlocked(&self.path, None).unwrap();
        let root = WorkspaceId::v4_from_bytes([71; 16]);
        let mut identity = IdentityStore::new(root);
        identity
            .set_passphrase(root, "root-pass", b"12345678")
            .unwrap();
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(root),
            None,
            None,
            [AclRight::Admin, AclRight::Read, AclRight::Write],
        )
        .unwrap();
        loom.store().save_identity_store(&identity).unwrap();
        loom.store().save_acl_store(&acl).unwrap();
        loom.set_identity_store(identity);
        root
    }

    fn configure_identity_store_with_reader(&self) -> (WorkspaceId, WorkspaceId) {
        let mut loom = loom_store::open_loom_unlocked(&self.path, None).unwrap();
        let root = WorkspaceId::v4_from_bytes([72; 16]);
        let reader = WorkspaceId::v4_from_bytes([73; 16]);
        let mut identity = IdentityStore::new(root);
        identity
            .add_principal(reader, "reader", PrincipalKind::User)
            .unwrap();
        identity
            .set_passphrase(root, "root-pass", b"12345678")
            .unwrap();
        identity
            .set_passphrase(reader, "reader-pass", b"12345678")
            .unwrap();
        let workspace = WorkspaceId::v4_from_bytes([51; 16]);
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(root),
            None,
            None,
            [AclRight::Admin, AclRight::Read, AclRight::Write],
        )
        .unwrap();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(reader),
            workspace: Some(workspace),
            domain: Some(loom_core::workspace::AclDomain::Tickets),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: [AclRight::Read].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();
        loom.store().save_identity_store(&identity).unwrap();
        loom.store().save_acl_store(&acl).unwrap();
        loom.set_identity_store(identity);
        loom.set_acl_store(acl);
        (root, reader)
    }
}

impl Drop for DaemonStore {
    fn drop(&mut self) {
        let _ = loom(["daemon", "stop", "--hard", &self.path]);
        let _ = std::fs::remove_file(&self.path);
    }
}

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: Receiver<Value>,
    stderr: Receiver<String>,
    next_id: u64,
}

impl McpClient {
    fn spawn(store: &str) -> Self {
        Self::spawn_with_env(store, [])
    }

    fn spawn_with_env<const N: usize>(store: &str, envs: [(&str, &str); N]) -> Self {
        Self::spawn_with_prefix_args_and_env(store, Vec::new(), envs)
    }

    fn spawn_with_auth(store: &str, principal: WorkspaceId, passphrase: &str) -> Self {
        let passphrase_path = temp_text_path("mcp-auth-passphrase", passphrase);
        let auth_source = format!("file:{}", passphrase_path.display());
        let principal = principal.to_string();
        let client = Self::spawn_with_prefix_args_and_env(
            store,
            vec![
                "--auth-principal".to_string(),
                principal,
                "--auth-key-source".to_string(),
                auth_source,
            ],
            [],
        );
        let _ = std::fs::remove_file(passphrase_path);
        client
    }

    fn spawn_with_prefix_args_and_env<const N: usize>(
        store: &str,
        prefix_args: Vec<String>,
        envs: [(&str, &str); N],
    ) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_loom"));
        command
            .args(prefix_args)
            .args(["mcp", store])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in envs {
            command.env(key, value);
        }
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (stdout_tx, stdout_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Ok(value) = serde_json::from_str::<Value>(&line) {
                            let _ = stdout_tx.send(value);
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let (stderr_tx, stderr_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut stderr_text = String::new();
            let _ = BufReader::new(stderr).read_to_string(&mut stderr_text);
            let _ = stderr_tx.send(stderr_text);
        });
        let mut client = Self {
            child,
            stdin,
            stdout: stdout_rx,
            stderr: stderr_rx,
            next_id: 1,
        };
        client.initialize();
        client
    }

    fn initialize(&mut self) {
        let response = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {
                    "name": "loom-daemon-mcp-authority-test",
                    "version": "0.0.0"
                }
            }),
        );
        assert!(
            response.get("error").is_none(),
            "initialize failed: {response:?}"
        );
        self.notify("notifications/initialized", json!({}));
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.write(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.write(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        self.read_response(id)
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.request(
            "tools/call",
            json!({
                "name": name,
                "arguments": arguments,
            }),
        );
        assert!(!response_is_error(&response), "{name} failed: {response:?}");
        response
    }

    fn call_tool_error(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.request(
            "tools/call",
            json!({
                "name": name,
                "arguments": arguments,
            }),
        );
        assert!(
            response_is_error(&response),
            "{name} unexpectedly succeeded: {response:?}"
        );
        response
    }

    fn request_after_daemon_stop(&mut self) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.write(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": "tickets_list",
                "arguments": {
                    "workspace": WORKSPACE,
                    "limit": 10
                }
            }
        }));
        match self.stdout.recv_timeout(Duration::from_secs(5)) {
            Ok(value) if value.get("id").and_then(Value::as_u64) == Some(id) => Ok(value),
            Ok(value) => Ok(value),
            Err(_) => Err(self.stderr.try_recv().unwrap_or_default()),
        }
    }

    fn read_response(&self, id: u64) -> Value {
        for _ in 0..32 {
            let value = self
                .stdout
                .recv_timeout(Duration::from_secs(20))
                .unwrap_or_else(|error| {
                    let stderr = self.stderr.try_recv().unwrap_or_default();
                    panic!("MCP response {id} timed out: {error}; stderr:\n{stderr}");
                });
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                return value;
            }
        }
        panic!("MCP response {id} was not received");
    }

    fn write(&mut self, value: Value) {
        serde_json::to_writer(&mut self.stdin, &value).unwrap();
        self.stdin.write_all(b"\n").unwrap();
        self.stdin.flush().unwrap();
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn loom<const N: usize>(args: [&str; N]) -> Result<String, String> {
    loom_with_env(args, [])
}

fn loom_with_env<const N: usize, const M: usize>(
    args: [&str; N],
    envs: [(&str, &str); M],
) -> Result<String, String> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loom"));
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command
        .output()
        .map_err(|error| format!("spawn loom: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "loom {} failed with {}\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn loom_with_auth<const N: usize>(
    principal: WorkspaceId,
    passphrase_path: &std::path::Path,
    args: [&str; N],
) -> Result<String, String> {
    let principal = principal.to_string();
    let auth_source = format!("file:{}", passphrase_path.display());
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args([
            "--auth-principal",
            &principal,
            "--auth-key-source",
            &auth_source,
        ])
        .args(args)
        .output()
        .map_err(|error| format!("spawn authenticated loom: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "authenticated loom failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn temp_text_path(tag: &str, text: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "loom-daemon-mcp-authority-{tag}-{}-{}.txt",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(&path, text).unwrap();
    path
}

fn spawn_holding_cli(store: &DaemonStore, millis: u64) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_loom"));
    command
        .args([
            "mcp-daemon-cli-test-hold-session",
            &store.path,
            "--millis",
            &millis.to_string(),
        ])
        .env("ULDREN_LOOM_DAEMON_SESSION_RENEWAL_MS", "1000")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut ready = String::new();
    reader.read_line(&mut ready).unwrap();
    if ready.trim() != "holding" {
        let status = child.wait().unwrap();
        let mut stderr = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        panic!("holding CLI did not start: status={status}; stdout={ready:?}; stderr={stderr}");
    }
    child
}

fn wait_for_status(store: &DaemonStore, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last = String::new();
    while Instant::now() < deadline {
        last = store.status();
        if last.contains(needle) {
            return last;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("daemon status did not contain {needle:?}: {last}");
}

fn wait_for_stopped(store: &DaemonStore) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last = String::new();
    while Instant::now() < deadline {
        last = store.status();
        if last.starts_with("stopped\t") {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("daemon did not stop: {last}");
}

struct GeneratedDaemonClient {
    paths: daemon::DaemonPaths,
    session_id: Vec<u8>,
    handle: LoomSession,
}

impl GeneratedDaemonClient {
    fn connect(store: &str) -> Self {
        let paths = daemon::paths(store).unwrap();
        let open = loom_remote_protocol::session::open_request_bytes(&SessionAuth::Unauthenticated);
        let reply = daemon::generated_session_open(&paths, &open).unwrap();
        let session_id = match loom_remote_protocol::session::parse_open_reply(&reply).unwrap() {
            SessionOpenReply::Ok { session_id, .. } => session_id,
            SessionOpenReply::Err(error) => panic!("session open failed: {error:?}"),
        };
        let payload = generated_call(
            &paths,
            Some(session_id.clone()),
            "Store",
            "open",
            Vec::new(),
            None,
        )
        .unwrap();
        let ResponsePayload::Ok(value) = payload else {
            panic!("Store.open returned {payload:?}");
        };
        let handle = LoomSession::from_value(&value).unwrap();
        Self {
            paths,
            session_id,
            handle,
        }
    }

    fn call(&self, interface: &str, method: &str, args: Vec<loom_codec::Value>) -> ResponsePayload {
        self.call_with_idempotency(interface, method, args, None)
    }

    fn call_with_idempotency(
        &self,
        interface: &str,
        method: &str,
        mut args: Vec<loom_codec::Value>,
        idempotency_key: Option<Vec<u8>>,
    ) -> ResponsePayload {
        let mut with_handle = Vec::with_capacity(args.len() + 1);
        with_handle.push(self.handle.to_value());
        with_handle.append(&mut args);
        generated_call(
            &self.paths,
            Some(self.session_id.clone()),
            interface,
            method,
            with_handle,
            idempotency_key,
        )
        .unwrap()
    }
}

fn generated_call(
    paths: &daemon::DaemonPaths,
    session_id: Option<Vec<u8>>,
    interface: &str,
    method: &str,
    args: Vec<loom_codec::Value>,
    idempotency_key: Option<Vec<u8>>,
) -> Result<ResponsePayload, String> {
    let request = generated_request(session_id, interface, method, args, idempotency_key);
    let response = daemon::generated_call(paths, &request).map_err(|error| error.to_string())?;
    Response::decode(&response)
        .map(|response| response.payload)
        .map_err(|error| format!("decode generated response: {error}"))
}

fn send_generated_call_and_drop_response(
    paths: &daemon::DaemonPaths,
    session_id: Option<Vec<u8>>,
    interface: &str,
    method: &str,
    args: Vec<loom_codec::Value>,
    idempotency_key: Option<Vec<u8>>,
) -> std::io::Result<()> {
    let request = generated_request(session_id, interface, method, args, idempotency_key);
    let frame = generated_binary_request(DAEMON_GENERATED_CALL_MAGIC, &request);
    let contents = std::fs::read_to_string(&paths.addr_file).unwrap();
    let addr = contents
        .lines()
        .find_map(|line| line.strip_prefix("addr="))
        .unwrap_or(contents.trim());
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(&frame).unwrap();
    stream.shutdown(Shutdown::Both)
}

struct DroppedGeneratedResponse {
    forwarded_request: Request,
    response_bytes: usize,
}

fn call_generated_through_response_drop_proxy(
    paths: &daemon::DaemonPaths,
    session_id: Option<Vec<u8>>,
    interface: &str,
    method: &str,
    args: Vec<loom_codec::Value>,
) -> (Result<ResponsePayload, String>, DroppedGeneratedResponse) {
    let original_addr = std::fs::read_to_string(&paths.addr_file).unwrap();
    let real_addr = daemon_addr_from_contents(&original_addr);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    std::fs::write(
        &paths.addr_file,
        daemon::addr_file_contents(paths, proxy_addr),
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    let proxy = std::thread::spawn(move || {
        let (mut caller, _) = listener.accept().unwrap();
        let mut request_frame = Vec::new();
        caller.read_to_end(&mut request_frame).unwrap();
        let request_body = generated_binary_request_body(&request_frame);
        let forwarded_request = Request::decode(request_body).unwrap();
        let mut daemon = TcpStream::connect(real_addr).unwrap();
        daemon.write_all(&request_frame).unwrap();
        daemon.shutdown(Shutdown::Write).unwrap();
        let mut response = Vec::new();
        daemon.read_to_end(&mut response).unwrap();
        tx.send(DroppedGeneratedResponse {
            forwarded_request,
            response_bytes: response.len(),
        })
        .unwrap();
    });

    let result = generated_call(paths, session_id, interface, method, args, None);
    std::fs::write(&paths.addr_file, original_addr).unwrap();
    proxy.join().unwrap();
    (result, rx.recv_timeout(Duration::from_secs(5)).unwrap())
}

fn call_generated_through_malformed_response_proxy(
    paths: &daemon::DaemonPaths,
    session_id: Option<Vec<u8>>,
    interface: &str,
    method: &str,
    args: Vec<loom_codec::Value>,
) -> Result<ResponsePayload, String> {
    let original_addr = std::fs::read_to_string(&paths.addr_file).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    std::fs::write(
        &paths.addr_file,
        daemon::addr_file_contents(paths, proxy_addr),
    )
    .unwrap();
    let proxy = std::thread::spawn(move || {
        let (mut caller, _) = listener.accept().unwrap();
        let mut request_frame = Vec::new();
        caller.read_to_end(&mut request_frame).unwrap();
        assert!(!request_frame.is_empty());
        caller.write_all(b"not-a-generated-response-frame").unwrap();
    });

    let result = generated_call(paths, session_id, interface, method, args, None);
    std::fs::write(&paths.addr_file, original_addr).unwrap();
    proxy.join().unwrap();
    result
}

fn daemon_addr_from_contents(contents: &str) -> String {
    contents
        .lines()
        .find_map(|line| line.strip_prefix("addr="))
        .unwrap_or(contents.trim())
        .to_string()
}

fn generated_binary_request_body(frame: &[u8]) -> &[u8] {
    let rest = frame
        .strip_prefix(DAEMON_GENERATED_CALL_MAGIC)
        .expect("generated call magic");
    let len = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
    let body = &rest[4..4 + len];
    assert_eq!(rest.len(), 4 + len);
    body
}

fn generated_request(
    session_id: Option<Vec<u8>>,
    interface: &str,
    method: &str,
    args: Vec<loom_codec::Value>,
    idempotency_key: Option<Vec<u8>>,
) -> Vec<u8> {
    Request {
        request_id: NEXT_GENERATED_REQUEST
            .fetch_add(1, Ordering::Relaxed)
            .to_be_bytes()
            .to_vec(),
        session_id,
        interface: interface.to_string(),
        method: method.to_string(),
        args,
        deadline_ms: 0,
        idempotency_key,
        principal_hint: None,
        compression: Compression::None,
        stream: false,
    }
    .encode()
    .unwrap()
}

fn generated_binary_request(magic: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(magic.len() + 4 + body.len());
    out.extend_from_slice(magic);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    out
}

fn assert_generated_edge_present(client: &GeneratedDaemonClient, collection: &str, edge_id: &str) {
    let payload = client.call(
        "Graph",
        "get_edge",
        vec![
            loom_codec::Value::Text(WORKSPACE.to_string()),
            loom_codec::Value::Text(collection.to_string()),
            loom_codec::Value::Text(edge_id.to_string()),
        ],
    );
    assert!(
        matches!(payload, ResponsePayload::Ok(loom_codec::Value::Bytes(_))),
        "expected generated edge bytes, got {payload:?}"
    );
}

fn assert_generated_edge_absent(client: &GeneratedDaemonClient, collection: &str, edge_id: &str) {
    let payload = client.call(
        "Graph",
        "get_edge",
        vec![
            loom_codec::Value::Text(WORKSPACE.to_string()),
            loom_codec::Value::Text(collection.to_string()),
            loom_codec::Value::Text(edge_id.to_string()),
        ],
    );
    assert_eq!(payload, ResponsePayload::Ok(loom_codec::Value::Null));
}

fn assert_mcp_graph_edge_absent(client: &mut McpClient, collection: &str, edge_id: &str) {
    let response = client.call_tool(
        "graph_get_edge",
        json!({
            "workspace": WORKSPACE,
            "collection": collection,
            "id": edge_id
        }),
    );
    assert_contains(&response, "null");
}

fn assert_identity_principal_count(client: &GeneratedDaemonClient, handle: &str, expected: usize) {
    let payload = client.call("Identity", "identity_list", Vec::new());
    let ResponsePayload::Ok(value) = payload else {
        panic!("Identity.identity_list returned {payload:?}");
    };
    let count = identity_principal_count(&value, handle);
    assert_eq!(count, expected, "identity snapshot: {value:?}");
}

fn assert_generated_error_code(payload: ResponsePayload, expected: loom_core::Code) {
    match payload {
        ResponsePayload::Err(error) => {
            assert_eq!(error.code, expected, "generated error: {error:?}");
        }
        other => panic!("expected generated {expected:?} error, got {other:?}"),
    }
}

fn identity_principal_count(value: &loom_codec::Value, handle: &str) -> usize {
    match value {
        loom_codec::Value::Bytes(bytes) => loom_codec::decode(bytes)
            .map(|decoded| identity_principal_count(&decoded, handle))
            .unwrap_or(0),
        loom_codec::Value::Array(fields) => fields
            .get(5)
            .and_then(|value| match value {
                loom_codec::Value::Array(values) => Some(
                    values
                        .iter()
                        .filter(|principal| principal_has_handle(principal, handle))
                        .count(),
                ),
                _ => None,
            })
            .unwrap_or(0),
        loom_codec::Value::Map(fields) => fields
            .iter()
            .find_map(|(key, value)| {
                (key == &loom_codec::Value::Text("principals".to_string())).then_some(value)
            })
            .and_then(|value| match value {
                loom_codec::Value::Array(values) => Some(
                    values
                        .iter()
                        .filter(|principal| principal_has_handle(principal, handle))
                        .count(),
                ),
                _ => None,
            })
            .unwrap_or(0),
        _ => 0,
    }
}

fn principal_has_handle(value: &loom_codec::Value, handle: &str) -> bool {
    match value {
        loom_codec::Value::Array(fields) => {
            fields.get(1) == Some(&loom_codec::Value::Text(handle.to_string()))
        }
        loom_codec::Value::Map(fields) => fields.iter().any(|(key, value)| {
            key == &loom_codec::Value::Text("handle".to_string())
                && value == &loom_codec::Value::Text(handle.to_string())
        }),
        _ => false,
    }
}

fn unused_loopback_addr() -> SocketAddr {
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    addr
}

fn assert_listener_available(addr: SocketAddr) {
    TcpListener::bind(addr).unwrap_or_else(|error| {
        panic!("expected {addr} to be available for binding: {error}");
    });
}

fn hosted_http_request(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn stdio_mcp_clients_share_one_daemon_owned_store() {
    let mut store = DaemonStore::new("shared");
    store.init();
    store.start();

    assert!(
        FileStore::open(&store.path).is_err(),
        "a daemon-owned store must exclude a process-private writable FileStore"
    );

    let mut first = McpClient::spawn(&store.path);
    let mut second = McpClient::spawn(&store.path);

    let created_ticket = first.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "Shared daemon authority",
                "description": "created through the first long lived MCP client",
                "status": "ready",
                "priority": "P1"
            },
            "policy_labels": ["mx506"]
        }),
    );
    assert_contains(&created_ticket, "CORE-1");
    let ticket_id = find_string_key(&created_ticket, "ticket_id").unwrap();
    let stale_root = find_string_key(&created_ticket, "profile_root").unwrap();

    let read_ticket = second.call_tool(
        "tickets_get",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "detailed": true
        }),
    );
    assert_contains(&read_ticket, "Shared daemon authority");

    let updated_ticket = first.call_tool(
        "tickets_update",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "set_fields": {
                "status": "in_progress"
            },
            "expected_root": stale_root
        }),
    );
    assert_contains(&updated_ticket, "in_progress");

    second.call_tool_error(
        "tickets_update",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "set_fields": {
                "status": "blocked"
            },
            "expected_root": stale_root
        }),
    );

    let jira_ticket = first.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "projection": "jira",
            "fields": {
                "fields": {
                    "summary": "Projected daemon authority",
                    "description": "created through jira projection"
                }
            },
            "policy_labels": ["mx506"]
        }),
    );
    assert_contains(&jira_ticket, "Projected daemon authority");
    let jira_ticket_id = find_string_key(&jira_ticket, "ticket_id").unwrap();

    let jira_projected = second.call_tool(
        "tickets_get",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": jira_ticket_id,
            "projection": "jira",
            "detailed": true
        }),
    );
    assert_contains(&jira_projected, "\"projection\":\"jira\"");
    assert_contains(
        &jira_projected,
        "\"summary\":\"Projected daemon authority\"",
    );

    let jira_updated = first.call_tool(
        "tickets_update",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": jira_ticket_id,
            "projection": "jira",
            "set_fields": {
                "fields": {
                    "summary": "Projected daemon authority updated"
                }
            }
        }),
    );
    assert_contains(&jira_updated, "Projected daemon authority updated");

    let jira_after_update = second.call_tool(
        "tickets_get",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": jira_ticket_id,
            "projection": "jira",
            "detailed": true
        }),
    );
    assert_contains(&jira_after_update, "\"projection\":\"jira\"");
    assert_contains(
        &jira_after_update,
        "\"summary\":\"Projected daemon authority updated\"",
    );

    first.call_tool(
        "lanes_create",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "mx506-lane",
            "lane_key": "mx506-lane",
            "title": "MX-506 lane",
            "lane_kind": "assignment",
            "lane_status": "ready",
            "ticket_ids": [ticket_id],
            "status_report": "created through first MCP client"
        }),
    );
    let lane = second.call_tool(
        "lanes_get",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "mx506-lane",
            "detailed": true
        }),
    );
    assert_contains(&lane, "Shared daemon authority");

    first.call_tool(
        "spaces_create",
        json!({
            "workspace": WORKSPACE,
            "space_id": "mx506-space",
            "title": "MX-506 space"
        }),
    );
    first.call_tool(
        "pages_create",
        json!({
            "workspace": WORKSPACE,
            "space_id": "mx506-space",
            "page_id": "mx506-page",
            "title": "MX-506 page"
        }),
    );
    first.call_tool(
        "pages_update",
        json!({
            "workspace": WORKSPACE,
            "page_id": "mx506-page",
            "body_text": "visible to the second MCP client"
        }),
    );
    let page = second.call_tool(
        "pages_get",
        json!({
            "workspace": WORKSPACE,
            "page_id": "mx506-page"
        }),
    );
    assert_contains(&page, "visible to the second MCP client");

    first.call_tool(
        "document_put_text",
        json!({
            "workspace": WORKSPACE,
            "collection": "mx506-docs",
            "id": "shared",
            "text": "document written through first MCP client"
        }),
    );
    let document = second.call_tool(
        "document_get_text",
        json!({
            "workspace": WORKSPACE,
            "collection": "mx506-docs",
            "id": "shared"
        }),
    );
    assert_contains(&document, "document written through first MCP client");

    store.stop();
    let stopped = first.request_after_daemon_stop();
    assert!(
        stopped.as_ref().is_err_and(|stderr| !stderr.is_empty())
            || stopped.as_ref().is_ok_and(response_is_error),
        "long lived MCP client unexpectedly kept writable daemon access after stop: {stopped:?}"
    );

    let mut restarted = McpClient::spawn(&store.path);
    let status = store.status();
    assert!(
        status.contains("running"),
        "new MCP launch did not restart the daemon: {status}"
    );
    let after_restart = restarted.call_tool(
        "document_get_text",
        json!({
            "workspace": WORKSPACE,
            "collection": "mx506-docs",
            "id": "shared"
        }),
    );
    assert_contains(&after_restart, "document written through first MCP client");
}

#[test]
fn rec3_cli_daemon_and_mcp_share_one_coherent_persistent_engine() {
    let mut store = DaemonStore::new("rec3-coherent-engine");
    store.init();
    let root = store.configure_identity_store();
    let passphrase_path = temp_text_path("rec3-root-passphrase", "root-pass");
    let document_v2 = temp_text_path("rec3-document-v2", "REC-3 document v2\n");
    store.start();
    wait_for_status(&store, "running\t");

    assert!(
        FileStore::open(&store.path).is_err(),
        "the running daemon must exclude an independent writable FileStore"
    );

    let mut mcp = McpClient::spawn_with_auth(&store.path, root, "root-pass");
    let ticket_create = loom_with_auth(
        root,
        &passphrase_path,
        [
            "tickets",
            "create",
            &store.path,
            WORKSPACE,
            "task",
            "--project-id",
            PROJECT_ID,
            "--title",
            "REC-3 coherent ticket",
            "--description",
            "created through generated CLI",
            "--format",
            "json",
        ],
    )
    .unwrap();
    let ticket_create: Value = serde_json::from_str(&ticket_create).unwrap();
    let ticket_id = find_string_key(&ticket_create, "ticket_id").expect("ticket id");
    let ticket_root = find_string_key(&ticket_create, "root_after")
        .or_else(|| find_string_key(&ticket_create, "new_root"))
        .or_else(|| find_string_key(&ticket_create, "profile_root"))
        .expect("ticket compare root");

    let ticket_from_mcp = mcp.call_tool(
        "tickets_get",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "detailed": true
        }),
    );
    assert_contains(&ticket_from_mcp, "created through generated CLI");

    let ticket_update = mcp.call_tool(
        "tickets_update",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "set_fields": {
                "status": "in_progress",
                "description": "updated through MCP"
            },
            "expected_root": ticket_root
        }),
    );
    assert_contains(&ticket_update, "updated through MCP");

    let ticket_from_cli = loom_with_auth(
        root,
        &passphrase_path,
        [
            "tickets",
            "get",
            &store.path,
            WORKSPACE,
            &ticket_id,
            "--detailed",
            "--format",
            "json",
        ],
    )
    .unwrap();
    assert!(ticket_from_cli.contains("updated through MCP"));
    assert!(ticket_from_cli.contains("in_progress"));

    let stale_ticket_update = loom_with_auth(
        root,
        &passphrase_path,
        [
            "tickets",
            "update",
            &store.path,
            WORKSPACE,
            &ticket_id,
            "--status",
            "blocked",
            "--expected-root",
            &ticket_root,
            "--format",
            "json",
        ],
    );
    assert!(
        stale_ticket_update
            .as_ref()
            .is_err_and(|error| error.to_ascii_uppercase().contains("CONFLICT")),
        "stale ticket update must fail with conflict: {stale_ticket_update:?}"
    );
    let ticket_after_rejection = mcp.call_tool(
        "tickets_get",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "detailed": true
        }),
    );
    assert_contains(&ticket_after_rejection, "updated through MCP");
    assert_eq!(
        ticket_after_rejection.pointer("/result/structuredContent/value/fields/status"),
        Some(&Value::String("in_progress".to_string()))
    );

    mcp.call_tool(
        "lanes_create",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "rec3-lane",
            "lane_key": "rec3-lane",
            "title": "REC-3 lane",
            "description": "created through MCP",
            "lane_kind": "assignment",
            "lane_status": "ready"
        }),
    );
    let lane_from_cli = loom_with_auth(
        root,
        &passphrase_path,
        [
            "lanes",
            "get",
            &store.path,
            WORKSPACE,
            "rec3-lane",
            "--detailed",
            "--format",
            "json",
        ],
    )
    .unwrap();
    assert!(lane_from_cli.contains("created through MCP"));

    loom_with_auth(
        root,
        &passphrase_path,
        [
            "lanes",
            "update",
            &store.path,
            WORKSPACE,
            "rec3-lane",
            "--lane-status",
            "working",
            "--status-report",
            "updated through generated CLI",
            "--format",
            "json",
        ],
    )
    .unwrap();
    let lane_from_mcp = mcp.call_tool(
        "lanes_get",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "rec3-lane",
            "detailed": true
        }),
    );
    assert_contains(&lane_from_mcp, "updated through generated CLI");
    assert_contains(&lane_from_mcp, "working");

    mcp.call_tool(
        "lanes_ticket_add",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "rec3-lane",
            "ticket_id": ticket_id
        }),
    );
    let lane_with_ticket = loom_with_auth(
        root,
        &passphrase_path,
        [
            "lanes",
            "get",
            &store.path,
            WORKSPACE,
            "rec3-lane",
            "--detailed",
            "--format",
            "json",
        ],
    )
    .unwrap();
    assert!(lane_with_ticket.contains(&ticket_id));
    assert!(lane_with_ticket.contains("REC-3 coherent ticket"));

    loom_with_auth(
        root,
        &passphrase_path,
        [
            "pages",
            "space-create",
            &store.path,
            WORKSPACE,
            "rec3-space",
            "REC-3 space",
            "--format",
            "json",
        ],
    )
    .unwrap();
    let space_from_mcp = mcp.call_tool(
        "spaces_get",
        json!({
            "workspace": WORKSPACE,
            "space_id": "rec3-space"
        }),
    );
    assert_contains(&space_from_mcp, "REC-3 space");

    mcp.call_tool(
        "pages_create",
        json!({
            "workspace": WORKSPACE,
            "space_id": "rec3-space",
            "page_id": "rec3-page",
            "title": "REC-3 page"
        }),
    );
    let page_from_cli = loom_with_auth(
        root,
        &passphrase_path,
        [
            "pages",
            "get",
            &store.path,
            WORKSPACE,
            "rec3-page",
            "--format",
            "json",
        ],
    )
    .unwrap();
    assert!(page_from_cli.contains("REC-3 page"));

    loom_with_auth(
        root,
        &passphrase_path,
        [
            "pages",
            "update",
            &store.path,
            WORKSPACE,
            "rec3-page",
            "REC-3 page body from generated CLI",
            "--format",
            "json",
        ],
    )
    .unwrap();
    let page_from_mcp = mcp.call_tool(
        "pages_get",
        json!({
            "workspace": WORKSPACE,
            "page_id": "rec3-page"
        }),
    );
    assert_contains(&page_from_mcp, "REC-3 page body from generated CLI");

    let document_put = mcp.call_tool(
        "document_put_text",
        json!({
            "workspace": WORKSPACE,
            "collection": "rec3-docs",
            "id": "shared",
            "text": "REC-3 document v1\n"
        }),
    );
    let document_tag = find_string_key(&document_put, "entity_tag").expect("document entity tag");
    let document_from_cli = loom_with_auth(
        root,
        &passphrase_path,
        [
            "document",
            "get-text",
            &store.path,
            WORKSPACE,
            "rec3-docs",
            "shared",
        ],
    )
    .unwrap();
    assert_eq!(document_from_cli, "REC-3 document v1\n");

    loom_with_auth(
        root,
        &passphrase_path,
        [
            "document",
            "put-text",
            &store.path,
            WORKSPACE,
            "rec3-docs",
            "shared",
            document_v2.to_str().unwrap(),
            "--expected-entity-tag",
            &document_tag,
        ],
    )
    .unwrap();
    let document_from_mcp = mcp.call_tool(
        "document_get_text",
        json!({
            "workspace": WORKSPACE,
            "collection": "rec3-docs",
            "id": "shared"
        }),
    );
    assert_contains(&document_from_mcp, "REC-3 document v2");

    let stale_document_put = mcp.call_tool_error(
        "document_put_text",
        json!({
            "workspace": WORKSPACE,
            "collection": "rec3-docs",
            "id": "shared",
            "text": "REC-3 stale document",
            "expected_entity_tag": document_tag
        }),
    );
    assert_error_contains(&stale_document_put, "CONFLICT");
    let document_after_rejection = loom_with_auth(
        root,
        &passphrase_path,
        [
            "document",
            "get-text",
            &store.path,
            WORKSPACE,
            "rec3-docs",
            "shared",
        ],
    )
    .unwrap();
    assert_eq!(document_after_rejection, "REC-3 document v2\n");

    drop(mcp);
    store.stop_with_auth(root, "root-pass");
    wait_for_stopped(&store);
    store.start();
    wait_for_status(&store, "running\t");
    assert!(
        FileStore::open(&store.path).is_err(),
        "the restarted daemon must remain the only writable store owner"
    );

    let mut restarted_mcp = McpClient::spawn_with_auth(&store.path, root, "root-pass");
    let restarted_ticket = restarted_mcp.call_tool(
        "tickets_get",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "detailed": true
        }),
    );
    assert_contains(&restarted_ticket, "updated through MCP");
    assert_eq!(
        restarted_ticket.pointer("/result/structuredContent/value/fields/status"),
        Some(&Value::String("in_progress".to_string()))
    );

    let restarted_lane = loom_with_auth(
        root,
        &passphrase_path,
        [
            "lanes",
            "get",
            &store.path,
            WORKSPACE,
            "rec3-lane",
            "--detailed",
            "--format",
            "json",
        ],
    )
    .unwrap();
    assert!(restarted_lane.contains("updated through generated CLI"));
    assert!(restarted_lane.contains(&ticket_id));

    let restarted_page = restarted_mcp.call_tool(
        "pages_get",
        json!({
            "workspace": WORKSPACE,
            "page_id": "rec3-page"
        }),
    );
    assert_contains(&restarted_page, "REC-3 page body from generated CLI");

    let restarted_document = loom_with_auth(
        root,
        &passphrase_path,
        [
            "document",
            "get-text",
            &store.path,
            WORKSPACE,
            "rec3-docs",
            "shared",
        ],
    )
    .unwrap();
    assert_eq!(restarted_document, "REC-3 document v2\n");

    drop(restarted_mcp);
    store.stop_with_auth(root, "root-pass");
    wait_for_stopped(&store);
    for path in [passphrase_path, document_v2] {
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn mu17b_generated_daemon_mutation_is_visible_to_separate_local_mcp_session() {
    let mut store = DaemonStore::new("mu17b-generated-to-mcp");
    store.init();
    store.seed_graph_edge("mu17b-a", "edge-a");
    store.start();

    let generated = GeneratedDaemonClient::connect(&store.path);
    assert_generated_edge_present(&generated, "mu17b-a", "edge-a");
    let removed = generated.call(
        "Graph",
        "remove_edge",
        vec![
            loom_codec::Value::Text(WORKSPACE.to_string()),
            loom_codec::Value::Text("mu17b-a".to_string()),
            loom_codec::Value::Text("edge-a".to_string()),
        ],
    );
    assert_eq!(removed, ResponsePayload::Ok(loom_codec::Value::Bool(true)));

    let mut mcp = McpClient::spawn(&store.path);
    assert_mcp_graph_edge_absent(&mut mcp, "mu17b-a", "edge-a");

    store.stop();
}

#[test]
fn mu17b_local_mcp_mutation_is_visible_to_separate_generated_daemon_session() {
    let mut store = DaemonStore::new("mu17b-mcp-to-generated");
    store.init();
    store.seed_graph_edge("mu17b-b", "edge-b");
    store.start();

    let mut mcp = McpClient::spawn(&store.path);
    let removed = mcp.call_tool(
        "graph_remove_edge",
        json!({
            "workspace": WORKSPACE,
            "collection": "mu17b-b",
            "id": "edge-b"
        }),
    );
    assert_contains(&removed, "true");

    let generated = GeneratedDaemonClient::connect(&store.path);
    assert_generated_edge_absent(&generated, "mu17b-b", "edge-b");

    store.stop();
}

#[test]
fn mu17b_generated_reads_reconnect_after_daemon_restart_and_see_external_commit() {
    let mut store = DaemonStore::new("mu17b-reconnect");
    store.init();
    store.seed_graph_edge("mu17b-c", "before-restart");
    store.start();

    let first = GeneratedDaemonClient::connect(&store.path);
    assert_generated_edge_present(&first, "mu17b-c", "before-restart");
    drop(first);
    store.stop();
    wait_for_stopped(&store);

    store.seed_graph_edge("mu17b-c", "after-restart");
    store.start();
    let second = GeneratedDaemonClient::connect(&store.path);
    assert_generated_edge_present(&second, "mu17b-c", "after-restart");

    store.stop();
}

#[test]
fn mu17b_transport_loss_during_non_idempotent_generated_mutation_is_ambiguous_without_replay() {
    let mut store = DaemonStore::new("mu17b-non-idempotent-loss");
    store.init();
    store.seed_graph_edge("mu17b-d", "edge-d");
    store.start();

    let generated = GeneratedDaemonClient::connect(&store.path);
    assert_generated_edge_present(&generated, "mu17b-d", "edge-d");
    let (loss, dropped) = call_generated_through_response_drop_proxy(
        &generated.paths,
        Some(generated.session_id.clone()),
        "Graph",
        "remove_edge",
        vec![
            generated.handle.to_value(),
            loom_codec::Value::Text(WORKSPACE.to_string()),
            loom_codec::Value::Text("mu17b-d".to_string()),
            loom_codec::Value::Text("edge-d".to_string()),
        ],
    );
    let error = loss.expect_err("transport loss must fail the production generated call path");
    assert!(
        error.contains("daemon generated response has an unexpected frame prefix"),
        "{error}"
    );
    assert_eq!(dropped.forwarded_request.interface, "Graph");
    assert_eq!(dropped.forwarded_request.method, "remove_edge");
    assert!(dropped.response_bytes > 0);

    let observer = GeneratedDaemonClient::connect(&store.path);
    let second_remove = observer.call(
        "Graph",
        "remove_edge",
        vec![
            loom_codec::Value::Text(WORKSPACE.to_string()),
            loom_codec::Value::Text("mu17b-d".to_string()),
            loom_codec::Value::Text("edge-d".to_string()),
        ],
    );
    assert_eq!(
        second_remove,
        ResponsePayload::Ok(loom_codec::Value::Bool(false))
    );

    store.stop();
}

#[test]
fn mu17b_transport_loss_during_idempotent_generated_mutation_replays_once_by_key() {
    let mut store = DaemonStore::new("mu17b-idempotent-loss");
    store.init();
    let root = store.configure_identity_store();
    store.start();

    let generated = GeneratedDaemonClient::connect(&store.path);
    assert_identity_principal_count(&generated, "mu17b-service", 0);
    let idempotency_key = b"mu17b-idempotent-principal".to_vec();
    let dropped = send_generated_call_and_drop_response(
        &generated.paths,
        Some(generated.session_id.clone()),
        "Identity",
        "identity_add_principal",
        vec![
            generated.handle.to_value(),
            loom_codec::Value::Text("mu17b-service".to_string()),
            loom_codec::Value::Text("MU17B Service".to_string()),
            loom_codec::Value::Bytes(vec![2]),
        ],
        Some(idempotency_key.clone()),
    );
    assert!(dropped.is_ok(), "drop response setup failed: {dropped:?}");

    let replay = generated.call_with_idempotency(
        "Identity",
        "identity_add_principal",
        vec![
            loom_codec::Value::Text("mu17b-service".to_string()),
            loom_codec::Value::Text("MU17B Service".to_string()),
            loom_codec::Value::Bytes(vec![2]),
        ],
        Some(idempotency_key),
    );
    assert!(
        matches!(replay, ResponsePayload::Ok(loom_codec::Value::Bytes(_))),
        "expected replayed principal UUID, got {replay:?}"
    );
    assert_identity_principal_count(&generated, "mu17b-service", 1);

    store.stop_with_auth(root, "root-pass");
}

#[test]
fn mu17c_tickets_behave_through_local_mcp_daemon() {
    let mut store = DaemonStore::new("mu17c-tickets");
    store.init();
    store.start();

    let mut mcp = McpClient::spawn(&store.path);
    let dependency = mcp.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MU-17c dependency",
                "status": "ready"
            }
        }),
    );
    let dependency_id = find_string_key(&dependency, "ticket_id").unwrap();

    let created = mcp.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MU-17c ticket",
                "description": "created through local MCP daemon",
                "status": "ready",
                "priority": "P2"
            },
            "policy_labels": ["mu17c"]
        }),
    );
    assert_contains(&created, "MU-17c ticket");
    let ticket_id = find_string_key(&created, "ticket_id").unwrap();

    let get_created = mcp.call_tool(
        "tickets_get",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "detailed": true
        }),
    );
    assert_contains(&get_created, "created through local MCP daemon");

    let updated = mcp.call_tool(
        "tickets_update",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "set_fields": {
                "status": "in_progress",
                "description": "updated through local MCP daemon"
            }
        }),
    );
    assert_contains(&updated, "in_progress");

    let listed = mcp.call_tool(
        "tickets_list",
        json!({
            "workspace": WORKSPACE,
            "limit": 10
        }),
    );
    assert_contains(&listed, "MU-17c ticket");

    let comment = mcp.call_tool(
        "tickets_update",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "comments": [{
                "comment_id": "mu17c-comment-1",
                "comment_type": "review_feedback",
                "body": "MU-17c comment through local MCP daemon"
            }]
        }),
    );
    assert_contains(&comment, "review_feedback");
    let comments = mcp.call_tool(
        "tickets_get",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "detailed": true
        }),
    );
    assert_contains(&comments, "mu17c-comment-1");
    assert_contains(&comments, "review_feedback");

    let relation = mcp.call_tool(
        "tickets_update",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "relation_sets": [{
                "relation_id": "mu17c-depends-on",
                "kind": "depends_on",
                "target_id": dependency_id
            }]
        }),
    );
    assert_contains(&relation, "depends_on");
    let relations = mcp.call_tool(
        "tickets_get",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "detailed": true
        }),
    );
    assert_contains(&relations, "depends_on");
    assert_contains(&relations, &dependency_id);

    let history = mcp.call_tool(
        "tickets_history",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "detailed": true,
            "limit": 20
        }),
    );
    assert_contains(&history, "ticket.created");
    assert_contains(&history, "ticket.transitioned");
    assert_contains(&history, "ticket.comment_added");
    assert_contains(&history, "ticket.relations_updated");

    store.stop();
}

#[test]
fn mu17c_jira_ticket_list_preserves_public_shape_through_local_mcp_daemon() {
    let mut store = DaemonStore::new("mu17c-jira-list");
    store.init();
    store.start();

    let mut mcp = McpClient::spawn(&store.path);
    let created = mcp.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MU-17c Jira list shape",
                "status": "ready"
            }
        }),
    );
    assert_contains(&created, "MU-17c Jira list shape");
    let listed = mcp.call_tool(
        "tickets_list",
        json!({
            "workspace": WORKSPACE,
            "projection": "jira",
            "limit": 10
        }),
    );
    assert_contains(&listed, "\"projection\":\"jira\"");
    assert_contains(&listed, "\"summary\":\"MU-17c Jira list shape\"");
    assert_not_contains(&listed, "projection_kind");
    assert_not_contains(&listed, "projection_source");
    assert_not_contains(&listed, "projection_selection_source");

    store.stop();
}

#[test]
fn mu17c_lane_filtered_ticket_list_uses_original_lane_selector_through_local_mcp_daemon() {
    let mut store = DaemonStore::new("mu17c-lane-filter");
    store.init();
    store.start();

    let mut mcp = McpClient::spawn(&store.path);
    let inside = mcp.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MU-17c inside lane",
                "status": "ready"
            }
        }),
    );
    let inside_id = find_string_key(&inside, "ticket_id").unwrap();
    let outside = mcp.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MU-17c outside lane",
                "status": "ready"
            }
        }),
    );
    let outside_id = find_string_key(&outside, "ticket_id").unwrap();
    mcp.call_tool(
        "lanes_create",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "mu17c-filter-lane",
            "lane_key": "mu17c-filter-lane",
            "title": "MU-17c filter lane",
            "lane_kind": "assignment",
            "lane_status": "ready",
            "ticket_ids": [inside_id]
        }),
    );

    let listed = mcp.call_tool(
        "tickets_list",
        json!({
            "workspace": WORKSPACE,
            "lane": "mu17c-filter-lane",
            "limit": 10
        }),
    );
    assert_contains(&listed, "MU-17c inside lane");
    assert_not_contains(&listed, "MU-17c outside lane");
    assert_not_contains(&listed, &outside_id);

    store.stop();
}

#[test]
fn mu17c_remote_lane_list_status_warnings_return_matching_diagnostics() {
    let mut store = DaemonStore::new("mu17c-lane-diagnostics");
    store.init();
    store.start();

    let mut mcp = McpClient::spawn(&store.path);
    let ticket = mcp.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MU-17c diagnostic ticket",
                "status": "ready"
            }
        }),
    );
    let ticket_id = find_string_key(&ticket, "ticket_id").unwrap();
    mcp.call_tool(
        "lanes_create",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "mu17c-diagnostic-lane",
            "lane_key": "mu17c-diagnostic-lane",
            "title": "MU-17c diagnostic lane",
            "lane_kind": "assignment",
            "lane_status": "ready",
            "ticket_ids": [ticket_id],
            "active_ticket_id": ticket_id,
            "status_report": "blocked by missing closeout comment"
        }),
    );

    let listed = mcp.call_tool(
        "lanes_list",
        json!({
            "workspace": WORKSPACE,
            "detailed": true,
            "limit": 10
        }),
    );
    assert_contains(&listed, "\"lane_id\":\"mu17c-diagnostic-lane\"");
    assert_contains(&listed, "\"status_warnings\"");
    assert_contains(&listed, "\"diagnostics\"");
    assert_contains(&listed, "status_report says blocked");
    assert_contains(&listed, "\"error\":\"status_report says blocked");

    store.stop();
}

#[test]
fn mu17c_lanes_behave_through_local_mcp_daemon() {
    let mut store = DaemonStore::new("mu17c-lanes");
    store.init();
    store.start();

    let mut mcp = McpClient::spawn(&store.path);
    let ticket = mcp.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MU-17c lane ticket",
                "status": "ready"
            }
        }),
    );
    let ticket_id = find_string_key(&ticket, "ticket_id").unwrap();

    let created = mcp.call_tool(
        "lanes_create",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "mu17c-lane",
            "lane_key": "mu17c-lane",
            "title": "MU-17c lane",
            "description": "created through local MCP daemon",
            "lane_kind": "assignment",
            "lane_status": "ready",
            "status_report": "empty lane"
        }),
    );
    assert_contains(&created, "MU-17c lane");

    let get_created = mcp.call_tool(
        "lanes_get",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "mu17c-lane",
            "detailed": true
        }),
    );
    assert_contains(&get_created, "empty lane");

    let listed = mcp.call_tool(
        "lanes_list",
        json!({
            "workspace": WORKSPACE,
            "detailed": true,
            "limit": 10
        }),
    );
    assert_contains(&listed, "mu17c-lane");

    let updated = mcp.call_tool(
        "lanes_update",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "mu17c-lane",
            "title": "MU-17c updated lane",
            "status_report": "updated lane status"
        }),
    );
    assert_contains(&updated, "updated lane status");

    let added = mcp.call_tool(
        "lanes_ticket_add",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "mu17c-lane",
            "ticket_id": ticket_id
        }),
    );
    assert_contains(&added, &ticket_id);
    let with_member = mcp.call_tool(
        "lanes_get",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "mu17c-lane",
            "detailed": true
        }),
    );
    assert_contains(&with_member, &ticket_id);
    assert_contains(&with_member, "MU-17c lane ticket");

    let removed = mcp.call_tool(
        "lanes_ticket_remove",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "mu17c-lane",
            "ticket_id": ticket_id
        }),
    );
    assert_contains(&removed, "updated lane status");
    let without_member = mcp.call_tool(
        "lanes_get",
        json!({
            "workspace": WORKSPACE,
            "lane_id": "mu17c-lane",
            "detailed": true
        }),
    );
    assert_not_contains(&without_member, &ticket_id);

    store.stop();
}

#[test]
fn mu17c_pages_behave_through_local_mcp_daemon() {
    let mut store = DaemonStore::new("mu17c-pages");
    store.init();
    store.start();

    let mut mcp = McpClient::spawn(&store.path);
    let space = mcp.call_tool(
        "spaces_create",
        json!({
            "workspace": WORKSPACE,
            "space_id": "mu17c-space",
            "title": "MU-17c space"
        }),
    );
    assert_contains(&space, "MU-17c space");
    let got_space = mcp.call_tool(
        "spaces_get",
        json!({
            "workspace": WORKSPACE,
            "space_id": "mu17c-space"
        }),
    );
    assert_contains(&got_space, "MU-17c space");

    let page = mcp.call_tool(
        "pages_create",
        json!({
            "workspace": WORKSPACE,
            "space_id": "mu17c-space",
            "page_id": "mu17c-page",
            "title": "MU-17c page"
        }),
    );
    assert_contains(&page, "MU-17c page");
    let got_page = mcp.call_tool(
        "pages_get",
        json!({
            "workspace": WORKSPACE,
            "page_id": "mu17c-page"
        }),
    );
    assert_contains(&got_page, "MU-17c page");

    let updated = mcp.call_tool(
        "pages_update",
        json!({
            "workspace": WORKSPACE,
            "page_id": "mu17c-page",
            "body_text": "MU-17c draft body"
        }),
    );
    assert_contains(&updated, "draft");
    let draft = mcp.call_tool(
        "pages_get",
        json!({
            "workspace": WORKSPACE,
            "page_id": "mu17c-page"
        }),
    );
    assert_contains(&draft, "MU-17c draft body");

    let published = mcp.call_tool(
        "pages_publish",
        json!({
            "workspace": WORKSPACE,
            "page_id": "mu17c-page"
        }),
    );
    assert_contains(&published, "published");
    let read_published = mcp.call_tool(
        "pages_get",
        json!({
            "workspace": WORKSPACE,
            "page_id": "mu17c-page"
        }),
    );
    assert_contains(&read_published, "published");
    assert_contains(&read_published, "MU-17c draft body");

    store.stop();
}

#[test]
fn mu17c_documents_behave_through_local_mcp_daemon() {
    let mut store = DaemonStore::new("mu17c-documents");
    store.init();
    store.start();

    let mut mcp = McpClient::spawn(&store.path);
    let text_put = mcp.call_tool(
        "document_put_text",
        json!({
            "workspace": WORKSPACE,
            "collection": "mu17c-docs",
            "id": "text",
            "text": "MU-17c text document"
        }),
    );
    assert_contains(&text_put, "entity_tag");
    let text = mcp.call_tool(
        "document_get_text",
        json!({
            "workspace": WORKSPACE,
            "collection": "mu17c-docs",
            "id": "text"
        }),
    );
    assert_contains(&text, "MU-17c text document");

    let binary_put = mcp.call_tool(
        "document_put_binary",
        json!({
            "workspace": WORKSPACE,
            "collection": "mu17c-docs",
            "id": "binary",
            "bytes": [255, 0, 97]
        }),
    );
    assert_contains(&binary_put, "entity_tag");
    let binary = mcp.call_tool(
        "document_get_binary",
        json!({
            "workspace": WORKSPACE,
            "collection": "mu17c-docs",
            "id": "binary"
        }),
    );
    assert_contains(&binary, "[255,0,97]");

    let query = mcp.call_tool(
        "document_query",
        json!({
            "workspace": WORKSPACE,
            "collection": "mu17c-docs",
            "id_prefix": "",
            "limit": 10
        }),
    );
    assert_contains(&query, "\"id\":\"binary\"");
    assert_contains(&query, "\"id\":\"text\"");

    let not_text = mcp.call_tool_error(
        "document_get_text",
        json!({
            "workspace": WORKSPACE,
            "collection": "mu17c-docs",
            "id": "binary"
        }),
    );
    assert_contains(&not_text, "DOCUMENT_NOT_TEXT");

    store.stop();
}

#[test]
fn mu17d_local_mcp_authentication_and_authorization_errors_are_stable() {
    let mut store = DaemonStore::new("mu17d-auth");
    store.init();
    let (root, reader) = store.configure_identity_store_with_reader();
    store.start();

    let mut unauthenticated = McpClient::spawn(&store.path);
    let unauthenticated_error = unauthenticated.call_tool_error(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MU-17d unauthenticated write",
                "status": "ready"
            }
        }),
    );
    assert_error_contains(&unauthenticated_error, "AUTHENTICATION_FAILED");

    let mut root_client = McpClient::spawn_with_auth(&store.path, root, "root-pass");
    let created = root_client.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MU-17d authenticated write",
                "status": "ready"
            }
        }),
    );
    assert_contains(&created, "MU-17d authenticated write");

    let mut reader_client = McpClient::spawn_with_auth(&store.path, reader, "reader-pass");
    let denied = reader_client.call_tool_error(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MU-17d denied write",
                "status": "ready"
            }
        }),
    );
    assert_error_contains(&denied, "PERMISSION_DENIED");

    store.stop_with_auth(root, "root-pass");
}

#[test]
fn mu17d_local_mcp_and_generated_daemon_stable_errors_are_preserved() {
    let mut store = DaemonStore::new("mu17d-stable-errors");
    store.init();
    store.seed_graph_edge("mu17d", "edge");
    store.start();

    let mut mcp = McpClient::spawn(&store.path);
    let created = mcp.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MU-17d conflict target",
                "status": "ready"
            }
        }),
    );
    let ticket_id = find_string_key(&created, "ticket_id").unwrap();
    let stale_root = find_string_key(&created, "profile_root").unwrap();
    mcp.call_tool(
        "tickets_update",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "set_fields": {
                "status": "in_progress"
            },
            "expected_root": stale_root
        }),
    );
    let conflict = mcp.call_tool_error(
        "tickets_update",
        json!({
            "workspace": WORKSPACE,
            "ticket_id": ticket_id,
            "set_fields": {
                "status": "blocked"
            },
            "expected_root": stale_root
        }),
    );
    assert_error_contains(&conflict, "CONFLICT");

    let generated = GeneratedDaemonClient::connect(&store.path);
    let unknown_session = generated_call(
        &generated.paths,
        Some(vec![0; 16]),
        "Store",
        "open",
        Vec::new(),
        None,
    )
    .unwrap();
    assert_generated_error_code(unknown_session, loom_core::Code::NotFound);

    let unknown_method = generated_call(
        &generated.paths,
        Some(generated.session_id.clone()),
        "Store",
        "missing_method",
        Vec::new(),
        None,
    )
    .unwrap();
    assert_generated_error_code(unknown_method, loom_core::Code::NotFound);

    let invalid_generated = generated.call(
        "Graph",
        "get_edge",
        vec![loom_codec::Value::Text(WORKSPACE.to_string())],
    );
    assert_generated_error_code(invalid_generated, loom_core::Code::InvalidArgument);

    let first = generated_call(
        &generated.paths,
        Some(generated.session_id.clone()),
        "Store",
        "blob_digest",
        vec![loom_codec::Value::Bytes(b"first".to_vec())],
        Some(b"mu17d-idempotency".to_vec()),
    )
    .unwrap();
    assert!(matches!(first, ResponsePayload::Ok(_)), "{first:?}");
    let idempotency_conflict = generated_call(
        &generated.paths,
        Some(generated.session_id.clone()),
        "Store",
        "blob_digest",
        vec![loom_codec::Value::Bytes(b"second".to_vec())],
        Some(b"mu17d-idempotency".to_vec()),
    )
    .unwrap();
    assert_generated_error_code(idempotency_conflict, loom_core::Code::Conflict);

    let malformed = call_generated_through_malformed_response_proxy(
        &generated.paths,
        Some(generated.session_id.clone()),
        "Store",
        "version",
        Vec::new(),
    )
    .expect_err("malformed daemon response must fail closed");
    assert!(
        malformed.contains("unexpected frame prefix"),
        "unexpected malformed response error: {malformed}"
    );

    store.stop();
}

#[test]
fn mu17g_managed_mcp_suppresses_hosted_listener_until_promotion() {
    let mut store = DaemonStore::new("mu17g-managed-promotion");
    store.init();
    let addr = unused_loopback_addr();
    store.configure_cas_listener(addr);

    let mut client = McpClient::spawn(&store.path);
    let managed = wait_for_status(&store, "startup_mode=managed");
    assert!(
        managed.contains("startup_initiator=cli.mcp.local"),
        "MCP startup did not record its initiating surface: {managed}"
    );
    assert_listener_available(addr);

    let promoted = loom(["daemon", "start", &store.path, "--transport", "tcp"])
        .unwrap_or_else(|error| panic!("promotion failed with status {}: {error}", store.status()));
    assert!(
        promoted.starts_with("running\t")
            || promoted.starts_with("promoted\t")
            || promoted.starts_with("persistent\t"),
        "explicit daemon start did not promote the managed daemon: {promoted}"
    );
    let persistent = wait_for_status(&store, "startup_mode=persistent");
    assert!(
        persistent.contains("startup_initiator=cli.daemon.start"),
        "promotion did not update startup initiator: {persistent}"
    );
    let request = "PUT /cas HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 5\r\n\r\nalpha";
    let deadline = Instant::now() + Duration::from_secs(5);
    let put = loop {
        if let Ok(mut stream) = TcpStream::connect(addr) {
            stream.write_all(request.as_bytes()).unwrap();
            stream.flush().unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            if !response.is_empty() || Instant::now() >= deadline {
                break response;
            }
        }
        assert!(
            Instant::now() < deadline,
            "promoted listener did not respond"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        put.starts_with("HTTP/1.1 201 Created"),
        "promoted listener did not use daemon-owned write authority: {put}; {}; stderr={}",
        store.status(),
        std::fs::read_to_string(
            daemon::paths(&store.path)
                .unwrap()
                .lock_file
                .with_extension("stderr.log")
        )
        .unwrap_or_default()
    );
    let digest = put
        .split("\"digest\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("promoted CAS response digest");
    let get = hosted_http_request(
        addr,
        &format!("GET /cas/{digest} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    );
    assert!(get.starts_with("HTTP/1.1 200 OK"), "{get}");
    assert!(get.ends_with("alpha"), "{get}");

    let repeated = loom(["daemon", "start", &store.path, "--transport", "tcp"]).unwrap();
    assert!(
        repeated.starts_with("running\t") || repeated.starts_with("persistent\t"),
        "repeated explicit start was not idempotent: {repeated}"
    );
    let after_repeated = store.status();
    assert!(
        after_repeated.contains("startup_mode=persistent"),
        "repeated start changed persistent mode: {after_repeated}"
    );
    let _ = client.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MCP continuity after promotion",
                "status": "ready"
            }
        }),
    );

    store.stop();
}

#[test]
fn mu17g_managed_mcp_and_cli_leases_control_process_lifetime() {
    let store = DaemonStore::new("mu17g-managed-leases");
    store.init();

    let mut client = McpClient::spawn_with_env(
        &store.path,
        [("ULDREN_LOOM_DAEMON_LIFECYCLE_LEASE_MS", "3000")],
    );
    wait_for_status(&store, "startup_mode=managed");
    let mut cli = spawn_holding_cli(&store, 6500);

    let list = client.call_tool(
        "tickets_create",
        json!({
            "workspace": WORKSPACE,
            "project_id": PROJECT_ID,
            "ticket_type": "task",
            "fields": {
                "title": "MCP during CLI lease",
                "status": "ready"
            }
        }),
    );
    assert!(
        !response_is_error(&list),
        "MCP was not usable while CLI lease was active: {list:?}"
    );

    drop(client);
    std::thread::sleep(Duration::from_millis(4000));
    let while_cli_live = store.status();
    assert!(
        while_cli_live.starts_with("running\t"),
        "managed daemon exited while long-running CLI lease was live: {while_cli_live}"
    );

    let cli_status = cli.wait().unwrap();
    assert!(cli_status.success(), "holding CLI failed: {cli_status}");
    wait_for_stopped(&store);

    let restarted = store.status();
    assert!(
        restarted.starts_with("stopped\t"),
        "crashed MCP lease did not expire and final lease did not shut down managed daemon: {restarted}"
    );
}

fn response_is_error(value: &Value) -> bool {
    value.get("error").is_some()
        || value
            .pointer("/result/isError")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn assert_contains(value: &Value, needle: &str) {
    let haystack = value.to_string();
    assert!(
        haystack.contains(needle),
        "expected response to contain {needle:?}: {value:?}"
    );
}

fn assert_not_contains(value: &Value, needle: &str) {
    let haystack = value.to_string();
    assert!(
        !haystack.contains(needle),
        "expected response not to contain {needle:?}: {value:?}"
    );
}

fn assert_error_contains(value: &Value, needle: &str) {
    assert!(
        response_is_error(value),
        "expected error response containing {needle:?}: {value:?}"
    );
    assert_contains(value, needle);
}

fn find_string_key(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key).and_then(Value::as_str) {
                return Some(found.to_string());
            }
            map.values().find_map(|value| find_string_key(value, key))
        }
        Value::Array(values) => values.iter().find_map(|value| find_string_key(value, key)),
        _ => None,
    }
}
