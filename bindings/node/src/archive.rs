//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use std::path::Path;

use loom_interchange::ArchiveKind;
use loom_interchange_io::{
    ArchiveExportOptions, ArchiveImportOptions, CarExportOptions, CarImportOptions,
    FsExportOptions, FsImportOptions, export_archive, export_car, export_fs, import_archive,
    import_car, import_fs,
};

use super::*;

fn parse_archive_kind(kind: &str) -> napi::Result<ArchiveKind> {
    match kind {
        "zip" => Ok(ArchiveKind::Zip),
        "tar" => Ok(ArchiveKind::Tar),
        "tar-zstd" | "tar.zstd" | "tzst" => Ok(ArchiveKind::TarZstd),
        "tar-gzip" | "tar.gz" | "tgz" => Ok(ArchiveKind::TarGzip),
        "gzip" | "gz" => Ok(ArchiveKind::Gzip),
        other => Err(napi::Error::from_reason(format!(
            "unsupported archive kind {other:?}; expected tar-zstd, tar, tar-gzip, zip, or gzip"
        ))),
    }
}

#[napi]
pub fn fs_import(
    loom_path: String,
    workspace: String,
    src_path: String,
    commit: bool,
    dry_run: bool,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let mut options = FsImportOptions::new(&src_path);
    options.commit = commit;
    options.dry_run = dry_run;
    let report = import_fs(&mut loom, ns, Path::new(&src_path), &options).map_err(reason)?;
    if !dry_run {
        save_loom(&mut loom).map_err(reason)?;
    }
    report.encode().map(Uint8Array::from).map_err(reason)
}

#[napi]
pub fn fs_export(
    loom_path: String,
    workspace: String,
    dst_path: String,
    revision: Option<String>,
    dry_run: bool,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let mut options = FsExportOptions::new(&dst_path);
    options.dry_run = dry_run;
    options.revision = revision;
    let report = export_fs(&loom, ns, Path::new(&dst_path), &options).map_err(reason)?;
    report.encode().map(Uint8Array::from).map_err(reason)
}

#[napi]
pub fn archive_import(
    loom_path: String,
    workspace: String,
    src_path: String,
    kind: String,
    dry_run: bool,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let archive_kind = parse_archive_kind(&kind)?;
    let mut options = ArchiveImportOptions::new(&src_path);
    options.dry_run = dry_run;
    let result = import_archive(&mut loom, ns, Path::new(&src_path), archive_kind, &options)
        .map_err(reason)?;
    if !dry_run {
        save_loom(&mut loom).map_err(reason)?;
    }
    result.report.encode().map(Uint8Array::from).map_err(reason)
}

#[napi]
pub fn archive_export(
    loom_path: String,
    workspace: String,
    dst_path: String,
    kind: String,
    revision: Option<String>,
    dry_run: bool,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let archive_kind = parse_archive_kind(&kind)?;
    let mut options = ArchiveExportOptions::new(&dst_path);
    options.dry_run = dry_run;
    options.revision = revision;
    let result =
        export_archive(&loom, ns, Path::new(&dst_path), archive_kind, &options).map_err(reason)?;
    result.report.encode().map(Uint8Array::from).map_err(reason)
}

#[napi]
pub fn car_import(
    loom_path: String,
    src_path: String,
    dry_run: bool,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let mut options = CarImportOptions::new(&src_path);
    options.dry_run = dry_run;
    let result = import_car(&mut loom, Path::new(&src_path), &options).map_err(reason)?;
    if !dry_run {
        save_loom(&mut loom).map_err(reason)?;
    }
    result.report.encode().map(Uint8Array::from).map_err(reason)
}

#[napi]
pub fn car_export(
    loom_path: String,
    workspace: String,
    dst_path: String,
    dry_run: bool,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let mut options = CarExportOptions::new(&dst_path);
    options.dry_run = dry_run;
    let result = export_car(&loom, ns, Path::new(&dst_path), &options).map_err(reason)?;
    result.report.encode().map(Uint8Array::from).map_err(reason)
}
