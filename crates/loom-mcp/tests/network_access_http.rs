#![cfg(feature = "http")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use loom_core::digest::Algo;
use loom_store::FileStore;
use uldren_loom_mcp::server::{Binding, HttpNetworkAccess, serve_http_with_network_access};
use uldren_loom_mcp::{LoomMcp, StoreAccess};

fn temp_store(tag: &str) -> String {
    let path = std::env::temp_dir().join(format!(
        "loom-mcp-network-access-{tag}-{}.loom",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
    path.to_string_lossy().into_owned()
}

fn reserve_addr() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    addr
}

fn wait_for_listener(addr: SocketAddr) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match TcpStream::connect(addr) {
            Ok(_) => return,
            Err(err) if Instant::now() < deadline => {
                let _ = err;
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(err) => panic!("listener {addr} did not start: {err}"),
        }
    }
}

fn http_request(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}

fn spawn_mcp_http(
    store: &str,
    addr: SocketAddr,
    network_access: HttpNetworkAccess,
) -> (tokio::runtime::Runtime, tokio::task::JoinHandle<()>) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let mcp = LoomMcp::new(StoreAccess::per_request(store, None));
    let join = runtime.spawn(async move {
        let _ = serve_http_with_network_access(
            mcp,
            addr,
            Binding::default(),
            false,
            Some(network_access),
        )
        .await;
    });
    wait_for_listener(addr);
    (runtime, join)
}

#[test]
fn live_mcp_http_network_access_denies_before_mcp_service() {
    let store = temp_store("deny");
    let addr = reserve_addr();
    let network_access: HttpNetworkAccess = Arc::new(|_, _, _| false);
    let (runtime, join) = spawn_mcp_http(&store, addr, network_access);

    let response = http_request(
        addr,
        "GET /mcp HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 403 Forbidden"), "{response}");
    assert!(response.contains("network access denied"), "{response}");

    join.abort();
    drop(runtime);
    let _ = std::fs::remove_file(store);
}

#[test]
fn live_mcp_http_network_access_allows_and_passes_forwarded_headers() {
    let store = temp_store("allow-forwarded");
    let addr = reserve_addr();
    let seen = Arc::new(Mutex::new(
        None::<(SocketAddr, Option<String>, Option<String>)>,
    ));
    let seen_for_gate = seen.clone();
    let network_access: HttpNetworkAccess = Arc::new(move |peer, x_forwarded_for, forwarded| {
        *seen_for_gate.lock().unwrap() = Some((
            peer,
            x_forwarded_for.map(str::to_string),
            forwarded.map(str::to_string),
        ));
        x_forwarded_for == Some("203.0.113.7") && forwarded == Some("for=203.0.113.7;proto=https")
    });
    let (runtime, join) = spawn_mcp_http(&store, addr, network_access);

    let response = http_request(
        addr,
        "GET /mcp HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nX-Forwarded-For: 203.0.113.7\r\nForwarded: for=203.0.113.7;proto=https\r\n\r\n",
    );
    assert!(
        !response.starts_with("HTTP/1.1 403 Forbidden"),
        "{response}"
    );
    let seen = seen.lock().unwrap().clone().unwrap();
    assert_eq!(seen.0.ip().to_string(), "127.0.0.1");
    assert_eq!(seen.1.as_deref(), Some("203.0.113.7"));
    assert_eq!(seen.2.as_deref(), Some("for=203.0.113.7;proto=https"));

    join.abort();
    drop(runtime);
    let _ = std::fs::remove_file(store);
}
