use loom_core::workspace::FacetKind;
use loom_core::{Algo, Loom, WorkspaceId};
use loom_store::{FileStore, save_loom};
use loom_vfs_nfs::build_fs;
use nfsserve::tcp::{NFSTcp, NFSTcpListener};

fn seed_loom(dir: &std::path::Path) -> std::path::PathBuf {
    let loom_path = dir.join("t.loom");
    let store = FileStore::create_with_profile(&loom_path, Algo::Blake3).unwrap();
    let mut loom = Loom::new(store);
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("docs"),
            WorkspaceId::from_bytes([7; 16]),
        )
        .unwrap();
    loom.write_file(ns, "hello.txt", b"hi", 0o100644).unwrap();
    save_loom(&mut loom).unwrap();
    loom_path
}

#[test]
fn nfs_server_binds() {
    let dir = std::env::temp_dir().join(format!("loomnfs-bind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let loom_path = seed_loom(&dir);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let fs = build_fs(&loom_path, "docs", false).unwrap();
        let listener = NFSTcpListener::bind("127.0.0.1:0", fs).await.unwrap();
        assert!(listener.get_listen_port() > 0);
    });
    let _ = std::fs::remove_dir_all(&dir);
}
