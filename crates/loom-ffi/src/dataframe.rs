//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use loom_core::tabular::cell_value;

fn ensure_dataframe_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Dataframe,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Dataframe)?;
    Ok(ns)
}

fn dataframe_batch_cbor(batch: loom_core::DataframeBatch) -> LoomResult<Vec<u8>> {
    let columns = batch
        .columns
        .into_iter()
        .map(|column| {
            CborValue::Array(vec![
                CborValue::Text(column.name),
                CborValue::Uint(u64::from(column.column_type.tag())),
                CborValue::Bool(column.nullable),
            ])
        })
        .collect::<Vec<_>>();
    let rows = batch
        .rows
        .into_iter()
        .map(|row| CborValue::Array(row.iter().map(cell_value).collect()))
        .collect::<Vec<_>>();
    cbor_encode(&CborValue::Array(vec![
        CborValue::Array(columns),
        CborValue::Array(rows),
    ]))
    .map_err(|e| LoomError::corrupt(format!("cbor: {e}")))
}

/// Create dataframe frame `name` from canonical `DataframePlan` CBOR in workspace `workspace`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `plan` null or
/// `plan_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_dataframe_create(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    plan: *const c_uchar,
    plan_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_dataframe_create");
    let workspace = arg_str!(workspace, "loom_dataframe_create");
    let name = arg_str!(name, "loom_dataframe_create");
    // SAFETY: caller guarantees `(plan, plan_len)` is readable/null (see docs).
    let plan_bytes = unsafe { byte_slice(plan, plan_len) };
    let result = (|| -> LoomResult<()> {
        let plan = loom_core::DataframePlan::decode(plan_bytes)?;
        let mut loom = open_h_write(h)?;
        let ns = ensure_dataframe_ns(&mut loom, workspace)?;
        loom_core::dataframe_create(&mut loom, ns, name, &plan)?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Execute dataframe frame `name` and return `[columns, rows]` as canonical CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `out_ptr`/`out_len`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_dataframe_collect_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_dataframe_collect_cbor");
    let workspace = arg_str!(workspace, "loom_dataframe_collect_cbor");
    let name = arg_str!(name, "loom_dataframe_collect_cbor");
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        dataframe_batch_cbor(loom_core::dataframe_collect(&loom, ns, name)?)
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Execute dataframe frame `name` and return at most `rows` rows as `[columns, rows]` canonical CBOR.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `out_ptr`/`out_len`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_dataframe_preview_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    rows: u64,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_dataframe_preview_cbor");
    let workspace = arg_str!(workspace, "loom_dataframe_preview_cbor");
    let name = arg_str!(name, "loom_dataframe_preview_cbor");
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        dataframe_batch_cbor(loom_core::dataframe_preview(&loom, ns, name, rows)?)
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Materialize dataframe frame `name`. A CAS materialization writes a digest string; other
/// materialization targets report no digest.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `out_digest` and
/// `out_has_digest` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_dataframe_materialize(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_digest: *mut *mut c_char,
    out_has_digest: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_dataframe_materialize");
    let workspace = arg_str!(workspace, "loom_dataframe_materialize");
    let name = arg_str!(name, "loom_dataframe_materialize");
    let result = (|| -> LoomResult<Option<Digest>> {
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        let digest = loom_core::dataframe_materialize(&mut loom, ns, name)?;
        save_loom(&mut loom)?;
        Ok(digest)
    })();
    match result {
        Ok(Some(digest)) => {
            if !out_has_digest.is_null() {
                // SAFETY: `out_has_digest` is writable per fn docs.
                unsafe { *out_has_digest = 1 };
            }
            // SAFETY: `out_digest` is writable per fn docs.
            unsafe { ok_str(out_digest, &digest.to_string()) }
        }
        Ok(None) => {
            // SAFETY: each non-null out-pointer is writable per fn docs.
            unsafe {
                if !out_has_digest.is_null() {
                    *out_has_digest = 0;
                }
                if !out_digest.is_null() {
                    *out_digest = core::ptr::null_mut();
                }
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Return the canonical dataframe plan digest as an `algo:hex` string.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_dataframe_plan_digest(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_dataframe_plan_digest");
    let workspace = arg_str!(workspace, "loom_dataframe_plan_digest");
    let name = arg_str!(name, "loom_dataframe_plan_digest");
    let result = (|| -> LoomResult<Digest> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom_core::dataframe_plan_digest(&loom, ns, name)
    })();
    match result {
        // SAFETY: `out` is writable per fn docs.
        Ok(digest) => unsafe { ok_str(out, &digest.to_string()) },
        Err(e) => fail(e),
    }
}

/// Return source digests pinned in the dataframe plan as a canonical CBOR array of `algo:hex` strings.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `out_ptr`/`out_len`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_dataframe_source_digests_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_dataframe_source_digests_cbor");
    let workspace = arg_str!(workspace, "loom_dataframe_source_digests_cbor");
    let name = arg_str!(name, "loom_dataframe_source_digests_cbor");
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom_wire::digest_list_to_cbor(loom_core::dataframe_source_digests(&loom, ns, name)?)
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}
