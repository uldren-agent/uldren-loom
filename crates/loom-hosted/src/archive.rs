use std::path::Path;

use loom_core::error::{LoomError, Result};
use loom_core::{Loom, WorkspaceId};
use loom_interchange::ArchiveKind;
use loom_interchange_io::{
    ArchiveExportOptions, ArchiveImportOptions, CarExportOptions, CarImportOptions,
    FsExportOptions, FsImportOptions, export_archive, export_car, export_fs, import_archive,
    import_car, import_fs,
};
use loom_store::FileStore;

fn parse_archive_kind(kind: &str) -> Result<ArchiveKind> {
    match kind {
        "zip" => Ok(ArchiveKind::Zip),
        "tar" => Ok(ArchiveKind::Tar),
        "tar-zstd" | "tar.zstd" | "tzst" => Ok(ArchiveKind::TarZstd),
        "tar-gzip" | "tar.gz" | "tgz" => Ok(ArchiveKind::TarGzip),
        "gzip" | "gz" => Ok(ArchiveKind::Gzip),
        other => Err(LoomError::invalid(format!(
            "unsupported archive kind {other:?}; expected tar-zstd, tar, tar-gzip, zip, or gzip"
        ))),
    }
}

pub(crate) fn fs_import(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    src_path: &str,
    commit: bool,
    dry_run: bool,
) -> Result<Vec<u8>> {
    let mut options = FsImportOptions::new(src_path);
    options.commit = commit;
    options.dry_run = dry_run;
    let report = import_fs(loom, workspace, Path::new(src_path), &options)?;
    report.encode()
}

pub(crate) fn fs_export(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    dst_path: &str,
    revision: Option<&str>,
    dry_run: bool,
) -> Result<Vec<u8>> {
    let mut options = FsExportOptions::new(dst_path);
    options.dry_run = dry_run;
    options.revision = revision.map(str::to_string);
    let report = export_fs(loom, workspace, Path::new(dst_path), &options)?;
    report.encode()
}

pub(crate) fn archive_import(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    src_path: &str,
    kind: &str,
    dry_run: bool,
) -> Result<Vec<u8>> {
    let archive_kind = parse_archive_kind(kind)?;
    let mut options = ArchiveImportOptions::new(src_path);
    options.dry_run = dry_run;
    let result = import_archive(loom, workspace, Path::new(src_path), archive_kind, &options)?;
    result.report.encode()
}

pub(crate) fn archive_export(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    dst_path: &str,
    kind: &str,
    revision: Option<&str>,
    dry_run: bool,
) -> Result<Vec<u8>> {
    let archive_kind = parse_archive_kind(kind)?;
    let mut options = ArchiveExportOptions::new(dst_path);
    options.dry_run = dry_run;
    options.revision = revision.map(str::to_string);
    let result = export_archive(loom, workspace, Path::new(dst_path), archive_kind, &options)?;
    result.report.encode()
}

pub(crate) fn car_import(
    loom: &mut Loom<FileStore>,
    src_path: &str,
    dry_run: bool,
) -> Result<Vec<u8>> {
    let mut options = CarImportOptions::new(src_path);
    options.dry_run = dry_run;
    let result = import_car(loom, Path::new(src_path), &options)?;
    result.report.encode()
}

pub(crate) fn car_export(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    dst_path: &str,
    dry_run: bool,
) -> Result<Vec<u8>> {
    let mut options = CarExportOptions::new(dst_path);
    options.dry_run = dry_run;
    let result = export_car(loom, workspace, Path::new(dst_path), &options)?;
    result.report.encode()
}
