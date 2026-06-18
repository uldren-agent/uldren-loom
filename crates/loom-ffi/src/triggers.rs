//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.
//!
//! C ABI for the `Triggers` family. Bindings are canonical `loom.triggers.binding` CBOR; fire-log
//! records are canonical `loom.triggers.fire-record` CBOR. Trigger ids are the canonical Uuid string.

use super::*;

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

fn parse_trigger_id(id: &str) -> LoomResult<loom_core::TriggerId> {
    WorkspaceId::parse(id).map_err(|_| LoomError::invalid(format!("invalid trigger id {id:?}")))
}

/// Store (create or replace) a trigger binding from canonical CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string; `binding` must be null
/// or readable for `binding_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_trigger_put(
    handle: *mut LoomSession,
    workspace: *const c_char,
    binding: *const c_uchar,
    binding_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_trigger_put");
    let workspace = arg_str!(workspace, "loom_trigger_put");
    let binding = unsafe { byte_slice(binding, binding_len) };
    let binding = match loom_core::trigger_binding_from_cbor(binding) {
        Ok(binding) => binding,
        Err(error) => return fail(error),
    };
    let mut loom = match open_h_write(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match ensure_program_ns(&mut loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::trigger_put(&mut loom, ns, &binding).and_then(|()| {
        save_loom(&mut loom)?;
        Ok(())
    }) {
        Ok(()) => 0,
        Err(error) => fail(error),
    }
}

/// Fetch a trigger binding by id as canonical CBOR bytes.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`id` must be valid C strings;
/// `out_ptr`/`out_len`/`out_found` must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_trigger_get_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_trigger_get_cbor");
    let workspace = arg_str!(workspace, "loom_trigger_get_cbor");
    let id = arg_str!(id, "loom_trigger_get_cbor");
    let tid = match parse_trigger_id(id) {
        Ok(tid) => tid,
        Err(error) => return fail(error),
    };
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::trigger_get(&loom, ns, tid) {
        Ok(binding) => {
            if !out_found.is_null() {
                unsafe { *out_found = 1 };
            }
            match loom_core::trigger_binding_to_cbor(&binding) {
                Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
                Err(error) => fail(error),
            }
        }
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

/// List all trigger bindings in `workspace` as canonical CBOR: an array of binding-CBOR byte strings.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace` must be a valid C string; `out_ptr`/`out_len` must
/// be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_trigger_list_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_trigger_list_cbor");
    let workspace = arg_str!(workspace, "loom_trigger_list_cbor");
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::trigger_list(&loom, ns) {
        Ok(bindings) => {
            let items = match bindings
                .iter()
                .map(|binding| loom_core::trigger_binding_to_cbor(binding).map(CborValue::Bytes))
                .collect::<LoomResult<Vec<_>>>()
            {
                Ok(items) => items,
                Err(error) => return fail(error),
            };
            match cbor_encode(&CborValue::Array(items)) {
                Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
                Err(error) => fail(LoomError::invalid(format!(
                    "trigger list encoding failed: {error}"
                ))),
            }
        }
        Err(error) => fail(error),
    }
}

/// Enable or disable a trigger; returns the updated binding as canonical CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`id` must be valid C strings; `out_ptr`/`out_len`
/// must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_trigger_enable(
    handle: *mut LoomSession,
    workspace: *const c_char,
    id: *const c_char,
    enabled: bool,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_trigger_enable");
    let workspace = arg_str!(workspace, "loom_trigger_enable");
    let id = arg_str!(id, "loom_trigger_enable");
    let tid = match parse_trigger_id(id) {
        Ok(tid) => tid,
        Err(error) => return fail(error),
    };
    let mut loom = match open_h_write(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::trigger_enable(&mut loom, ns, tid, enabled).and_then(|binding| {
        save_loom(&mut loom)?;
        Ok(binding)
    }) {
        Ok(binding) => match loom_core::trigger_binding_to_cbor(&binding) {
            Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
            Err(error) => fail(error),
        },
        Err(error) => fail(error),
    }
}

/// Remove a trigger by id; `out_removed` is set to 1 if a binding existed, 0 otherwise.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`id` must be valid C strings; `out_removed` must be
/// writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_trigger_remove(
    handle: *mut LoomSession,
    workspace: *const c_char,
    id: *const c_char,
    out_removed: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_trigger_remove");
    let workspace = arg_str!(workspace, "loom_trigger_remove");
    let id = arg_str!(id, "loom_trigger_remove");
    let tid = match parse_trigger_id(id) {
        Ok(tid) => tid,
        Err(error) => return fail(error),
    };
    let mut loom = match open_h_write(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::trigger_remove(&mut loom, ns, tid).and_then(|removed| {
        save_loom(&mut loom)?;
        Ok(removed)
    }) {
        Ok(removed) => {
            if !out_removed.is_null() {
                unsafe { *out_removed = i32::from(removed) };
            }
            0
        }
        Err(error) => fail(error),
    }
}

/// The fire-log history of a trigger from `from_seq` (up to `limit`) as canonical CBOR: an array of
/// fire-record-CBOR byte strings.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`id` must be valid C strings; `out_ptr`/`out_len`
/// must be writable when non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_trigger_history_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    id: *const c_char,
    from_seq: u64,
    limit: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_trigger_history_cbor");
    let workspace = arg_str!(workspace, "loom_trigger_history_cbor");
    let id = arg_str!(id, "loom_trigger_history_cbor");
    let tid = match parse_trigger_id(id) {
        Ok(tid) => tid,
        Err(error) => return fail(error),
    };
    let loom = match open_h_read(h) {
        Ok(loom) => loom,
        Err(error) => return fail(error),
    };
    let ns = match resolve_workspace_arg(&loom, workspace) {
        Ok(ns) => ns,
        Err(error) => return fail(error),
    };
    match loom_core::trigger_history(&loom, ns, tid, from_seq, limit as usize) {
        Ok(records) => {
            let items = match records
                .iter()
                .map(|record| loom_core::fire_record_to_cbor(record).map(CborValue::Bytes))
                .collect::<LoomResult<Vec<_>>>()
            {
                Ok(items) => items,
                Err(error) => return fail(error),
            };
            match cbor_encode(&CborValue::Array(items)) {
                Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
                Err(error) => fail(LoomError::invalid(format!(
                    "trigger history encoding failed: {error}"
                ))),
            }
        }
        Err(error) => fail(error),
    }
}
