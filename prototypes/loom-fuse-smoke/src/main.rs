//! Manual FUSE mount smoke check: create a loom, mount it, exercise read/write/mkdir/readdir through
//! the kernel, then unmount. On Linux an unprivileged mount needs either a setuid `fusermount3` or a
//! user workspace, so run it under `unshare -Urm --map-root-user <bin> <loom_path> <mountpoint>`.
//! Prints `SMOKE: OK` on success. This is a manual prototype, excluded from the workspace gate.

use std::path::Path;
use std::time::Duration;

use loom_core::workspace::{FacetKind, WorkspaceId};
use loom_core::{Algo, Loom};
use loom_store::{FileStore, save_loom};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: {} <loom_path> <mountpoint>", args[0]);
        std::process::exit(2);
    }
    let loom_path = &args[1];
    let mnt = args[2].clone();

    // Create a fresh loom with a files workspace and one seed file.
    {
        let store = FileStore::create_with_profile(loom_path, Algo::Blake3).expect("create store");
        let mut loom = Loom::new(store);
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("docs"),
                WorkspaceId::from_bytes([7; 16]),
            )
            .expect("create workspace");
        loom.write_file(ns, "hello.txt", b"hi", 0o100644)
            .expect("seed write");
        save_loom(&mut loom).expect("save");
    }

    // Mount it in the background.
    let session = loom_vfs_fuse::spawn(
        Path::new(loom_path),
        "files",
        "docs",
        Path::new(&mnt),
        false,
    )
    .expect("mount");
    std::thread::sleep(Duration::from_millis(300));

    let join = |name: &str| format!("{mnt}/{name}");

    // Read the seed file through the kernel.
    let seed = std::fs::read(join("hello.txt")).expect("read seed");
    assert_eq!(seed, b"hi", "seed content");

    // Write a new file and read it back.
    std::fs::write(join("new.txt"), b"world").expect("write new");
    assert_eq!(std::fs::read(join("new.txt")).expect("read new"), b"world");

    // Create a subdirectory and a file inside it.
    std::fs::create_dir(join("sub")).expect("mkdir");
    std::fs::write(join("sub/a.txt"), b"x").expect("write in subdir");
    assert_eq!(
        std::fs::read(join("sub/a.txt")).expect("read subdir file"),
        b"x"
    );

    // Directory listing reflects everything.
    let mut names: Vec<String> = std::fs::read_dir(&mnt)
        .expect("readdir")
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["hello.txt", "new.txt", "sub"],
        "directory listing"
    );

    // Unmount.
    drop(session);
    println!("SMOKE: OK");
}
