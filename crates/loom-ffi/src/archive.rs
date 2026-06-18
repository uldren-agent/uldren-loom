//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use std::path::Path;

use loom_interchange::ArchiveKind;
use loom_interchange_io::{
    ArchiveExportOptions, ArchiveImportOptions, CarExportOptions, CarImportOptions,
    FsExportOptions, FsImportOptions, export_archive, export_car, export_fs, import_archive,
    import_car, import_fs,
};

use super::*;

fn parse_archive_kind(kind: &str) -> LoomResult<ArchiveKind> {
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

fn optional_revision(value: *const c_char, what: &'static str) -> LoomResult<Option<String>> {
    if value.is_null() {
        Ok(None)
    } else {
        unsafe { cstr(value) }
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| LoomError::invalid(format!("{what}: non-UTF-8 revision")))
    }
}

fn archive_import_ns(
    h: &LoomSession,
    workspace: &str,
    src_path: &str,
    kind: &str,
    dry_run: bool,
) -> LoomResult<Vec<u8>> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let archive_kind = parse_archive_kind(kind)?;
    let mut options = ArchiveImportOptions::new(src_path);
    options.dry_run = dry_run;
    let result = import_archive(&mut loom, ns, Path::new(src_path), archive_kind, &options)?;
    if !dry_run {
        save_loom(&mut loom)?;
    }
    result.report.encode()
}

fn fs_import_ns(
    h: &LoomSession,
    workspace: &str,
    src_path: &str,
    commit: bool,
    dry_run: bool,
) -> LoomResult<Vec<u8>> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let mut options = FsImportOptions::new(src_path);
    options.commit = commit;
    options.dry_run = dry_run;
    let report = import_fs(&mut loom, ns, Path::new(src_path), &options)?;
    if !dry_run {
        save_loom(&mut loom)?;
    }
    report.encode()
}

fn fs_export_ns(
    h: &LoomSession,
    workspace: &str,
    dst_path: &str,
    revision: Option<String>,
    dry_run: bool,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let mut options = FsExportOptions::new(dst_path);
    options.revision = revision;
    options.dry_run = dry_run;
    let report = export_fs(&loom, ns, Path::new(dst_path), &options)?;
    report.encode()
}

fn archive_export_ns(
    h: &LoomSession,
    workspace: &str,
    dst_path: &str,
    kind: &str,
    revision: Option<String>,
    dry_run: bool,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let archive_kind = parse_archive_kind(kind)?;
    let mut options = ArchiveExportOptions::new(dst_path);
    options.dry_run = dry_run;
    options.revision = revision;
    let result = export_archive(&loom, ns, Path::new(dst_path), archive_kind, &options)?;
    result.report.encode()
}

fn car_import_ns(h: &LoomSession, src_path: &str, dry_run: bool) -> LoomResult<Vec<u8>> {
    let mut loom = open_h_write(h)?;
    let mut options = CarImportOptions::new(src_path);
    options.dry_run = dry_run;
    let result = import_car(&mut loom, Path::new(src_path), &options)?;
    if !dry_run {
        save_loom(&mut loom)?;
    }
    result.report.encode()
}

fn car_export_ns(
    h: &LoomSession,
    workspace: &str,
    dst_path: &str,
    dry_run: bool,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let mut options = CarExportOptions::new(dst_path);
    options.dry_run = dry_run;
    let result = export_car(&loom, ns, Path::new(dst_path), &options)?;
    result.report.encode()
}

/// Import a host filesystem tree into a workspace and return canonical-CBOR `ImportReport` bytes.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `src_path` must be valid C strings;
/// `out_ptr` and `out_len` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_fs_import(
    handle: *mut LoomSession,
    workspace: *const c_char,
    src_path: *const c_char,
    commit: i32,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_fs_import");
    let workspace = arg_str!(workspace, "loom_fs_import");
    let src_path = arg_str!(src_path, "loom_fs_import");
    match fs_import_ns(h, workspace, src_path, commit != 0, dry_run != 0) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Export a workspace file tree to a host filesystem tree and return canonical-CBOR `ExportReport`
/// bytes.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`, `dst_path`, and optional `revision` must be valid
/// C strings; `out_ptr` and `out_len` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_fs_export(
    handle: *mut LoomSession,
    workspace: *const c_char,
    dst_path: *const c_char,
    revision: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_fs_export");
    let workspace = arg_str!(workspace, "loom_fs_export");
    let dst_path = arg_str!(dst_path, "loom_fs_export");
    let revision = match optional_revision(revision, "loom_fs_export") {
        Ok(revision) => revision,
        Err(e) => return fail(e),
    };
    match fs_export_ns(h, workspace, dst_path, revision, dry_run != 0) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import a host archive into a workspace and return canonical-CBOR `ImportReport` bytes.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`, `src_path`, and `kind` must be valid C strings;
/// `out_ptr` and `out_len` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_archive_import(
    handle: *mut LoomSession,
    workspace: *const c_char,
    src_path: *const c_char,
    kind: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_archive_import");
    let workspace = arg_str!(workspace, "loom_archive_import");
    let src_path = arg_str!(src_path, "loom_archive_import");
    let kind = arg_str!(kind, "loom_archive_import");
    match archive_import_ns(h, workspace, src_path, kind, dry_run != 0) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Export a workspace file tree to a host archive and return canonical-CBOR `ExportReport` bytes.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`, `dst_path`, `kind`, and optional `revision` must
/// be valid C strings; `out_ptr` and `out_len` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_archive_export(
    handle: *mut LoomSession,
    workspace: *const c_char,
    dst_path: *const c_char,
    kind: *const c_char,
    revision: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_archive_export");
    let workspace = arg_str!(workspace, "loom_archive_export");
    let dst_path = arg_str!(dst_path, "loom_archive_export");
    let kind = arg_str!(kind, "loom_archive_export");
    let revision = match optional_revision(revision, "loom_archive_export") {
        Ok(revision) => revision,
        Err(e) => return fail(e),
    };
    match archive_export_ns(h, workspace, dst_path, kind, revision, dry_run != 0) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Import a deterministic Loom CAR file and return canonical-CBOR `ImportReport` bytes.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `src_path` must be a valid C string; `out_ptr` and `out_len`
/// must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_car_import(
    handle: *mut LoomSession,
    src_path: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_car_import");
    let src_path = arg_str!(src_path, "loom_car_import");
    match car_import_ns(h, src_path, dry_run != 0) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Export a workspace object graph to deterministic Loom CAR and return canonical-CBOR `ExportReport`
/// bytes.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` and `dst_path` must be valid C strings; `out_ptr`
/// and `out_len` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_car_export(
    handle: *mut LoomSession,
    workspace: *const c_char,
    dst_path: *const c_char,
    dry_run: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_car_export");
    let workspace = arg_str!(workspace, "loom_car_export");
    let dst_path = arg_str!(dst_path, "loom_car_export");
    match car_export_ns(h, workspace, dst_path, dry_run != 0) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Async form of [`loom_fs_import`].
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out_task` must be
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_fs_import_async(
    handle: *mut LoomSession,
    workspace: *const c_char,
    src_path: *const c_char,
    commit: i32,
    dry_run: i32,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_fs_import_async");
    let workspace = arg_str!(workspace, "loom_fs_import_async").to_string();
    let src_path = arg_str!(src_path, "loom_fs_import_async").to_string();
    let owned = task_handle(h);
    let commit = commit != 0;
    let dry_run = dry_run != 0;
    unsafe {
        spawn_task(
            out_task,
            Box::new(move || fs_import_ns(&owned, &workspace, &src_path, commit, dry_run)),
        )
    }
}

/// Async form of [`loom_fs_export`].
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out_task` must be
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_fs_export_async(
    handle: *mut LoomSession,
    workspace: *const c_char,
    dst_path: *const c_char,
    revision: *const c_char,
    dry_run: i32,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_fs_export_async");
    let workspace = arg_str!(workspace, "loom_fs_export_async").to_string();
    let dst_path = arg_str!(dst_path, "loom_fs_export_async").to_string();
    let revision = match optional_revision(revision, "loom_fs_export_async") {
        Ok(revision) => revision,
        Err(e) => return fail(e),
    };
    let owned = task_handle(h);
    let dry_run = dry_run != 0;
    unsafe {
        spawn_task(
            out_task,
            Box::new(move || fs_export_ns(&owned, &workspace, &dst_path, revision, dry_run)),
        )
    }
}

/// Async form of [`loom_archive_import`].
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out_task` must be
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_archive_import_async(
    handle: *mut LoomSession,
    workspace: *const c_char,
    src_path: *const c_char,
    kind: *const c_char,
    dry_run: i32,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_archive_import_async");
    let workspace = arg_str!(workspace, "loom_archive_import_async").to_string();
    let src_path = arg_str!(src_path, "loom_archive_import_async").to_string();
    let kind = arg_str!(kind, "loom_archive_import_async").to_string();
    let owned = task_handle(h);
    let dry_run = dry_run != 0;
    unsafe {
        spawn_task(
            out_task,
            Box::new(move || archive_import_ns(&owned, &workspace, &src_path, &kind, dry_run)),
        )
    }
}

/// Async form of [`loom_archive_export`].
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out_task` must be
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_archive_export_async(
    handle: *mut LoomSession,
    workspace: *const c_char,
    dst_path: *const c_char,
    kind: *const c_char,
    revision: *const c_char,
    dry_run: i32,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_archive_export_async");
    let workspace = arg_str!(workspace, "loom_archive_export_async").to_string();
    let dst_path = arg_str!(dst_path, "loom_archive_export_async").to_string();
    let kind = arg_str!(kind, "loom_archive_export_async").to_string();
    let revision = match optional_revision(revision, "loom_archive_export_async") {
        Ok(revision) => revision,
        Err(e) => return fail(e),
    };
    let owned = task_handle(h);
    let dry_run = dry_run != 0;
    unsafe {
        spawn_task(
            out_task,
            Box::new(move || {
                archive_export_ns(&owned, &workspace, &dst_path, &kind, revision, dry_run)
            }),
        )
    }
}

/// Async form of [`loom_car_import`].
///
/// # Safety
/// `handle` must be from [`loom_open`]; `src_path` must be a valid C string; `out_task` must be
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_car_import_async(
    handle: *mut LoomSession,
    src_path: *const c_char,
    dry_run: i32,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_car_import_async");
    let src_path = arg_str!(src_path, "loom_car_import_async").to_string();
    let owned = task_handle(h);
    let dry_run = dry_run != 0;
    unsafe {
        spawn_task(
            out_task,
            Box::new(move || car_import_ns(&owned, &src_path, dry_run)),
        )
    }
}

/// Async form of [`loom_car_export`].
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out_task` must be
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_car_export_async(
    handle: *mut LoomSession,
    workspace: *const c_char,
    dst_path: *const c_char,
    dry_run: i32,
    out_task: *mut *mut LoomTask,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_car_export_async");
    let workspace = arg_str!(workspace, "loom_car_export_async").to_string();
    let dst_path = arg_str!(dst_path, "loom_car_export_async").to_string();
    let owned = task_handle(h);
    let dry_run = dry_run != 0;
    unsafe {
        spawn_task(
            out_task,
            Box::new(move || car_export_ns(&owned, &workspace, &dst_path, dry_run)),
        )
    }
}
