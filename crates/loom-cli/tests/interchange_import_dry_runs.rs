use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use loom_core::{Algo, FacetKind, Loom, WorkspaceId};
use loom_interchange_io::{CarExportOptions, export_car};
use loom_store::{FileStore, save_loom};

struct StoreFixture {
    path: String,
}

impl StoreFixture {
    fn new(tag: &str) -> Self {
        let mut path = PathBuf::from("/private/tmp");
        path.push(format!(
            "loom-interchange-import-dry-run-{tag}-{}-{}.loom",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = path.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&path);
        seed_store(&path);
        Self { path }
    }
}

impl Drop for StoreFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

struct TempDirFixture {
    path: PathBuf,
}

impl TempDirFixture {
    fn new(tag: &str) -> Self {
        let mut path = PathBuf::from("/private/tmp");
        path.push(format!(
            "loom-interchange-import-dry-run-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDirFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct StoreSnapshot {
    bytes: Vec<u8>,
    len: u64,
    modified: SystemTime,
    generation: u64,
}

impl StoreSnapshot {
    fn capture(path: &str) -> Self {
        let meta = std::fs::metadata(path).unwrap();
        Self {
            bytes: std::fs::read(path).unwrap(),
            len: meta.len(),
            modified: meta.modified().unwrap(),
            generation: FileStore::open_read(path)
                .unwrap()
                .mutable_overlay_generation()
                .unwrap()
                .as_u64(),
        }
    }

    fn assert_unchanged(&self, path: &str) {
        let meta = std::fs::metadata(path).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), self.bytes);
        assert_eq!(meta.len(), self.len);
        assert_eq!(meta.modified().unwrap(), self.modified);
        assert_eq!(
            FileStore::open_read(path)
                .unwrap()
                .mutable_overlay_generation()
                .unwrap()
                .as_u64(),
            self.generation
        );
    }
}

fn seed_store(path: &str) {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).unwrap();
    let workspace = WorkspaceId::v4_from_bytes([96; 16]);
    let mut loom = Loom::new(fs);
    loom.registry_mut()
        .create(FacetKind::Files, Some("main"), workspace)
        .unwrap();
    loom.registry_mut()
        .add_facet(workspace, FacetKind::Sql)
        .unwrap();
    save_loom(&mut loom).unwrap();
}

fn loom(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "loom {} failed with {}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn write_zip(path: &Path, name: &str, bytes: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file(name, options).unwrap();
    archive.write_all(bytes).unwrap();
    archive.finish().unwrap();
}

fn seed_car(path: &Path) {
    let source_store = path.with_extension("loom");
    let _ = std::fs::remove_file(&source_store);
    let fs = FileStore::create_with_profile(source_store.to_string_lossy().as_ref(), Algo::Blake3)
        .unwrap();
    let workspace = WorkspaceId::v4_from_bytes([97; 16]);
    let mut loom = Loom::new(fs);
    loom.registry_mut()
        .create(FacetKind::Files, Some("main"), workspace)
        .unwrap();
    loom.create_directory(workspace, "docs", true).unwrap();
    loom.write_file(workspace, "docs/a.txt", b"alpha", 0o100644)
        .unwrap();
    loom.commit(workspace, "seed", "seed", 1).unwrap();
    export_car(
        &loom,
        workspace,
        path,
        &CarExportOptions::new(path.to_string_lossy().into_owned()),
    )
    .unwrap();
    let _ = std::fs::remove_file(source_store);
}

#[test]
fn import_fs_dry_run_uses_cli_read_path_without_durable_mutation() {
    let fixture = StoreFixture::new("fs");
    let dir = TempDirFixture::new("fs-src");
    std::fs::create_dir_all(dir.path.join("docs")).unwrap();
    std::fs::write(dir.path.join("docs/a.txt"), b"alpha").unwrap();
    let before = StoreSnapshot::capture(&fixture.path);

    let output = loom(&[
        "interchange",
        "import-fs",
        &fixture.path,
        "main",
        dir.path.to_str().unwrap(),
        "--dry-run",
        "--format",
        "json",
    ]);

    assert!(output.contains("\"dry_run\""));
    assert!(output.contains("true"));
    before.assert_unchanged(&fixture.path);
}

#[test]
fn import_archive_dry_run_uses_cli_read_path_without_durable_mutation() {
    let fixture = StoreFixture::new("archive");
    let dir = TempDirFixture::new("archive-src");
    let archive = dir.path.join("notes.zip");
    write_zip(&archive, "docs/a.txt", b"alpha");
    let before = StoreSnapshot::capture(&fixture.path);

    let output = loom(&[
        "interchange",
        "import-archive",
        &fixture.path,
        "main",
        archive.to_str().unwrap(),
        "--kind",
        "zip",
        "--dry-run",
        "--format",
        "json",
    ]);

    assert!(output.contains("\"dry_run\""));
    assert!(output.contains("true"));
    before.assert_unchanged(&fixture.path);
}

#[test]
fn import_table_csv_dry_run_uses_cli_read_path_without_durable_mutation() {
    let fixture = StoreFixture::new("table-csv");
    let dir = TempDirFixture::new("table-csv-src");
    let csv = dir.path.join("items.csv");
    std::fs::write(&csv, b"id,name\n1,alpha\n").unwrap();
    let before = StoreSnapshot::capture(&fixture.path);

    let output = loom(&[
        "interchange",
        "import-table-csv",
        &fixture.path,
        "main",
        "app",
        "items",
        csv.to_str().unwrap(),
        "--schema",
        "id:int,name:text",
        "--primary-key",
        "id",
        "--dry-run",
        "--format",
        "json",
    ]);

    assert!(output.contains("\"dry_run\""));
    assert!(output.contains("true"));
    before.assert_unchanged(&fixture.path);
}

#[test]
fn import_car_dry_run_uses_cli_read_path_without_durable_mutation() {
    let fixture = StoreFixture::new("car");
    let dir = TempDirFixture::new("car-src");
    let car = dir.path.join("source.car");
    seed_car(&car);
    let before = StoreSnapshot::capture(&fixture.path);

    let output = loom(&[
        "interchange",
        "import-car",
        &fixture.path,
        car.to_str().unwrap(),
        "--dry-run",
        "--format",
        "json",
    ]);

    assert!(output.contains("\"dry_run\""));
    assert!(output.contains("true"));
    before.assert_unchanged(&fixture.path);
}
