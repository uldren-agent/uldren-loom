use loom_core::error::Code;
use loom_core::lock::LockMode;
use loom_store::FileStore;
use loom_store::daemon::{self, AcquireRequest};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TempPath(std::path::PathBuf);

impl TempPath {
    fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let mut p = std::env::temp_dir();
        p.push(format!("loomstore-{tag}-{pid}-{n}.loom"));
        let _ = std::fs::remove_file(&p);
        Self(p)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap()
}

#[test]
fn active_daemon_rejects_direct_writer_but_allows_reader_and_authorized_open() {
    let tp = TempPath::new("daemon-owned-direct-open");
    drop(FileStore::open(tp.path()).unwrap());
    let paths = daemon::paths(tp.path()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let store = paths.store.clone();
    let store_id = paths.store_id.clone();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = String::new();
        stream.read_to_string(&mut request).unwrap();
        assert_eq!(request, "status\n");
        writeln!(
            stream,
            "running\tprotocol=1\ttransport=tcp\tfake-pid\t{store}\tidentity={store_id}\tsessions=0\tpins=0"
        )
        .unwrap();
    });

    let err = FileStore::open(tp.path()).unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    assert!(
        err.to_string()
            .contains("direct writable opens are disabled")
    );
    handle.join().unwrap();

    let reader = FileStore::open_read(tp.path()).unwrap();
    let writer = FileStore::open_daemon_authorized(tp.path()).unwrap();
    drop(writer);
    drop(reader);
    let _ = std::fs::remove_file(paths.addr_file);
}

#[test]
fn stale_daemon_address_does_not_block_direct_writer() {
    let tp = TempPath::new("stale-daemon-direct-open");
    drop(FileStore::open(tp.path()).unwrap());
    let paths = daemon::paths(tp.path()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    std::fs::write(&paths.addr_file, addr.to_string()).unwrap();

    let writer = FileStore::open(tp.path()).unwrap();
    drop(writer);
    let _ = std::fs::remove_file(paths.addr_file);
}

#[test]
fn lock_acquire_zero_wait_returns_locked_immediately() {
    let root = std::env::temp_dir().join(format!("loom-daemon-lock-nowait-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let store = root.join("store.loom");
    std::fs::write(&store, b"store").unwrap();
    let paths = daemon::paths(&store).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let join = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = String::new();
        stream.read_to_string(&mut request).unwrap();
        assert!(request.starts_with("lock-acquire\tresource\talice\ts1\texclusive\t"));
        stream
            .write_all(b"error\tLOCKED: lock is held by another owner\n")
            .unwrap();
    });
    let err = daemon::lock_acquire(
        &paths,
        AcquireRequest {
            key: "resource",
            principal: "alice",
            session: "s1",
            mode: LockMode::Exclusive,
            lease_ms: 100,
            wait_ms: 0,
            now_ms: now_ms(),
        },
    )
    .unwrap_err();
    join.join().unwrap();
    assert_eq!(err.code, Code::Locked);
    let _ = std::fs::remove_file(paths.addr_file);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn lock_acquire_bounded_wait_retries_until_available() {
    let root = std::env::temp_dir().join(format!("loom-daemon-lock-wait-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let store = root.join("store.loom");
    std::fs::write(&store, b"store").unwrap();
    let paths = daemon::paths(&store).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let join = std::thread::spawn(move || {
        for response in [
            "error\tLOCKED: lock is held by another owner\n",
            "lock\tresource\talice\ts1\texclusive\t7\t12345\n",
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            stream.read_to_string(&mut request).unwrap();
            assert!(request.starts_with("lock-acquire\tresource\talice\ts1\texclusive\t"));
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    let response = daemon::lock_acquire(
        &paths,
        AcquireRequest {
            key: "resource",
            principal: "alice",
            session: "s1",
            mode: LockMode::Exclusive,
            lease_ms: 100,
            wait_ms: 500,
            now_ms: now_ms(),
        },
    )
    .unwrap();
    join.join().unwrap();
    assert_eq!(response, "lock\tresource\talice\ts1\texclusive\t7\t12345\n");
    let _ = std::fs::remove_file(paths.addr_file);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn stale_daemon_address_is_not_found() {
    let root =
        std::env::temp_dir().join(format!("loom-daemon-stale-address-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let addr_file = root.join("stale.addr");
    std::fs::write(&addr_file, addr.to_string()).unwrap();
    let err = daemon::request(&addr_file, "status\n").unwrap_err();
    assert_eq!(err.code, Code::NotFound);
    assert!(err.message.contains("daemon is not running"));
    let _ = std::fs::remove_dir_all(root);
}
