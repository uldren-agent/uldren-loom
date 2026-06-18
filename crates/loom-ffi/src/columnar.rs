//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_wire::columnar::{
    aggregates_from_cbor, columns_from_cbor, columns_to_cbor, digest_to_cbor, inspect_to_cbor,
    row_from_cbor, rows_to_cbor, select_columns_from_cbor, select_filter_from_cbor, values_to_cbor,
};

// ---------------------------------------------------------------------------------------------------
// Columnar dataset (Columnar facet) - typed, append-only segmented rows in a named dataset within a
// workspace. A column schema crosses as a CBOR array of `[name, type_tag]` (the `ColumnType` wire tag);
// a row crosses as a CBOR cell array (the shared Value cell codec); a scan/select crosses as a CBOR
// array of rows. The StateAccess `select` filter is the CBOR array `[column, op, value_cell]`, with op
// tags 0 eq, 1 ne, 2 lt, 3 le, 4 gt, 5 ge; an empty filter buffer scans every row.
// ---------------------------------------------------------------------------------------------------

fn ensure_columnar_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Columnar,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Columnar)?;
    Ok(ns)
}

/// Create columnar dataset `name` with `columns` (a CBOR array of `[name, type_tag]`) and
/// `target_segment_rows` (0 for the engine default) in workspace `workspace` (UUID or name, created with
/// the `columnar` facet if absent). `CONFLICT` if the dataset already exists. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `columns` null or
/// `columns_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_columnar_create(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    columns: *const c_uchar,
    columns_len: usize,
    target_segment_rows: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_columnar_create");
    let workspace = arg_str!(workspace, "loom_columnar_create");
    let name = arg_str!(name, "loom_columnar_create");
    // SAFETY: caller guarantees `(columns, columns_len)` is readable/null (see docs).
    let columns_bytes = unsafe { byte_slice(columns, columns_len) };
    let result = (|| -> LoomResult<()> {
        let columns = columns_from_cbor(columns_bytes)?;
        let mut loom = open_h_write(h)?;
        let ns = ensure_columnar_ns(&mut loom, workspace)?;
        loom_core::columnar_create(&mut loom, ns, name, columns, target_segment_rows)?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Append `row` (a CBOR cell array) to dataset `name`, validating arity + column types. `NOT_FOUND` if
/// the dataset was never created; `INVALID_ARGUMENT` on an arity or type mismatch. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `row` null or `row_len`
/// readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_columnar_append(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    row: *const c_uchar,
    row_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_columnar_append");
    let workspace = arg_str!(workspace, "loom_columnar_append");
    let name = arg_str!(name, "loom_columnar_append");
    // SAFETY: caller guarantees `(row, row_len)` is readable/null (see docs).
    let row_bytes = unsafe { byte_slice(row, row_len) };
    let result = (|| -> LoomResult<()> {
        let row = row_from_cbor(row_bytes)?;
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom_core::columnar_append(&mut loom, ns, name, row)?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// All rows of dataset `name` in append order as a CBOR array of cell arrays. Writes owned bytes to
/// `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and returns `0`. `NOT_FOUND` if the dataset
/// does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_columnar_scan_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_columnar_scan_cbor");
    let workspace = arg_str!(workspace, "loom_columnar_scan_cbor");
    let name = arg_str!(name, "loom_columnar_scan_cbor");
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(rows_to_cbor(loom_core::columnar_scan(&loom, ns, name)?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// The `(name, type_tag)` columns of dataset `name` as a CBOR array of `[name, type_tag]`. Writes owned
/// bytes to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and returns `0`. `NOT_FOUND` if the
/// dataset does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_columnar_columns_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_columnar_columns_cbor");
    let workspace = arg_str!(workspace, "loom_columnar_columns_cbor");
    let name = arg_str!(name, "loom_columnar_columns_cbor");
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(columns_to_cbor(loom_core::columnar_columns(
            &loom, ns, name,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// The total row count of dataset `name`, written to `*out_count`. Returns `0`. `NOT_FOUND` if the
/// dataset does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_columnar_rows(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_count: *mut u64,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_columnar_rows");
    let workspace = arg_str!(workspace, "loom_columnar_rows");
    let name = arg_str!(name, "loom_columnar_rows");
    let result = (|| -> LoomResult<usize> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom_core::columnar_rows(&loom, ns, name)
    })();
    match result {
        Ok(count) => {
            if !out_count.is_null() {
                // SAFETY: `out_count` is writable per fn docs.
                unsafe { *out_count = count as u64 };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Re-chunk dataset `name` at its target segment size. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_columnar_compact(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_columnar_compact");
    let workspace = arg_str!(workspace, "loom_columnar_compact");
    let name = arg_str!(name, "loom_columnar_compact");
    let result = (|| -> LoomResult<()> {
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom_core::columnar_compact(&mut loom, ns, name)?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Inspect dataset `name` as CBOR `[columns, rows, segment_count, target_segment_rows, source_digest]`.
/// Writes owned bytes to `(*out_ptr, *out_len)` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_columnar_inspect_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_columnar_inspect_cbor");
    let workspace = arg_str!(workspace, "loom_columnar_inspect_cbor");
    let name = arg_str!(name, "loom_columnar_inspect_cbor");
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(inspect_to_cbor(loom_core::columnar_inspect(
            &loom, ns, name,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Return the source digest for dataset `name` as a CBOR text value. Writes owned bytes to
/// `(*out_ptr, *out_len)` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_columnar_source_digest_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_columnar_source_digest_cbor");
    let workspace = arg_str!(workspace, "loom_columnar_source_digest_cbor");
    let name = arg_str!(name, "loom_columnar_source_digest_cbor");
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(digest_to_cbor(loom_core::columnar_source_digest(
            &loom, ns, name,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Project `columns` (a CBOR array of text) from dataset `name`'s rows matching `filter` as a CBOR array
/// of cell arrays. The filter is the CBOR array `[column, op, value_cell]` (op: 0 eq, 1 ne, 2 lt, 3 le,
/// 4 gt, 5 ge); an empty filter buffer scans every row. Writes owned bytes to `(*out_ptr, *out_len)`
/// (free with [`loom_bytes_free`]) and returns `0`. `NOT_FOUND` if the dataset does not exist;
/// `INVALID_ARGUMENT` on an unknown column.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `columns`/`filter` null or
/// their `_len` bytes readable; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_columnar_select_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    columns: *const c_uchar,
    columns_len: usize,
    filter: *const c_uchar,
    filter_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_columnar_select_cbor");
    let workspace = arg_str!(workspace, "loom_columnar_select_cbor");
    let name = arg_str!(name, "loom_columnar_select_cbor");
    // SAFETY: caller guarantees `(columns, columns_len)` and `(filter, filter_len)` are readable/null.
    let columns_bytes = unsafe { byte_slice(columns, columns_len) };
    let filter_bytes = unsafe { byte_slice(filter, filter_len) };
    let result = (|| -> LoomResult<Vec<u8>> {
        let column_names = select_columns_from_cbor(columns_bytes)?;
        let filter = select_filter_from_cbor(filter_bytes)?;
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        let col_refs: Vec<&str> = column_names.iter().map(String::as_str).collect();
        let filter_ref = filter.as_ref().map(|(c, op, v)| (c.as_str(), *op, v));
        Ok(rows_to_cbor(loom_core::columnar_select(
            &loom, ns, name, &col_refs, filter_ref,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Evaluate aggregate expressions over dataset `name` as a CBOR cell array. `aggregates` is a CBOR
/// array of `[op, column?]` expressions, with op tags: 0 count, 1 count-non-null, 2 min, 3 max, 4 sum.
/// Count may omit `column` or set it to null. The optional filter uses the same CBOR shape as
/// [`loom_columnar_select_cbor`]. Writes owned bytes to `(*out_ptr, *out_len)` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `aggregates`/`filter` null
/// or their `_len` bytes readable; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_columnar_aggregate_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    aggregates: *const c_uchar,
    aggregates_len: usize,
    filter: *const c_uchar,
    filter_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_columnar_aggregate_cbor");
    let workspace = arg_str!(workspace, "loom_columnar_aggregate_cbor");
    let name = arg_str!(name, "loom_columnar_aggregate_cbor");
    // SAFETY: caller guarantees `(aggregates, aggregates_len)` and `(filter, filter_len)` are readable/null.
    let aggregates_bytes = unsafe { byte_slice(aggregates, aggregates_len) };
    let filter_bytes = unsafe { byte_slice(filter, filter_len) };
    let result = (|| -> LoomResult<Vec<u8>> {
        let aggregates = aggregates_from_cbor(aggregates_bytes)?;
        let filter = select_filter_from_cbor(filter_bytes)?;
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        let filter_ref = filter.as_ref().map(|(c, op, v)| (c.as_str(), *op, v));
        Ok(values_to_cbor(loom_core::columnar_aggregate(
            &loom,
            ns,
            name,
            &aggregates,
            filter_ref,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}
