use std::path::{Path, PathBuf};

use loom_core::workspace::{FacetKind, WorkspaceId};
use loom_core::{Algo, Loom};
use loom_store::{FileStore, save_loom};
use uldren_loom_mcp::{LoomMcp, StoreAccess};

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let store = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: import_mcp_apps <output.loom> <apps-dir> [workspace]".to_string())?;
    let apps_dir = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: import_mcp_apps <output.loom> <apps-dir> [workspace]".to_string())?;
    let workspace = args.next().unwrap_or_else(|| "apps".to_string());
    if args.next().is_some() {
        return Err("usage: import_mcp_apps <output.loom> <apps-dir> [workspace]".to_string());
    }
    if store.exists() {
        return Err(format!("output loom already exists: {}", store.display()));
    }
    if !apps_dir.is_dir() {
        return Err(format!(
            "apps directory does not exist: {}",
            apps_dir.display()
        ));
    }

    create_store(&store, &workspace)?;
    let mcp = LoomMcp::new(StoreAccess::per_request(&store, None));
    let mut imported = Vec::new();
    for entry in std::fs::read_dir(&apps_dir).map_err(|e| format!("read apps dir: {e}"))? {
        let entry = entry.map_err(|e| format!("read app entry: {e}"))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let app = entry
            .file_name()
            .into_string()
            .map_err(|_| "app directory name is not UTF-8".to_string())?;
        import_app(&mcp, &workspace, &app, &path)?;
        imported.push(app);
    }
    imported.sort();
    for app in imported {
        println!("imported {app}");
    }
    Ok(())
}

fn create_store(store: &Path, workspace: &str) -> Result<(), String> {
    let fs = FileStore::create_with_profile(store, Algo::Blake3).map_err(|e| e.to_string())?;
    let mut loom = Loom::new(fs);
    loom.registry_mut()
        .create(
            FacetKind::Files,
            Some(workspace),
            WorkspaceId::v4_from_bytes([77u8; 16]),
        )
        .map_err(|e| e.to_string())?;
    save_loom(&mut loom).map_err(|e| e.to_string())
}

fn import_app(mcp: &LoomMcp, workspace: &str, app: &str, app_dir: &Path) -> Result<(), String> {
    let index = read_file(&app_dir.join("index.html"))?;
    let meta = read_file(&app_dir.join("_meta.md"))?;
    mcp.write_mcp_app_create(workspace, app, &index, &meta)
        .map_err(|e| e.to_string())?;
    import_extra_files(mcp, workspace, app, app_dir, app_dir)?;
    let shown = mcp
        .read_mcp_app_show(workspace, app)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("imported app is not valid: {app}"))?;
    println!("{}", shown.uri);
    Ok(())
}

fn import_extra_files(
    mcp: &LoomMcp,
    workspace: &str,
    app: &str,
    root: &Path,
    dir: &Path,
) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("read app file entry: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            import_extra_files(mcp, workspace, app, root, &path)?;
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|e| format!("relative app path: {e}"))?;
        if rel == Path::new("index.html") || rel == Path::new("_meta.md") {
            continue;
        }
        let rel = rel
            .to_str()
            .ok_or_else(|| "app asset path is not UTF-8".to_string())?
            .replace(std::path::MAIN_SEPARATOR, "/");
        let bytes = read_file(&path)?;
        mcp.write_mcp_app_write_file(workspace, app, &rel, &bytes, 0o100644)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))
}
