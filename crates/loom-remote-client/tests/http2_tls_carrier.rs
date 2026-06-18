//! End-to-end HTTP/2-over-TLS carrier: a `RemoteLoomClient` over `Http2TlsTransport`
//! talking to `RemoteHttpServer` over a live loopback TLS socket. Covers discovery, a unary call with
//! engine parity, a streaming queue range, and the SQL family (session open/exec/query/close, a batch
//! transaction, and an async task) across the wire with a self-signed cert.
//!
//! Licensed under BUSL-1.1.
#![cfg(feature = "carrier")]

use loom_client::LocalLoomClient;
use loom_hosted_core::remote::{
    RemoteAuth, RemoteAuthMode, RemoteRuntime, RemoteServerConfig, RemoteTlsTrust,
};
use loom_hosted_core::remote_carrier::RemoteHttpServer;
use loom_hosted_core::remote_http::RemoteHttpService;
use loom_locator::{ContextResolver, Layer};
use loom_remote_client::carrier::Http2TlsTransport;
use loom_remote_client::{RemoteConnection, RemoteLoomClient};
use loom_remote_protocol::discovery::{DiscoveryMode, DiscoveryRoutes};
use loom_remote_protocol::generated_api::{Kv, Queue, Sql, Store, Tasks, Workspaces};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerConfig};

const CALL_PATH: &str = "/apps/loom/v1/call";

fn temp_store() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "loom-remote-h2tls-{}-{}.loom",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::remove_dir_all(&path).ok();
    LocalLoomClient::new(&path).create().expect("create store");
    path
}

fn config() -> RemoteServerConfig {
    RemoteServerConfig {
        service_root: "https://localhost/apps/loom".to_string(),
        call_endpoint: "https://localhost/apps/loom/v1/call".to_string(),
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

/// A self-signed cert for `localhost`, returned as (server config, cert DER for the client to trust).
fn tls_material() -> (ServerConfig, Vec<u8>) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert_der = cert.cert.der().to_vec();
    let key_der = PrivateKeyDer::try_from(cert.signing_key.serialize_der()).unwrap();
    let server = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(cert_der.clone())], key_der)
        .unwrap();
    (server, cert_der)
}

fn client_config(trusted_cert: Vec<u8>) -> ClientConfig {
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(trusted_cert)).unwrap();
    ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

fn resolver() -> ContextResolver {
    ContextResolver::from_layers(&[Layer::new(
        "test",
        "[contexts.prod]\ntarget = \"https://localhost/apps/loom\"\n",
    )])
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_client_round_trips_over_http2_tls() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let path = temp_store();
    let runtime = Arc::new(RemoteRuntime::start(&path, config()).expect("start"));
    let connection = runtime.register_connection("h2-client");
    let session = runtime
        .open_session(connection, RemoteAuth::Unauthenticated)
        .expect("session");
    let service = Arc::new(RemoteHttpService::new(runtime.clone(), CALL_PATH));

    let (server_tls, cert_der) = tls_material();
    let server = RemoteHttpServer::bind("127.0.0.1:0".parse().unwrap(), server_tls, service)
        .await
        .expect("bind server");
    let addr = server.local_addr();

    let transport = Http2TlsTransport::new(addr, "localhost", CALL_PATH, client_config(cert_der));
    let conn = RemoteConnection::connect(transport, "prod", &resolver(), DiscoveryMode::Default)
        .await
        .expect("connect over tls");
    let client = RemoteLoomClient::new(conn);
    client.bind_session(session.id.clone());

    // Discovery negotiated over TLS.
    assert_eq!(client.connection().version(), 1);

    // Unary call parity across the real socket, via the generated `Store::version` stub.
    assert_eq!(
        client.version().await.expect("version"),
        LocalLoomClient::new(&path).store_version()
    );

    // Generated-client-to-generated-server parity across the socket: the stubs send the IDL handle at
    // arg 0, which the generated server decodes-and-discards in favor of its resolved engine session.
    let handle = client.open().await.expect("open store session");

    let key = loom_core::kv::key_to_cbor(&loom_core::tabular::Value::Text("k".to_string()));
    Kv::put(
        &client,
        handle.clone(),
        "app".to_string(),
        "c".to_string(),
        key.clone(),
        b"v".to_vec(),
    )
    .await
    .expect("kv put");
    assert_eq!(
        Kv::get(
            &client,
            handle.clone(),
            "app".to_string(),
            "c".to_string(),
            key
        )
        .await
        .expect("kv get"),
        Some(b"v".to_vec())
    );

    for entry in [b"a".as_slice(), b"b".as_slice()] {
        Queue::append(
            &client,
            handle.clone(),
            "jobs".to_string(),
            "in".to_string(),
            entry.to_vec(),
        )
        .await
        .expect("queue append");
    }
    assert_eq!(
        Queue::range(
            &client,
            handle.clone(),
            "jobs".to_string(),
            "in".to_string(),
            0,
            2
        )
        .await
        .expect("queue range"),
        vec![b"a".to_vec(), b"b".to_vec()]
    );

    client
        .workspace_create(handle, Some("proj".to_string()), None)
        .await
        .expect("workspace create");

    // SQL family end-to-end through the generated `Sql`/`Tasks` stubs over the live TLS socket. The IDL
    // `loom_path` is dropped on the wire; the runtime reopens the bound store by path per statement with
    // no runtime-held writer lock to conflict. The server runs on its own task, so the SQL engine's
    // internal executor does not nest with the client's.
    use futures::StreamExt;
    let sql = client
        .sql_open("sqlapp".to_string(), "db".to_string())
        .await
        .expect("sql open");
    for stmt in [
        "CREATE TABLE t (x INTEGER)",
        "INSERT INTO t (x) VALUES (1)",
        "INSERT INTO t (x) VALUES (2)",
    ] {
        client
            .sql_exec(sql.clone(), stmt.to_string())
            .await
            .expect("sql exec");
    }
    let mut rows = client
        .sql_query(sql.clone(), "SELECT x FROM t ORDER BY x".to_string())
        .await
        .expect("sql query");
    let mut row_count = 0;
    while let Some(row) = rows.next().await {
        row.expect("row frame");
        row_count += 1;
    }
    assert_eq!(row_count, 2, "two rows stream back over the TLS carrier");
    client.sql_close(sql).await.expect("sql close");

    // SqlBatch transaction end-to-end.
    let batch = client
        .sql_batch_begin("sqlapp".to_string(), "db".to_string())
        .await
        .expect("batch begin");
    client
        .sql_batch_exec(batch.clone(), "INSERT INTO t (x) VALUES (3)".to_string())
        .await
        .expect("batch exec");
    client
        .sql_batch_commit(batch.clone())
        .await
        .expect("batch commit");
    client.sql_batch_close(batch).await.expect("batch close");

    // Async SQL task end-to-end through the generated `Tasks` stubs: spawn, poll, take result, free.
    let sql2 = client
        .sql_open("sqlapp".to_string(), "db".to_string())
        .await
        .expect("sql open 2");
    let task = client
        .sql_exec_async(sql2.clone(), "INSERT INTO t (x) VALUES (4)".to_string())
        .await
        .expect("sql exec async");
    client.task_poll(task.clone()).await.expect("task poll");
    client.task_result(task.clone()).await.expect("task result");
    client.task_free(task).expect("task free");
    client.sql_close(sql2).await.expect("sql close 2");

    // The batch + async rows are durable: a fresh session reads four rows.
    let verify = client
        .sql_open("sqlapp".to_string(), "db".to_string())
        .await
        .expect("sql verify");
    let mut all = client
        .sql_query(verify.clone(), "SELECT x FROM t".to_string())
        .await
        .expect("verify query");
    let mut total = 0;
    while let Some(row) = all.next().await {
        row.expect("verify row");
        total += 1;
    }
    assert_eq!(
        total, 4,
        "batch + async rows persisted over the TLS carrier"
    );
    client.sql_close(verify).await.expect("verify close");

    server.shutdown();
    runtime.shutdown();
    std::fs::remove_dir_all(&path).ok();
}

/// A real remote client obtains its session over the carrier's dedicated session route - no in-process
/// `runtime.open_session` and no manual `bind_session`. After the wire handshake, ordinary calls and a KV
/// round trip work against the session the server minted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_opens_a_session_over_the_carrier_route() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let path = temp_store();
    let runtime = Arc::new(RemoteRuntime::start(&path, config()).expect("start"));
    // Deliberately no `register_connection` / `open_session` here: the client must establish its own
    // session over the wire.
    let service = Arc::new(RemoteHttpService::new(runtime.clone(), CALL_PATH));

    let (server_tls, cert_der) = tls_material();
    let server = RemoteHttpServer::bind("127.0.0.1:0".parse().unwrap(), server_tls, service)
        .await
        .expect("bind server");
    let addr = server.local_addr();

    let transport = Http2TlsTransport::new(addr, "localhost", CALL_PATH, client_config(cert_der));
    let conn = RemoteConnection::connect(transport, "prod", &resolver(), DiscoveryMode::Default)
        .await
        .expect("connect over tls");
    let client = RemoteLoomClient::new(conn);

    // Open the session purely over the carrier session route.
    let session_id = client
        .open_session(loom_remote_protocol::session::SessionAuth::Unauthenticated)
        .await
        .expect("open session over the wire");
    assert!(!session_id.is_empty(), "server minted an opaque session id");

    // The wire-opened session serves a real unary call with engine parity.
    assert_eq!(
        client.version().await.expect("version"),
        LocalLoomClient::new(&path).store_version()
    );

    // Store.open resolves to the wire-opened session, and a KV round trip works through it.
    let handle = client.open().await.expect("store open");
    let key = loom_core::kv::key_to_cbor(&loom_core::tabular::Value::Text("k".to_string()));
    Kv::put(
        &client,
        handle.clone(),
        "app".to_string(),
        "c".to_string(),
        key.clone(),
        b"v".to_vec(),
    )
    .await
    .expect("kv put");
    assert_eq!(
        Kv::get(&client, handle, "app".to_string(), "c".to_string(), key)
            .await
            .expect("kv get"),
        Some(b"v".to_vec())
    );

    server.shutdown();
    runtime.shutdown();
    std::fs::remove_dir_all(&path).ok();
}

/// An unbounded, server-driven tail streamed over the real TLS socket: proves the carrier delivers frames
/// incrementally (an infinite stream could never open if it were buffered), holds bounded memory (only a
/// small prefix is pulled from an endless source), and that cancelling by dropping the stream cleans up
/// without breaking the connection (a later unary call still succeeds).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unbounded_tail_streams_incrementally_and_cancels_cleanly() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let path = temp_store();
    let runtime = Arc::new(RemoteRuntime::start(&path, config()).expect("start"));
    let connection = runtime.register_connection("tail-client");
    let session = runtime
        .open_session(connection, RemoteAuth::Unauthenticated)
        .expect("session");
    let service = Arc::new(RemoteHttpService::new(runtime.clone(), CALL_PATH));

    let (server_tls, cert_der) = tls_material();
    let server = RemoteHttpServer::bind("127.0.0.1:0".parse().unwrap(), server_tls, service)
        .await
        .expect("bind server");
    let addr = server.local_addr();

    let transport = Http2TlsTransport::new(addr, "localhost", CALL_PATH, client_config(cert_der));
    let conn = RemoteConnection::connect(transport, "prod", &resolver(), DiscoveryMode::Default)
        .await
        .expect("connect over tls");
    let client = RemoteLoomClient::new(conn);
    client.bind_session(session.id.clone());

    // Opening returns immediately even though the tail never ends: frames are pulled incrementally, not
    // buffered whole. Pull a bounded prefix and assert the monotonic tick sequence.
    let mut stream = client
        .open_stream("Diagnostics", "event_tail", Vec::new())
        .await
        .expect("open tail");
    for expected in 0u64..5 {
        let item = stream
            .next_item()
            .await
            .expect("tick")
            .expect("an item frame");
        assert_eq!(
            u64::from_be_bytes(item.try_into().expect("8-byte tick")),
            expected
        );
    }

    // Cancel by dropping the stream: the reader task drops the HTTP/2 response, resetting the server
    // stream so it stops producing.
    drop(stream);

    // The connection survived the cancel: a unary call still round-trips.
    assert_eq!(
        client.version().await.expect("version after cancel"),
        LocalLoomClient::new(&path).store_version()
    );

    server.shutdown();
    runtime.shutdown();
    std::fs::remove_dir_all(&path).ok();
}
