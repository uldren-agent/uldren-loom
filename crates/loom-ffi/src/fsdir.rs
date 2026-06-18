//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.
//!
//! C ABI for the `FileSystem` directory/metadata surface. `create_directory`/`remove_directory` mutate
//! the working tree; `stat` returns canonical CBOR `loom.fs.stat.v1` (`[path, kind, size, mode]`) and
//! `list_directory` returns canonical CBOR `loom.fs.dir-listing.v1` (an array of `[name, kind]`).

use super::*;

fn ensure_files_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Files,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Files)?;
    Ok(ns)
}

/// Record `path` as a directory in `workspace` (created with the `files` facet if absent). Idempotent if
/// `path` is already a directory; `recursive` also creates missing parents.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`path` must be valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_fs_create_directory(
    handle: *mut LoomSession,
    workspace: *const c_char,
    path: *const c_char,
    recursive: bool,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_fs_create_directory");
    let workspace = arg_str!(workspace, "loom_fs_create_directory");
    let path = arg_str!(path, "loom_fs_create_directory");
    let mut loom = match open_h_write(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match ensure_files_ns(&mut loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom.create_directory(ns, path, recursive).and_then(|()| {
        save_loom(&mut loom)?;
        Ok(())
    }) {
        Ok(()) => 0,
        Err(error) => fail(error),
    }
}

/// Delete directory `path` in `workspace`. Without `recursive` a non-empty directory is
/// `INVALID_ARGUMENT` and an absent directory is `NOT_FOUND`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`path` must be valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_fs_remove_directory(
    handle: *mut LoomSession,
    workspace: *const c_char,
    path: *const c_char,
    recursive: bool,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_fs_remove_directory");
    let workspace = arg_str!(workspace, "loom_fs_remove_directory");
    let path = arg_str!(path, "loom_fs_remove_directory");
    let mut loom = match open_h_write(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom.remove_directory(ns, path, recursive).and_then(|()| {
        save_loom(&mut loom)?;
        Ok(())
    }) {
        Ok(()) => 0,
        Err(error) => fail(error),
    }
}

/// Metadata for `path` in `workspace` as canonical CBOR `loom.fs.stat.v1` (`[path, kind, size, mode]`).
/// A path that resolves to neither a file nor a directory is `NOT_FOUND` (`*out_found` set to `0`).
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`path` valid C strings; `out_ptr`/`out_len`/
/// `out_found` writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_fs_stat(
    handle: *mut LoomSession,
    workspace: *const c_char,
    path: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_fs_stat");
    let workspace = arg_str!(workspace, "loom_fs_stat");
    let path = arg_str!(path, "loom_fs_stat");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom.stat(ns, path) {
        Ok(stat) => match loom_wire::fs::fs_stat_to_cbor(&stat) {
            Ok(bytes) => {
                if !out_found.is_null() {
                    unsafe { *out_found = 1 };
                }
                unsafe { ok_bytes(out_ptr, out_len, bytes) }
            }
            Err(error) => fail(error),
        },
        Err(error) if error.code == loom_core::Code::NotFound => {
            unsafe {
                if !out_found.is_null() {
                    *out_found = 0;
                }
                if !out_ptr.is_null() {
                    *out_ptr = core::ptr::null_mut();
                }
                if !out_len.is_null() {
                    *out_len = 0;
                }
            }
            0
        }
        Err(error) => fail(error),
    }
}

/// The immediate children of directory `path` in `workspace` as canonical CBOR
/// `loom.fs.dir-listing.v1` (an array of `[name, kind]`, sorted by name; root is `""` or `"/"`). A
/// non-directory path is `NOT_FOUND`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`path` valid C strings; `out_ptr`/`out_len` writable
/// when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_fs_list_directory(
    handle: *mut LoomSession,
    workspace: *const c_char,
    path: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_fs_list_directory");
    let workspace = arg_str!(workspace, "loom_fs_list_directory");
    let path = arg_str!(path, "loom_fs_list_directory");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom.list_directory(ns, path) {
        Ok(entries) => match loom_wire::fs::dir_listing_to_cbor(&entries) {
            Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
            Err(error) => fail(error),
        },
        Err(error) => fail(error),
    }
}
