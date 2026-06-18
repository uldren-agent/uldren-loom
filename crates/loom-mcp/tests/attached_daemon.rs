use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use loom_core::{
    AclEffect, AclGrant, AclRight, AclScope, AclStore, AclSubject, Algo, FacetKind, IdentityStore,
    Loom, PrincipalKind, WorkspaceId,
};
use loom_store::{FileStore, LocalOpenAuth, daemon, save_loom};
use uldren_loom_mcp::writes::WriteAdmission;
use uldren_loom_mcp::{LoomMcp, StoreAccess};

fn fresh_loom(path: &std::path::Path) {
    loom_coordination::with_local_store_write_lock(path, || {
        let store = FileStore::create_with_profile(path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        save_loom(&mut loom).unwrap();
        drop(loom);
        Ok(())
    })
    .unwrap();
}

fn temp_path() -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let uniq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "loom-mcp-attached-{}-{seq}-{uniq}.loom",
        std::process::id()
    ))
}

fn nid(seed: u8) -> WorkspaceId {
    WorkspaceId::from_bytes([seed; 16])
}

fn fake_daemon_exchange(mut stream: std::net::TcpStream, response: &str) -> String {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let mut request = String::new();
    {
        let mut reader = std::io::BufReader::new(&mut stream);
        reader.read_line(&mut request).unwrap();
    }
    stream.write_all(response.as_bytes()).unwrap();
    request
}

#[test]
fn attached_access_rejects_stale_daemon_address_as_not_running() {
    let path = temp_path();
    fresh_loom(&path);
    let paths = daemon::paths(&path).unwrap();
    std::fs::create_dir_all(paths.addr_file.parent().unwrap()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    std::fs::write(&paths.addr_file, addr.to_string()).unwrap();
    let err = match StoreAccess::per_request_attached(&path, None) {
        Ok(_) => panic!("attached access must reject a stale daemon address"),
        Err(err) => err,
    };
    assert_eq!(err.code, loom_core::error::Code::NotFound);
    let _ = std::fs::remove_file(&paths.addr_file);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn attached_access_attaches_and_detaches() {
    let path = temp_path();
    fresh_loom(&path);
    let paths = daemon::paths(&path).unwrap();
    std::fs::create_dir_all(paths.addr_file.parent().unwrap()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let join = std::thread::spawn(move || {
        for response in ["attached\tok\n", "detached\tok\n"] {
            let (stream, _) = listener.accept().unwrap();
            let request = fake_daemon_exchange(stream, response);
            tx.send(request).unwrap();
        }
    });
    {
        let access = StoreAccess::per_request_attached(&path, None).unwrap();
        assert!(matches!(access, StoreAccess::PerRequest { .. }));
    }
    join.join().unwrap();
    let attach = rx.recv().unwrap();
    let detach = rx.recv().unwrap();
    assert!(attach.starts_with("session-attach\tmcp:"));
    assert!(detach.starts_with("session-detach\tmcp:"));
    let _ = std::fs::remove_file(&paths.addr_file);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn attached_access_applies_launch_principal_auth() {
    let path = temp_path();
    let user = nid(2);
    loom_coordination::with_local_store_write_lock(&path, || {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let root = nid(1);
        let mut identity = IdentityStore::new(root);
        identity
            .add_principal(user, "alice", PrincipalKind::User)
            .unwrap();
        identity
            .set_passphrase(user, "alice-pass", b"12345678")
            .unwrap();
        store.save_identity_store(&identity).unwrap();
        store.save_acl_store(&AclStore::new()).unwrap();
        let mut loom = Loom::new(store);
        save_loom(&mut loom).unwrap();
        drop(loom);
        Ok(())
    })
    .unwrap();

    let paths = daemon::paths(&path).unwrap();
    std::fs::create_dir_all(paths.addr_file.parent().unwrap()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let join = std::thread::spawn(move || {
        for response in ["attached\tok\n", "session\tlive\n", "detached\tok\n"] {
            let (stream, _) = listener.accept().unwrap();
            fake_daemon_exchange(stream, response);
        }
    });
    {
        let access = StoreAccess::per_request_attached_auth(
            &path,
            LocalOpenAuth {
                principal: Some(user),
                passphrase: Some("alice-pass".to_string()),
                session_id: Some("mcp-test".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        access
            .read(|loom| {
                assert_eq!(loom.effective_principal()?.unwrap(), user);
                Ok(())
            })
            .unwrap();
    }
    join.join().unwrap();
    let _ = std::fs::remove_file(&paths.addr_file);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn attached_access_fails_closed_when_session_is_not_live() {
    let path = temp_path();
    fresh_loom(&path);
    let paths = daemon::paths(&path).unwrap();
    std::fs::create_dir_all(paths.addr_file.parent().unwrap()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let join = std::thread::spawn(move || {
        for response in [
            "attached\tok\n",
            "error\tNOT_FOUND: daemon session is not attached\n",
            "detached\tok\n",
        ] {
            let (stream, _) = listener.accept().unwrap();
            fake_daemon_exchange(stream, response);
        }
    });
    {
        let access = StoreAccess::per_request_attached(&path, None).unwrap();
        let err = access.read(|_| Ok(())).unwrap_err();
        assert_eq!(err.code, loom_core::Code::NotFound);
        assert!(err.message.contains("no longer live"));
    }
    join.join().unwrap();
    let _ = std::fs::remove_file(&paths.addr_file);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn attached_access_write_fails_closed_before_daemon_authorized_open() {
    let path = temp_path();
    fresh_loom(&path);
    let paths = daemon::paths(&path).unwrap();
    std::fs::create_dir_all(paths.addr_file.parent().unwrap()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let join = std::thread::spawn(move || {
        for response in [
            "attached\tok\n",
            "error\tNOT_FOUND: daemon session is not attached\n",
            "detached\tok\n",
        ] {
            let (stream, _) = listener.accept().unwrap();
            let request = fake_daemon_exchange(stream, response);
            tx.send(request).unwrap();
        }
    });
    {
        let access = StoreAccess::per_request_attached(&path, None).unwrap();
        let err = access.write(|_| Ok(())).unwrap_err();
        assert_eq!(err.code, loom_core::Code::NotFound);
        assert!(err.message.contains("no longer live"));
    }
    join.join().unwrap();
    let attach = rx.recv().unwrap();
    let check = rx.recv().unwrap();
    let detach = rx.recv().unwrap();
    assert!(attach.starts_with("session-attach\tmcp:"));
    assert!(check.starts_with("session-check\tmcp:"));
    assert!(detach.starts_with("session-detach\tmcp:"));
    let _ = std::fs::remove_file(&paths.addr_file);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn attached_access_denies_without_auth_and_permits_launch_principal() {
    let path = temp_path();
    let user = nid(2);
    loom_coordination::with_local_store_write_lock(&path, || {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, Some("work"), nid(3))
            .unwrap();
        loom.write_file(ns, "a.txt", b"hello", 0o100644).unwrap();
        let root = nid(1);
        let mut identity = IdentityStore::new(root);
        identity
            .add_principal(user, "alice", PrincipalKind::User)
            .unwrap();
        identity
            .set_passphrase(user, "alice-pass", b"12345678")
            .unwrap();
        loom.store().save_identity_store(&identity).unwrap();
        let mut acl = AclStore::new();
        acl.grant(AclGrant {
            subject: AclSubject::Principal(user),
            workspace: Some(ns),
            domain: Some(FacetKind::Files.into()),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: [AclRight::Read].into_iter().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();
        loom.store().save_acl_store(&acl).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);
        Ok(())
    })
    .unwrap();

    let paths = daemon::paths(&path).unwrap();
    std::fs::create_dir_all(paths.addr_file.parent().unwrap()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let join = std::thread::spawn(move || {
        for response in [
            "attached\tok\n",
            "session\tlive\n",
            "detached\tok\n",
            "attached\tok\n",
            "session\tlive\n",
            "detached\tok\n",
        ] {
            let (stream, _) = listener.accept().unwrap();
            fake_daemon_exchange(stream, response);
        }
    });

    {
        let access = StoreAccess::per_request_attached(&path, None).unwrap();
        let mcp = LoomMcp::new(access);
        let err = mcp.read_fs_read_file("work", "a.txt").unwrap_err();
        assert_eq!(err.code, loom_core::error::Code::AuthenticationFailed);
    }
    {
        let access = StoreAccess::per_request_attached_auth(
            &path,
            LocalOpenAuth {
                principal: Some(user),
                passphrase: Some("alice-pass".to_string()),
                session_id: Some("mcp-test".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        let mcp = LoomMcp::new(access);
        assert_eq!(mcp.read_fs_read_file("work", "a.txt").unwrap(), b"hello");
    }
    join.join().unwrap();
    let _ = std::fs::remove_file(&paths.addr_file);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn attached_access_can_write_while_daemon_owns_store() {
    let path = temp_path();
    fresh_loom(&path);
    let paths = daemon::paths(&path).unwrap();
    std::fs::create_dir_all(paths.addr_file.parent().unwrap()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let join = std::thread::spawn(move || {
        for response in [
            "attached\tok\n".to_string(),
            "session\tlive\n".to_string(),
            "detached\tok\n".to_string(),
        ] {
            let (stream, _) = listener.accept().unwrap();
            let request = fake_daemon_exchange(stream, &response);
            tx.send(request).unwrap();
        }
    });
    {
        let access = StoreAccess::per_request_attached(&path, None).unwrap();
        access
            .write(|_loom| Ok(()))
            .expect("attached writes use daemon-authorized open");
    }
    join.join().unwrap();
    let attach = rx.recv().unwrap();
    let check = rx.recv().unwrap();
    let detach = rx.recv().unwrap();
    assert!(attach.starts_with("session-attach\tmcp:"));
    assert!(check.starts_with("session-check\tmcp:"));
    assert!(detach.starts_with("session-detach\tmcp:"));
    let _ = std::fs::remove_file(&paths.addr_file);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn attached_drive_lease_lifecycle_uses_daemon_locks() {
    let path = temp_path();
    let ns = loom_coordination::with_local_store_write_lock(&path, || {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, Some("repo"), nid(3))
            .unwrap();
        loom.registry_mut().add_facet(ns, FacetKind::Vcs).unwrap();
        save_loom(&mut loom).unwrap();
        drop(loom);
        Ok(ns)
    })
    .unwrap();

    let key = format!("drive/{ns}/main/file/file-1");
    let paths = daemon::paths(&path).unwrap();
    std::fs::create_dir_all(paths.addr_file.parent().unwrap()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let join = std::thread::spawn({
        let key = key.clone();
        let ns = ns.to_string();
        move || {
            let mut acquire_count = 0u64;
            let mut refresh_count = 0u64;
            loop {
                let (stream, _) = listener.accept().unwrap();
                let mut stream = stream;
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut request = String::new();
                {
                    let mut reader = std::io::BufReader::new(&mut stream);
                    reader.read_line(&mut request).unwrap();
                }
                let response = if request.starts_with("session-attach\t") {
                    "attached\tok\n".to_string()
                } else if request.starts_with("session-check\t") {
                    "session\tlive\n".to_string()
                } else if request.starts_with("lock-acquire\t") {
                    acquire_count += 1;
                    let deadline = match acquire_count {
                        1 => 1001,
                        2 => 3001,
                        _ => 4001,
                    };
                    format!("lock\t{key}\t{ns}\tmcp:test\texclusive\t{acquire_count}\t{deadline}\n")
                } else if request.starts_with("lock-apply-fence\t") {
                    "applied\n".to_string()
                } else if request.starts_with("lock-refresh\t") {
                    refresh_count += 1;
                    if refresh_count == 1 {
                        format!("lock\t{key}\t{ns}\tmcp:test\texclusive\t1\t2001\n")
                    } else {
                        "error\tLOCK_LEASE_EXPIRED: lock lease expired\n".to_string()
                    }
                } else if request.starts_with("lock-release\t") {
                    "released\n".to_string()
                } else if request.starts_with("lock-break\t") {
                    "broken\t1\n".to_string()
                } else if request.starts_with("session-detach\t") {
                    "detached\tok\n".to_string()
                } else {
                    format!("error\tunexpected request {request:?}\n")
                };
                stream.write_all(response.as_bytes()).unwrap();
                let stop = request.starts_with("session-detach\t");
                tx.send(request).unwrap();
                if stop {
                    break;
                }
            }
        }
    });
    {
        let access = StoreAccess::per_request_attached(&path, None).unwrap();
        let mcp = LoomMcp::new(access);
        let root = mcp
            .read_drive_list("repo", "main", "root")
            .unwrap()
            .profile_root;
        let acquired = mcp
            .write_drive_acquire_lease("repo", "main", "file", "file-1", 1000, 0)
            .unwrap();
        assert_eq!(acquired.key, key);
        assert_eq!(acquired.principal, ns.to_string());
        assert_eq!(acquired.mode, "exclusive");
        assert_eq!(acquired.fence.sequence, 1);
        assert_eq!(acquired.lease_deadline_ms, 1001);

        let folder = mcp
            .write_drive_create_folder(
                "repo",
                "main",
                "root",
                "folder-1",
                "Specs",
                &root,
                Some(&WriteAdmission {
                    target_kind: "file".to_string(),
                    target_id: "file-1".to_string(),
                    fence: loom_core::Fence::from(acquired.fence),
                }),
            )
            .unwrap();
        assert_eq!(folder.operation_kind, "folder.created");

        let refreshed = mcp
            .write_drive_refresh_lease(
                "repo",
                "main",
                "file",
                "file-1",
                loom_core::Fence::embedded(1),
                1000,
            )
            .unwrap();
        assert_eq!(refreshed.key, key);
        assert_eq!(refreshed.lease_deadline_ms, 2001);

        assert!(
            mcp.write_drive_release_lease(
                "repo",
                "main",
                "file",
                "file-1",
                loom_core::Fence::embedded(1),
            )
            .unwrap()
        );
        let reacquired = mcp
            .write_drive_acquire_lease("repo", "main", "file", "file-1", 1000, 0)
            .unwrap();
        assert_eq!(reacquired.key, key);
        assert_eq!(reacquired.fence.sequence, 2);
        let expired = mcp
            .write_drive_refresh_lease(
                "repo",
                "main",
                "file",
                "file-1",
                loom_core::Fence::embedded(2),
                1000,
            )
            .unwrap_err();
        assert_eq!(expired.code, loom_core::error::Code::LockLeaseExpired);
        let broken = mcp
            .write_drive_break_lease("repo", "main", "file", "file-1")
            .unwrap();
        assert_eq!(broken.key, key);
        assert_eq!(broken.broken_holders, 1);
    }
    join.join().unwrap();
    let requests: Vec<String> = rx.try_iter().collect();
    assert!(requests[0].starts_with("session-attach\tmcp:"));
    assert!(requests.iter().any(|request| {
        request.starts_with(&format!("lock-acquire\t{key}\t{ns}\tmcp:"))
            && request.contains("\texclusive\t1000\t")
    }));
    assert!(requests.iter().any(|request| {
        request.starts_with(&format!("lock-apply-fence\t{key}\t{ns}\tmcp:"))
            && request.contains("\texclusive\t1\t")
    }));
    assert!(requests.iter().any(|request| {
        request.starts_with(&format!("lock-refresh\t{key}\t{ns}\tmcp:"))
            && request.contains("\texclusive\t1\t1000\t")
    }));
    assert!(requests.iter().any(|request| {
        request.starts_with(&format!("lock-release\t{key}\t{ns}\tmcp:"))
            && request.contains("\texclusive\t1\t")
    }));
    assert!(
        requests
            .iter()
            .any(|request| request.starts_with(&format!("lock-break\t{key}\t")))
    );
    assert!(requests.last().unwrap().starts_with("session-detach\tmcp:"));
    let store = FileStore::open_read(&path).unwrap();
    let log = store
        .control_get(&loom_substrate::drive::drive_operation_log_key("main").unwrap())
        .unwrap()
        .unwrap();
    let log = loom_substrate::drive::DriveOperationLog::decode(&log).unwrap();
    let kinds: Vec<_> = log
        .records
        .iter()
        .map(|record| record.operation_kind.as_str())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "lock.acquired",
            "folder.created",
            "lock.refreshed",
            "lock.released",
            "lock.acquired",
            "lock.expired",
            "lock.broken"
        ]
    );
    let _ = std::fs::remove_file(&paths.addr_file);
    let _ = std::fs::remove_file(&path);
}
