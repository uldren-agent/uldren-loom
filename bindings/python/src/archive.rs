//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use std::path::Path;

use loom_interchange::ArchiveKind;
use loom_interchange_io::{
    ArchiveExportOptions, ArchiveImportOptions, CarExportOptions, CarImportOptions,
    FsExportOptions, FsImportOptions, export_archive, export_car, export_fs, import_archive,
    import_car, import_fs,
};

use super::*;

fn parse_archive_kind(kind: &str) -> PyResult<ArchiveKind> {
    match kind {
        "zip" => Ok(ArchiveKind::Zip),
        "tar" => Ok(ArchiveKind::Tar),
        "tar-zstd" | "tar.zstd" | "tzst" => Ok(ArchiveKind::TarZstd),
        "tar-gzip" | "tar.gz" | "tgz" => Ok(ArchiveKind::TarGzip),
        "gzip" | "gz" => Ok(ArchiveKind::Gzip),
        other => Err(PyRuntimeError::new_err(format!(
            "unsupported archive kind {other:?}; expected tar-zstd, tar, tar-gzip, zip, or gzip"
        ))),
    }
}

#[pyfunction]
#[pyo3(signature = (path, workspace, src_path, commit=false, dry_run=false, passphrase=None))]
pub(crate) fn fs_import<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    src_path: &str,
    commit: bool,
    dry_run: bool,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let mut options = FsImportOptions::new(src_path);
    options.commit = commit;
    options.dry_run = dry_run;
    let report = import_fs(&mut loom, ns, Path::new(src_path), &options).map_err(py_err)?;
    if !dry_run {
        save_loom(&mut loom).map_err(py_err)?;
    }
    let bytes = report.encode().map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, dst_path, revision=None, dry_run=false, passphrase=None))]
pub(crate) fn fs_export<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    dst_path: &str,
    revision: Option<String>,
    dry_run: bool,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let mut options = FsExportOptions::new(dst_path);
    options.dry_run = dry_run;
    options.revision = revision;
    let report = export_fs(&loom, ns, Path::new(dst_path), &options).map_err(py_err)?;
    let bytes = report.encode().map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, src_path, kind, dry_run=false, passphrase=None))]
pub(crate) fn archive_import<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    src_path: &str,
    kind: &str,
    dry_run: bool,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let archive_kind = parse_archive_kind(kind)?;
    let mut options = ArchiveImportOptions::new(src_path);
    options.dry_run = dry_run;
    let result = import_archive(&mut loom, ns, Path::new(src_path), archive_kind, &options)
        .map_err(py_err)?;
    if !dry_run {
        save_loom(&mut loom).map_err(py_err)?;
    }
    let bytes = result.report.encode().map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, dst_path, kind, revision=None, dry_run=false, passphrase=None))]
pub(crate) fn archive_export<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    dst_path: &str,
    kind: &str,
    revision: Option<String>,
    dry_run: bool,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let archive_kind = parse_archive_kind(kind)?;
    let mut options = ArchiveExportOptions::new(dst_path);
    options.dry_run = dry_run;
    options.revision = revision;
    let result =
        export_archive(&loom, ns, Path::new(dst_path), archive_kind, &options).map_err(py_err)?;
    let bytes = result.report.encode().map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, src_path, dry_run=false, passphrase=None))]
pub(crate) fn car_import<'py>(
    py: Python<'py>,
    path: &str,
    src_path: &str,
    dry_run: bool,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let mut options = CarImportOptions::new(src_path);
    options.dry_run = dry_run;
    let result = import_car(&mut loom, Path::new(src_path), &options).map_err(py_err)?;
    if !dry_run {
        save_loom(&mut loom).map_err(py_err)?;
    }
    let bytes = result.report.encode().map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, dst_path, dry_run=false, passphrase=None))]
pub(crate) fn car_export<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    dst_path: &str,
    dry_run: bool,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let mut options = CarExportOptions::new(dst_path);
    options.dry_run = dry_run;
    let result = export_car(&loom, ns, Path::new(dst_path), &options).map_err(py_err)?;
    let bytes = result.report.encode().map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}
