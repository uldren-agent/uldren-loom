use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use loom_client::LocalLoomClient;
use loom_hosted_core::HostedTlsConfig;
use loom_hosted_core::remote::{RemoteAuthMode, RemoteRuntime, RemoteServerConfig, RemoteTlsTrust};
use loom_hosted_core::remote_carrier::RemoteHttpServer;
use loom_hosted_core::remote_http::RemoteHttpService;
use loom_remote_protocol::discovery::{DiscoveryMode, DiscoveryRoutes};

fn temp_store() -> std::path::PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "loom-remote-carrier-{}-{}.loom",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::remove_file(&path).ok();
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

fn server_tls() -> Arc<tokio_rustls::rustls::ServerConfig> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    HostedTlsConfig::from_pem_bytes_with_client_trust(
        "cert.pem",
        cert.cert.pem().as_bytes(),
        "key.pem",
        cert.signing_key.serialize_pem().as_bytes(),
        None,
    )
    .unwrap()
    .server_config()
}

#[tokio::test]
async fn server_binds_a_tls_socket_and_accepts_connections() {
    let path = temp_store();
    let runtime = Arc::new(RemoteRuntime::start(&path, config()).expect("start"));
    let service = Arc::new(RemoteHttpService::new(
        runtime.clone(),
        "/apps/loom/v1/call",
    ));
    let server = RemoteHttpServer::bind("127.0.0.1:0".parse().unwrap(), server_tls(), service)
        .await
        .expect("bind");

    let addr = server.local_addr();
    assert_ne!(addr.port(), 0, "an ephemeral port was resolved");
    tokio::net::TcpStream::connect(addr)
        .await
        .expect("socket accepts connections");

    server.shutdown();
    runtime.shutdown();
    std::fs::remove_file(path).ok();
}
