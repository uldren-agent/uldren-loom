//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::{Document, FieldValue, Mapping, QueryRequest, QueryResponse};

fn ensure_search_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Search,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Search)?;
    Ok(ns)
}

fn mapping_from_cbor(bytes: &[u8]) -> LoomResult<Mapping> {
    loom_core::search_mapping_from_cbor(bytes)
}

fn field_value_cbor(value: &FieldValue) -> CborValue {
    match value {
        FieldValue::Text(s) => CborValue::Text(s.clone()),
        FieldValue::Bytes(b) => CborValue::Bytes(b.clone()),
    }
}

fn document_to_cbor(doc: &Document) -> Vec<u8> {
    let pairs = doc
        .iter()
        .map(|(field, v)| (CborValue::Text(field.clone()), field_value_cbor(v)))
        .collect();
    cbor_encode(&CborValue::Map(pairs)).unwrap_or_default()
}

fn document_from_cbor(bytes: &[u8]) -> LoomResult<Document> {
    let value = loom_codec::decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))?;
    let CborValue::Map(pairs) = value else {
        return Err(LoomError::invalid("search document must be a CBOR map"));
    };
    let mut doc = Document::new();
    for (k, v) in pairs {
        let CborValue::Text(field) = k else {
            return Err(LoomError::invalid(
                "search document field name must be text",
            ));
        };
        let value = match v {
            CborValue::Text(s) => FieldValue::Text(s),
            CborValue::Bytes(b) => FieldValue::Bytes(b),
            _ => {
                return Err(LoomError::invalid(
                    "search document value must be text or bytes",
                ));
            }
        };
        doc.insert(field, value);
    }
    Ok(doc)
}

fn query_request_from_cbor(bytes: &[u8]) -> LoomResult<QueryRequest> {
    loom_core::search_request_from_cbor(bytes)
}

fn response_to_cbor(response: &QueryResponse) -> Vec<u8> {
    loom_core::search_response_cbor(response)
}

/// Create search collection `name` with the field `mapping` (CBOR `field -> [type_tag, stored, faceted]`)
/// in workspace `workspace` (UUID or name, created with the `search` facet if absent). `CONFLICT` if the
/// collection already exists. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `mapping` null or
/// `mapping_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_search_create(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    mapping: *const c_uchar,
    mapping_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_search_create");
    let workspace = arg_str!(workspace, "loom_search_create");
    let name = arg_str!(name, "loom_search_create");
    // SAFETY: caller guarantees `(mapping, mapping_len)` is readable/null (see docs).
    let mapping_bytes = unsafe { byte_slice(mapping, mapping_len) };
    let result = (|| -> LoomResult<()> {
        let mapping = mapping_from_cbor(mapping_bytes)?;
        let mut loom = open_h_write(h)?;
        let ns = ensure_search_ns(&mut loom, workspace)?;
        loom_core::search_create(&mut loom, ns, name, mapping)?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Insert or replace the document at `id` (opaque bytes) in collection `name`; `doc` is a CBOR
/// `field -> value` map (each value text or bytes). `NOT_FOUND` if the collection was never created.
/// Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `id`/`doc` null or their
/// `_len` bytes readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_search_index(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_uchar,
    id_len: usize,
    doc: *const c_uchar,
    doc_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_search_index");
    let workspace = arg_str!(workspace, "loom_search_index");
    let name = arg_str!(name, "loom_search_index");
    // SAFETY: caller guarantees `(id, id_len)` and `(doc, doc_len)` are readable/null (see docs).
    let id_bytes = unsafe { byte_slice(id, id_len) };
    let doc_bytes = unsafe { byte_slice(doc, doc_len) };
    let result = (|| -> LoomResult<()> {
        let doc = document_from_cbor(doc_bytes)?;
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom_core::search_index(&mut loom, ns, name, id_bytes.to_vec(), doc)?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Fetch the document at `id` in collection `name` as a CBOR `field -> value` map. On success returns
/// `0` and sets `*out_found`: present -> `1` and bytes at `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]); absent -> `0` and `(null, 0)`. `NOT_FOUND` if the collection does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `id` null or `id_len`
/// readable bytes; `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_search_get(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_uchar,
    id_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_search_get");
    let workspace = arg_str!(workspace, "loom_search_get");
    let name = arg_str!(name, "loom_search_get");
    // SAFETY: caller guarantees `(id, id_len)` is readable/null (see docs).
    let id_bytes = unsafe { byte_slice(id, id_len) };
    let result = (|| -> LoomResult<Option<Vec<u8>>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(loom_core::search_get(&loom, ns, name, id_bytes)?.map(|d| document_to_cbor(&d)))
    })();
    match result {
        Ok(Some(bytes)) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` is writable per fn docs.
                unsafe { *out_found = 1 };
            }
            // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
            unsafe { ok_bytes(out_ptr, out_len, bytes) }
        }
        Ok(None) => {
            // SAFETY: each non-null out-pointer is writable per fn docs.
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
        Err(e) => fail(e),
    }
}

/// Remove `id` from collection `name`; writes whether it was present (`1`/`0`) to `*out_found` and
/// returns `0`. `NOT_FOUND` if the collection does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `id` null or `id_len`
/// readable bytes; `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_search_delete(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_uchar,
    id_len: usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_search_delete");
    let workspace = arg_str!(workspace, "loom_search_delete");
    let name = arg_str!(name, "loom_search_delete");
    // SAFETY: caller guarantees `(id, id_len)` is readable/null (see docs).
    let id_bytes = unsafe { byte_slice(id, id_len) };
    let result = (|| -> LoomResult<bool> {
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        let present = loom_core::search_delete(&mut loom, ns, name, id_bytes)?;
        if present {
            save_loom(&mut loom)?;
        }
        Ok(present)
    })();
    match result {
        Ok(found) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` is writable per fn docs.
                unsafe { *out_found = i32::from(found) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Document ids in collection `name` as a CBOR array of byte strings. When `has_prefix != 0` only ids
/// starting with `(prefix, prefix_len)` are returned; otherwise every id is returned. Writes owned
/// bytes to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and returns `0`. `NOT_FOUND` if the
/// collection does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `prefix` null or `prefix_len`
/// readable bytes; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_search_ids_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    prefix: *const c_uchar,
    prefix_len: usize,
    has_prefix: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_search_ids_cbor");
    let workspace = arg_str!(workspace, "loom_search_ids_cbor");
    let name = arg_str!(name, "loom_search_ids_cbor");
    // SAFETY: caller guarantees `(prefix, prefix_len)` is readable/null (see docs).
    let prefix_bytes = unsafe { byte_slice(prefix, prefix_len) };
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        let prefix = (has_prefix != 0).then_some(prefix_bytes);
        Ok(loom_core::search_ids_cbor(loom_core::search_ids(
            &loom, ns, name, prefix,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Replace the field mapping of collection `name` (CBOR `field -> [type_tag, stored, faceted]`).
/// `NOT_FOUND` if the collection does not exist. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `mapping` null or
/// `mapping_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_search_remap(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    mapping: *const c_uchar,
    mapping_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_search_remap");
    let workspace = arg_str!(workspace, "loom_search_remap");
    let name = arg_str!(name, "loom_search_remap");
    // SAFETY: caller guarantees `(mapping, mapping_len)` is readable/null (see docs).
    let mapping_bytes = unsafe { byte_slice(mapping, mapping_len) };
    let result = (|| -> LoomResult<()> {
        let mapping = mapping_from_cbor(mapping_bytes)?;
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom_core::search_remap(&mut loom, ns, name, mapping)?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Run the portable linear-scan query over collection `name`. `request` is the CBOR array
/// `[query, limit, offset]` (`query` a recursive node: `[0, field, text]` match, `[1, field, value]`
/// term, `[2, field, [terms], slop]` phrase, `[3, field, lower, upper, incl_lower, incl_upper]` range,
/// `[4, [must], [should], [must_not]]` bool). The response is the CBOR array
/// `[reduced, [[id, score_cell, highlights] ...], facets, aggregations]`. Writes owned bytes to `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]) and returns `0`. `NOT_FOUND` if the collection does not exist; `NO_SUCH_FIELD`
/// for an unmapped query field.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `request` null or
/// `request_len` readable bytes; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_search_query_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    request: *const c_uchar,
    request_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_search_query_cbor");
    let workspace = arg_str!(workspace, "loom_search_query_cbor");
    let name = arg_str!(name, "loom_search_query_cbor");
    // SAFETY: caller guarantees `(request, request_len)` is readable/null (see docs).
    let request_bytes = unsafe { byte_slice(request, request_len) };
    let result = (|| -> LoomResult<Vec<u8>> {
        let request = query_request_from_cbor(request_bytes)?;
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(response_to_cbor(&loom_core::search_query(
            &loom, ns, name, &request,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}
