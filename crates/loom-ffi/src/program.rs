//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.
//!
//! C ABI for the `Program` family and `Store.digest_algo`. Program records are canonical CBOR
//! `[name, manifest_digest, body_digest, body_len, manifest]`; `program_get` returns `[record, body]`.
//! Manifests are the canonical `loom.compute.manifest` CBOR.

use super::*;
use loom_compute::{
    Manifest, ProgramBody, StoredProgram, program_get, program_inspect, program_list, program_put,
    program_remove,
};

fn ensure_program_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Program,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Program)?;
    Ok(ns)
}

/// Resolve `workspace` for a read; `Ok(None)` if the workspace does not exist (the read families treat
/// an absent workspace as an empty program set, matching the in-process client).
fn read_program_ns(loom: &Loom<FileStore>, workspace: &str) -> LoomResult<Option<WorkspaceId>> {
    match resolve_workspace_arg(loom, workspace) {
        Ok(ns) => Ok(Some(ns)),
        Err(error) if error.code == loom_core::Code::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn program_record_value(record: &StoredProgram) -> CborValue {
    CborValue::Array(vec![
        CborValue::Text(record.name.clone()),
        CborValue::Text(record.manifest_digest.to_string()),
        CborValue::Text(record.body_digest.to_string()),
        CborValue::Uint(record.body_len),
        CborValue::Bytes(record.manifest.encode()),
    ])
}

fn program_record_to_cbor(record: &StoredProgram) -> LoomResult<Vec<u8>> {
    cbor_encode(&program_record_value(record))
        .map_err(|e| LoomError::invalid(format!("encode program record: {e}")))
}

fn program_body_to_cbor(body: &ProgramBody) -> LoomResult<Vec<u8>> {
    cbor_encode(&CborValue::Array(vec![
        program_record_value(&body.record),
        CborValue::Bytes(body.body.clone()),
    ]))
    .map_err(|e| LoomError::invalid(format!("encode program body: {e}")))
}

/// Store `manifest`/`body` as program `name` in `workspace` (created with the `program` facet if
/// absent). Returns the stored record as canonical CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `manifest`/`body` null or
/// readable for their lengths; `out_ptr`/`out_len` writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_program_put(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    manifest: *const c_uchar,
    manifest_len: usize,
    body: *const c_uchar,
    body_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_program_put");
    let workspace = arg_str!(workspace, "loom_program_put");
    let name = arg_str!(name, "loom_program_put");
    let manifest = unsafe { byte_slice(manifest, manifest_len) };
    let body = unsafe { byte_slice(body, body_len) };
    let manifest = match Manifest::decode(manifest) {
        Some(manifest) => manifest,
        None => return fail(LoomError::invalid("malformed program manifest")),
    };
    let mut loom = match open_h_write(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match ensure_program_ns(&mut loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match program_put(&mut loom, ns, name, manifest, body).and_then(|stored| {
        save_loom(&mut loom)?;
        program_record_to_cbor(&stored)
    }) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(error) => fail(error),
    }
}

/// Fetch program `name`'s record (without body) as canonical CBOR. `*out_found` is `0` when absent.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; out pointers writable when
/// non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_program_inspect(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_program_inspect");
    let workspace = arg_str!(workspace, "loom_program_inspect");
    let name = arg_str!(name, "loom_program_inspect");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let record = match read_program_ns(&loom, workspace) {
        Ok(Some(ns)) => program_inspect(&loom, ns, name),
        Ok(None) => Ok(None),
        Err(error) => return fail(error),
    };
    program_optional_cbor(
        record.and_then(|r| r.as_ref().map(program_record_to_cbor).transpose()),
        out_ptr,
        out_len,
        out_found,
    )
}

/// Fetch program `name`'s `[record, body]` as canonical CBOR. `*out_found` is `0` when absent.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; out pointers writable when
/// non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_program_get(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_program_get");
    let workspace = arg_str!(workspace, "loom_program_get");
    let name = arg_str!(name, "loom_program_get");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let body = match read_program_ns(&loom, workspace) {
        Ok(Some(ns)) => program_get(&loom, ns, name),
        Ok(None) => Ok(None),
        Err(error) => return fail(error),
    };
    program_optional_cbor(
        body.and_then(|b| b.as_ref().map(program_body_to_cbor).transpose()),
        out_ptr,
        out_len,
        out_found,
    )
}

/// List all program records in `workspace` as canonical CBOR (an array of record arrays, name-sorted).
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` valid C string; `out_ptr`/`out_len` writable when
/// non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_program_list(
    handle: *mut LoomSession,
    workspace: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_program_list");
    let workspace = arg_str!(workspace, "loom_program_list");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let records = match read_program_ns(&loom, workspace) {
        Ok(Some(ns)) => match program_list(&loom, ns) {
            Ok(records) => records,
            Err(error) => return fail(error),
        },
        Ok(None) => Vec::new(),
        Err(error) => return fail(error),
    };
    let items = records.iter().map(program_record_value).collect();
    match cbor_encode(&CborValue::Array(items)) {
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(error) => fail(LoomError::invalid(format!("encode program list: {error}"))),
    }
}

/// Remove program `name` from `workspace`; `*out_removed` is `1` if a record existed, `0` otherwise.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `out_removed` writable when
/// non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_program_remove(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_removed: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_program_remove");
    let workspace = arg_str!(workspace, "loom_program_remove");
    let name = arg_str!(name, "loom_program_remove");
    let mut loom = match open_h_write(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let removed = match read_program_ns(&loom, workspace) {
        Ok(Some(ns)) => match program_remove(&mut loom, ns, name) {
            Ok(removed) => removed,
            Err(error) => return fail(error),
        },
        Ok(None) => false,
        Err(error) => return fail(error),
    };
    if removed && let Err(error) = save_loom(&mut loom) {
        return fail(error);
    }
    if !out_removed.is_null() {
        unsafe { *out_removed = i32::from(removed) };
    }
    0
}

/// The digest algorithm the served store uses (e.g. `"sha256"`), as an owned C string in `*out_algo`
/// (free with [`loom_string_free`]).
///
/// # Safety
/// `handle` must be from [`loom_open`]; `out_algo` writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_store_digest_algo(
    handle: *mut LoomSession,
    out_algo: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_store_digest_algo");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let algo = loom.store().digest_algo().as_str();
    let algo = match CString::new(algo) {
        Ok(algo) => algo,
        Err(_) => return fail(LoomError::invalid("digest algorithm contains NUL")),
    };
    if !out_algo.is_null() {
        unsafe { *out_algo = algo.into_raw() };
    }
    0
}

/// Emit an optional CBOR payload: `*out_found` is `1` and `(*out_ptr,*out_len)` an owned buffer when
/// present, `0` with cleared out pointers when absent.
fn program_optional_cbor(
    value: LoomResult<Option<Vec<u8>>>,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    match value {
        Ok(Some(bytes)) => {
            if !out_found.is_null() {
                unsafe { *out_found = 1 };
            }
            unsafe { ok_bytes(out_ptr, out_len, bytes) }
        }
        Ok(None) => {
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
