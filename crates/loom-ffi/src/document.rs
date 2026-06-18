//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use loom_core::document::{document_put_binary_with_entity_tag, document_put_text_with_entity_tag};

// ---------------------------------------------------------------------------------------------------
// Document (Document facet) - text and binary document operations over string-id collections.
// ---------------------------------------------------------------------------------------------------

fn ensure_doc_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Document,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Document)?;
    Ok(ns)
}

fn optional_entity_tag_arg(value: *const c_char, what: &str) -> LoomResult<Option<String>> {
    if value.is_null() {
        return Ok(None);
    }
    // SAFETY: caller guarantees non-null `value` is a valid C string.
    let Some(value) = (unsafe { cstr(value) }) else {
        return Err(LoomError::invalid(format!(
            "{what}: non-UTF-8 expected_entity_tag"
        )));
    };
    loom_core::parse_document_entity_tag(value)?;
    Ok(Some(value.to_string()))
}

fn owned_c_string(value: &str, what: &str) -> LoomResult<CString> {
    CString::new(value).map_err(|_| LoomError::invalid(format!("{what}: text contains NUL")))
}

fn doc_put_text_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    id: &str,
    text: &str,
    expected_entity_tag: Option<&str>,
) -> LoomResult<loom_core::document::DocumentPutResult> {
    let mut loom = open_h_write(h)?;
    let ns = ensure_doc_ns(&mut loom, workspace)?;
    let result = document_put_text_with_entity_tag(
        &mut loom,
        ns,
        collection,
        id,
        text,
        expected_entity_tag,
    )?;
    save_loom(&mut loom)?;
    Ok(result)
}

fn doc_put_binary_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    id: &str,
    bytes: &[u8],
    expected_entity_tag: Option<&str>,
) -> LoomResult<loom_core::document::DocumentPutResult> {
    let mut loom = open_h_write(h)?;
    let ns = ensure_doc_ns(&mut loom, workspace)?;
    let result = document_put_binary_with_entity_tag(
        &mut loom,
        ns,
        collection,
        id,
        bytes.to_vec(),
        expected_entity_tag,
    )?;
    save_loom(&mut loom)?;
    Ok(result)
}

fn doc_get_text_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    id: &str,
) -> LoomResult<Option<(String, Digest, String)>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(document_get_text(&loom, ns, collection, id)?
        .map(|document| (document.text, document.digest, document.entity_tag)))
}

fn doc_get_binary_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    id: &str,
) -> LoomResult<Option<(Vec<u8>, Digest, String)>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(document_get_binary(&loom, ns, collection, id)?
        .map(|document| (document.bytes, document.digest, document.entity_tag)))
}

fn doc_delete_ns(h: &LoomSession, workspace: &str, collection: &str, id: &str) -> LoomResult<bool> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let present = doc_delete(&mut loom, ns, collection, id)?;
    if present {
        save_loom(&mut loom)?;
    }
    Ok(present)
}

fn doc_list_binary_cbor_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    document_list_binary(&loom, ns, collection)
}

fn doc_index_create_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    name: &str,
    path: &str,
    unique: bool,
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let field_path = DocumentFieldPath::dotted(path)?;
    doc_create_index(
        &mut loom,
        ns,
        collection,
        DocumentIndexDef::new(name, field_path, unique)?,
    )?;
    save_loom(&mut loom)?;
    Ok(())
}

fn doc_index_create_json_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    declaration_json: &[u8],
) -> LoomResult<()> {
    let value = serde_json::from_slice::<serde_json::Value>(declaration_json)
        .map_err(|err| LoomError::invalid(err.to_string()))?;
    let declaration = loom_core::document_index_declaration_from_json(&value)?;
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_core::doc_create_index_declaration(&mut loom, ns, collection, declaration)?;
    save_loom(&mut loom)?;
    Ok(())
}

fn doc_index_drop_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    name: &str,
) -> LoomResult<bool> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let dropped = doc_drop_index(&mut loom, ns, collection, name)?;
    if dropped {
        save_loom(&mut loom)?;
    }
    Ok(dropped)
}

fn doc_index_rebuild_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    name: &str,
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    doc_rebuild_index(&mut loom, ns, collection, name)?;
    save_loom(&mut loom)?;
    Ok(())
}

fn doc_index_list_json_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    serde_json::to_vec(&loom_core::document_index_declarations_json(
        loom_core::doc_list_index_declarations(&loom, ns, collection)?,
    ))
    .map_err(|err| LoomError::invalid(err.to_string()))
}

fn doc_index_status_json_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    serde_json::to_vec(&document_index_statuses_json(doc_index_statuses(
        &loom, ns, collection,
    )?))
    .map_err(|err| LoomError::invalid(err.to_string()))
}

fn doc_find_json_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    index: &str,
    value_json: &[u8],
) -> LoomResult<Vec<u8>> {
    let value = serde_json::from_slice::<serde_json::Value>(value_json)
        .map_err(|err| LoomError::invalid(err.to_string()))?;
    let value = document_index_value_from_json(&value)?;
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    serde_json::to_vec(&document_ids_json(doc_find(
        &loom, ns, collection, index, &value,
    )?))
    .map_err(|err| LoomError::invalid(err.to_string()))
}

fn doc_query_json_ns(
    h: &LoomSession,
    workspace: &str,
    collection: &str,
    query_json: &[u8],
) -> LoomResult<Vec<u8>> {
    let query = serde_json::from_slice::<serde_json::Value>(query_json)
        .map_err(|err| LoomError::invalid(err.to_string()))?;
    let query = document_query_from_json(&query)?;
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    serde_json::to_vec(&document_query_result_json(doc_query(
        &loom, ns, collection, &query,
    )?))
    .map_err(|err| LoomError::invalid(err.to_string()))
}

/// Put UTF-8 text at string `id` in collection `collection`. `expected_entity_tag` is optional; when
/// non-null it must match the current document entity tag. Writes the new content digest and entity
/// tag as owned C strings to `*out_digest` and `*out_entity_tag` (free with [`loom_string_free`]).
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out_digest` is
/// null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_put_text(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    id: *const c_char,
    text: *const c_char,
    expected_entity_tag: *const c_char,
    out_digest: *mut *mut c_char,
    out_entity_tag: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_put_text");
    let workspace = arg_str!(workspace, "loom_doc_put_text");
    let collection = arg_str!(collection, "loom_doc_put_text");
    let id = arg_str!(id, "loom_doc_put_text");
    let text = arg_str!(text, "loom_doc_put_text");
    let expected_entity_tag =
        match optional_entity_tag_arg(expected_entity_tag, "loom_doc_put_text") {
            Ok(expected_entity_tag) => expected_entity_tag,
            Err(e) => return fail(e),
        };
    match doc_put_text_ns(
        h,
        workspace,
        collection,
        id,
        text,
        expected_entity_tag.as_deref(),
    ) {
        Ok(result) => {
            let digest = result.digest.to_string();
            let digest = match owned_c_string(&digest, "loom_doc_put_text") {
                Ok(digest) => digest,
                Err(e) => return fail(e),
            };
            let entity_tag = match owned_c_string(&result.entity_tag, "loom_doc_put_text") {
                Ok(entity_tag) => entity_tag,
                Err(e) => return fail(e),
            };
            if !out_digest.is_null() {
                // SAFETY: `out_digest` writable per docs.
                unsafe { *out_digest = digest.into_raw() };
            }
            if !out_entity_tag.is_null() {
                // SAFETY: `out_entity_tag` writable per docs.
                unsafe { *out_entity_tag = entity_tag.into_raw() };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Fetch `id` as UTF-8 text. Invalid stored UTF-8 returns `DOCUMENT_NOT_TEXT`. On success, writes
/// presence to `*out_found`; when present, `*out_text`, `*out_digest`, and `*out_entity_tag` are
/// owned C strings to free with [`loom_string_free`].
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; out pointers are
/// null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_get_text(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    id: *const c_char,
    out_text: *mut *mut c_char,
    out_digest: *mut *mut c_char,
    out_entity_tag: *mut *mut c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_get_text");
    let workspace = arg_str!(workspace, "loom_doc_get_text");
    let collection = arg_str!(collection, "loom_doc_get_text");
    let id = arg_str!(id, "loom_doc_get_text");
    match doc_get_text_ns(h, workspace, collection, id) {
        Ok(Some((text, digest, entity_tag))) => {
            let text = match owned_c_string(&text, "loom_doc_get_text") {
                Ok(text) => text,
                Err(e) => return fail(e),
            };
            let digest = digest.to_string();
            let digest = match owned_c_string(&digest, "loom_doc_get_text") {
                Ok(digest) => digest,
                Err(e) => return fail(e),
            };
            let entity_tag = match owned_c_string(&entity_tag, "loom_doc_get_text") {
                Ok(entity_tag) => entity_tag,
                Err(e) => return fail(e),
            };
            // SAFETY: each non-null out pointer is writable per docs.
            unsafe {
                if !out_found.is_null() {
                    *out_found = 1;
                }
                if !out_text.is_null() {
                    *out_text = text.into_raw();
                }
                if !out_digest.is_null() {
                    *out_digest = digest.into_raw();
                }
                if !out_entity_tag.is_null() {
                    *out_entity_tag = entity_tag.into_raw();
                }
            }
            0
        }
        Ok(None) => {
            // SAFETY: each non-null out pointer is writable per docs.
            unsafe {
                if !out_found.is_null() {
                    *out_found = 0;
                }
                if !out_text.is_null() {
                    *out_text = core::ptr::null_mut();
                }
                if !out_digest.is_null() {
                    *out_digest = core::ptr::null_mut();
                }
                if !out_entity_tag.is_null() {
                    *out_entity_tag = core::ptr::null_mut();
                }
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Put binary bytes at string `id` in collection `collection`. `expected_entity_tag` is optional;
/// when non-null it must match the current document entity tag. Writes the new content digest and
/// entity tag as owned C strings to `*out_digest` and `*out_entity_tag` (free with
/// [`loom_string_free`]).
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `value` null or
/// `value_len` readable bytes; out pointers are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_put_binary(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    id: *const c_char,
    value: *const c_uchar,
    value_len: usize,
    expected_entity_tag: *const c_char,
    out_digest: *mut *mut c_char,
    out_entity_tag: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_put_binary");
    let workspace = arg_str!(workspace, "loom_doc_put_binary");
    let collection = arg_str!(collection, "loom_doc_put_binary");
    let id = arg_str!(id, "loom_doc_put_binary");
    // SAFETY: caller guarantees `(value, value_len)` is readable/null (see docs).
    let value = unsafe { byte_slice(value, value_len) };
    let expected_entity_tag =
        match optional_entity_tag_arg(expected_entity_tag, "loom_doc_put_binary") {
            Ok(expected_entity_tag) => expected_entity_tag,
            Err(e) => return fail(e),
        };
    match doc_put_binary_ns(
        h,
        workspace,
        collection,
        id,
        value,
        expected_entity_tag.as_deref(),
    ) {
        Ok(result) => {
            let digest = result.digest.to_string();
            let digest = match owned_c_string(&digest, "loom_doc_put_binary") {
                Ok(digest) => digest,
                Err(e) => return fail(e),
            };
            let entity_tag = match owned_c_string(&result.entity_tag, "loom_doc_put_binary") {
                Ok(entity_tag) => entity_tag,
                Err(e) => return fail(e),
            };
            if !out_digest.is_null() {
                // SAFETY: `out_digest` writable per docs.
                unsafe { *out_digest = digest.into_raw() };
            }
            if !out_entity_tag.is_null() {
                // SAFETY: `out_entity_tag` writable per docs.
                unsafe { *out_entity_tag = entity_tag.into_raw() };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Fetch binary bytes at `id`. On success, writes presence to `*out_found`; when present,
/// `(*out_ptr, *out_len)` is an owned byte buffer freeable with [`loom_bytes_free`] and `*out_digest`
/// and `*out_entity_tag` are owned C strings freeable with [`loom_string_free`].
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; out pointers are
/// null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_get_binary(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_digest: *mut *mut c_char,
    out_entity_tag: *mut *mut c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_get_binary");
    let workspace = arg_str!(workspace, "loom_doc_get_binary");
    let collection = arg_str!(collection, "loom_doc_get_binary");
    let id = arg_str!(id, "loom_doc_get_binary");
    match doc_get_binary_ns(h, workspace, collection, id) {
        Ok(Some((bytes, digest, entity_tag))) => {
            let digest = digest.to_string();
            let digest = match owned_c_string(&digest, "loom_doc_get_binary") {
                Ok(digest) => digest,
                Err(e) => return fail(e),
            };
            let entity_tag = match owned_c_string(&entity_tag, "loom_doc_get_binary") {
                Ok(entity_tag) => entity_tag,
                Err(e) => return fail(e),
            };
            // SAFETY: `out_ptr`/`out_len` writable per docs.
            let status = unsafe { ok_bytes(out_ptr, out_len, bytes) };
            if status != 0 {
                return status;
            }
            // SAFETY: each non-null out pointer is writable per docs.
            unsafe {
                if !out_found.is_null() {
                    *out_found = 1;
                }
                if !out_digest.is_null() {
                    *out_digest = digest.into_raw();
                }
                if !out_entity_tag.is_null() {
                    *out_entity_tag = entity_tag.into_raw();
                }
            }
            0
        }
        Ok(None) => {
            // SAFETY: each non-null out pointer is writable per docs.
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
                if !out_digest.is_null() {
                    *out_digest = core::ptr::null_mut();
                }
                if !out_entity_tag.is_null() {
                    *out_entity_tag = core::ptr::null_mut();
                }
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// List collection `collection` as the Loom Canonical CBOR array of `[id, doc]` pairs in id order.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection` valid C strings; `out_ptr`/`out_len`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_list_binary_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_list_binary_cbor");
    let workspace = arg_str!(workspace, "loom_doc_list_binary_cbor");
    let collection = arg_str!(collection, "loom_doc_list_binary_cbor");
    match doc_list_binary_cbor_ns(h, workspace, collection) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Remove `id` from collection `collection`; writes presence (`1`/`0`) to `*out_found` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`collection`/`id` valid C strings; `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_delete(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    id: *const c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_delete");
    let workspace = arg_str!(workspace, "loom_doc_delete");
    let collection = arg_str!(collection, "loom_doc_delete");
    let id = arg_str!(id, "loom_doc_delete");
    match doc_delete_ns(h, workspace, collection, id) {
        Ok(found) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` writable per docs.
                unsafe { *out_found = i32::from(found) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Create a native document index over a dotted JSON field path.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_index_create(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    name: *const c_char,
    path: *const c_char,
    unique: i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_index_create");
    let workspace = arg_str!(workspace, "loom_doc_index_create");
    let collection = arg_str!(collection, "loom_doc_index_create");
    let name = arg_str!(name, "loom_doc_index_create");
    let path = arg_str!(path, "loom_doc_index_create");
    match doc_index_create_ns(h, workspace, collection, name, path, unique != 0) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Create a native document index from a full `DocumentIndexDeclaration` JSON object.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments and declaration byte pointers must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_index_create_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    declaration_json: *const c_uchar,
    declaration_json_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_index_create_json");
    let workspace = arg_str!(workspace, "loom_doc_index_create_json");
    let collection = arg_str!(collection, "loom_doc_index_create_json");
    // SAFETY: caller guarantees `(declaration_json, declaration_json_len)` is readable/null.
    let declaration_json = unsafe { byte_slice(declaration_json, declaration_json_len) };
    match doc_index_create_json_ns(h, workspace, collection, declaration_json) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Drop a native document index and write whether it was present.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings; `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_index_drop(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    name: *const c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_index_drop");
    let workspace = arg_str!(workspace, "loom_doc_index_drop");
    let collection = arg_str!(collection, "loom_doc_index_drop");
    let name = arg_str!(name, "loom_doc_index_drop");
    match doc_index_drop_ns(h, workspace, collection, name) {
        Ok(found) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` writable per docs.
                unsafe { *out_found = i32::from(found) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Rebuild a native document index.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments must be valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_index_rebuild(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    name: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_index_rebuild");
    let workspace = arg_str!(workspace, "loom_doc_index_rebuild");
    let collection = arg_str!(collection, "loom_doc_index_rebuild");
    let name = arg_str!(name, "loom_doc_index_rebuild");
    match doc_index_rebuild_ns(h, workspace, collection, name) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// List native document indexes as UTF-8 JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_index_list_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_index_list_json");
    let workspace = arg_str!(workspace, "loom_doc_index_list_json");
    let collection = arg_str!(collection, "loom_doc_index_list_json");
    match doc_index_list_json_ns(h, workspace, collection) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Return native document index readiness as UTF-8 JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_index_status_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_index_status_json");
    let workspace = arg_str!(workspace, "loom_doc_index_status_json");
    let collection = arg_str!(collection, "loom_doc_index_status_json");
    match doc_index_status_json_ns(h, workspace, collection) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Find document ids by exact index value. `value_json` is a JSON scalar and output is UTF-8 JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments valid C strings; byte and out pointers valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_find_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    index: *const c_char,
    value_json: *const c_uchar,
    value_json_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_find_json");
    let workspace = arg_str!(workspace, "loom_doc_find_json");
    let collection = arg_str!(collection, "loom_doc_find_json");
    let index = arg_str!(index, "loom_doc_find_json");
    // SAFETY: caller guarantees `(value_json, value_json_len)` is readable/null (see docs).
    let value_json = unsafe { byte_slice(value_json, value_json_len) };
    match doc_find_json_ns(h, workspace, collection, index, value_json) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Execute a native document query. `query_json` and output are UTF-8 JSON.
///
/// # Safety
/// `handle` must be from [`loom_open`]; string arguments valid C strings; byte and out pointers valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_doc_query_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    collection: *const c_char,
    query_json: *const c_uchar,
    query_json_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_doc_query_json");
    let workspace = arg_str!(workspace, "loom_doc_query_json");
    let collection = arg_str!(collection, "loom_doc_query_json");
    // SAFETY: caller guarantees `(query_json, query_json_len)` is readable/null (see docs).
    let query_json = unsafe { byte_slice(query_json, query_json_len) };
    match doc_query_json_ns(h, workspace, collection, query_json) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}
