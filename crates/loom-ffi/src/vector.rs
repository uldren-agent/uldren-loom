//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::EmbeddingModel;
use loom_wire::vector::{
    accelerator_policy_from_int, embedding_model_cbor, floats_from_bytes, hits_cbor,
    meta_filter_from_cbor, metadata_from_cbor, metric_from_int, vector_entry_to_cbor,
};

// ---------------------------------------------------------------------------------------------------
// Vector set (Vector facet) - dense embeddings + metadata in a named set within a workspace, with exact
// top-k search. An embedding crosses as raw little-endian `f32` bytes (4 bytes per component); metadata
// crosses as a Loom Canonical CBOR map of `text -> cell`; a get crosses as `[vector_bytes, metadata]`;
// a search result crosses as a CBOR array of `[id, score_cell]`, highest score first. The metric tags
// match the engine: 1 cosine, 2 negative-squared-L2, 3 dot.
// ---------------------------------------------------------------------------------------------------

fn ensure_vector_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Vector,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Vector)?;
    Ok(ns)
}

fn vector_ids_cbor(ids: Vec<String>) -> LoomResult<Vec<u8>> {
    loom_wire::string_list_to_cbor(ids)
}

unsafe fn ok_optional_bytes(
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
    bytes: Option<Vec<u8>>,
) -> i32 {
    match bytes {
        Some(bytes) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` is writable per caller contract.
                unsafe { *out_found = 1 };
            }
            // SAFETY: `out_ptr`/`out_len` are writable per caller contract.
            unsafe { ok_bytes(out_ptr, out_len, bytes) }
        }
        None => {
            // SAFETY: each non-null out-pointer is writable per caller contract.
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
    }
}

/// Create vector set `name` of width `dim` and `metric` (1 cosine, 2 L2, 3 dot) in workspace `workspace`
/// (UUID or name, created with the `vector` facet if absent). `CONFLICT` if the set already exists.
/// Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_create(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    dim: usize,
    metric: i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_create");
    let workspace = arg_str!(workspace, "loom_vector_create");
    let name = arg_str!(name, "loom_vector_create");
    let result = (|| -> LoomResult<()> {
        let metric = metric_from_int(metric)?;
        let mut loom = open_h_write(h)?;
        let ns = ensure_vector_ns(&mut loom, workspace)?;
        loom_core::vector_create(&mut loom, ns, name, dim, metric)?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Insert or replace the vector at `id` in set `name`: `vector` is `vector_len` bytes of little-endian
/// `f32` (4 per component); `metadata` is a CBOR `text -> cell` map (or empty). `NOT_FOUND` if the set
/// was never created; `DIMENSION_MISMATCH` on a wrong width. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings; `vector`/`metadata` null
/// or their `_len` bytes readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_upsert(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    vector: *const c_uchar,
    vector_len: usize,
    metadata: *const c_uchar,
    metadata_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_upsert");
    let workspace = arg_str!(workspace, "loom_vector_upsert");
    let name = arg_str!(name, "loom_vector_upsert");
    let id = arg_str!(id, "loom_vector_upsert");
    // SAFETY: caller guarantees `(vector, vector_len)` and `(metadata, metadata_len)` are readable/null.
    let vec_bytes = unsafe { byte_slice(vector, vector_len) };
    let meta_bytes = unsafe { byte_slice(metadata, metadata_len) };
    let result = (|| -> LoomResult<()> {
        let vector = floats_from_bytes(vec_bytes)?;
        let metadata = metadata_from_cbor(meta_bytes)?;
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom_core::vector_upsert(&mut loom, ns, name, id, vector, metadata)?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Insert or replace a vector with source text and optional embedding model profile. `source_text` must
/// be UTF-8. When `has_model_id != 0`, the model profile dimension is inferred from `vector`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings; `vector`/`metadata`/
/// `source_text` null or their `_len` bytes readable; `model_id`/`weights_digest` valid when their
/// corresponding `has_*` flag is non-zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_upsert_source(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    vector: *const c_uchar,
    vector_len: usize,
    metadata: *const c_uchar,
    metadata_len: usize,
    source_text: *const c_uchar,
    source_text_len: usize,
    model_id: *const c_char,
    has_model_id: i32,
    weights_digest: *const c_char,
    has_weights_digest: i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_upsert_source");
    let workspace = arg_str!(workspace, "loom_vector_upsert_source");
    let name = arg_str!(name, "loom_vector_upsert_source");
    let id = arg_str!(id, "loom_vector_upsert_source");
    // SAFETY: caller guarantees these input buffers are readable/null.
    let vec_bytes = unsafe { byte_slice(vector, vector_len) };
    let meta_bytes = unsafe { byte_slice(metadata, metadata_len) };
    // SAFETY: caller guarantees the source buffer is readable/null.
    let source_bytes = unsafe { byte_slice(source_text, source_text_len) };
    let model_id = if has_model_id != 0 {
        Some(arg_str!(model_id, "loom_vector_upsert_source"))
    } else {
        None
    };
    let weights_digest = if has_weights_digest != 0 {
        Some(arg_str!(weights_digest, "loom_vector_upsert_source").to_string())
    } else {
        None
    };
    let result = (|| -> LoomResult<()> {
        let vector = floats_from_bytes(vec_bytes)?;
        let metadata = metadata_from_cbor(meta_bytes)?;
        let source_text = std::str::from_utf8(source_bytes)
            .map_err(|e| LoomError::invalid(format!("source_text must be UTF-8: {e}")))?;
        let embedding_model =
            model_id.map(|model_id| EmbeddingModel::new(model_id, vector.len(), weights_digest));
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom_core::vector_upsert_with_source(
            &mut loom,
            ns,
            name,
            id,
            vector,
            metadata,
            source_text,
            embedding_model,
        )?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Fetch the vector + metadata at `id` in set `name` as the CBOR array `[vector_bytes, metadata]`
/// (`vector_bytes` little-endian `f32`; `metadata` a `text -> cell` map). On success returns `0` and
/// sets `*out_found`: present -> `1` and bytes at `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]);
/// absent -> `0` and `(null, 0)`. `NOT_FOUND` if the set does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_get(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_get");
    let workspace = arg_str!(workspace, "loom_vector_get");
    let name = arg_str!(name, "loom_vector_get");
    let id = arg_str!(id, "loom_vector_get");
    let result = (|| -> LoomResult<Option<Vec<u8>>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(loom_core::vector_get(&loom, ns, name, id)?
            .map(|(vector, metadata)| vector_entry_to_cbor(&vector, &metadata)))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len`/`out_found` are writable per fn docs.
        Ok(bytes) => unsafe { ok_optional_bytes(out_ptr, out_len, out_found, bytes) },
        Err(e) => fail(e),
    }
}

/// Fetch source text for vector `id` as UTF-8 bytes. Present source returns `*out_found = 1`; absent
/// source returns `*out_found = 0`. `NOT_FOUND` if the set does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_source_text(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_source_text");
    let workspace = arg_str!(workspace, "loom_vector_source_text");
    let name = arg_str!(name, "loom_vector_source_text");
    let id = arg_str!(id, "loom_vector_source_text");
    let result = (|| -> LoomResult<Option<Vec<u8>>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(loom_core::vector_source_text(&loom, ns, name, id)?.map(String::into_bytes))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len`/`out_found` are writable per fn docs.
        Ok(bytes) => unsafe { ok_optional_bytes(out_ptr, out_len, out_found, bytes) },
        Err(e) => fail(e),
    }
}

/// Fetch the set embedding model profile as CBOR `[1, model_id, dimension, weights_digest]`. Absent
/// profile returns `*out_found = 0`. `NOT_FOUND` if the set does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_embedding_model_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_embedding_model_cbor");
    let workspace = arg_str!(workspace, "loom_vector_embedding_model_cbor");
    let name = arg_str!(name, "loom_vector_embedding_model_cbor");
    let result = (|| -> LoomResult<Option<Vec<u8>>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(loom_core::vector_embedding_model(&loom, ns, name)?.map(|m| embedding_model_cbor(&m)))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len`/`out_found` are writable per fn docs.
        Ok(bytes) => unsafe { ok_optional_bytes(out_ptr, out_len, out_found, bytes) },
        Err(e) => fail(e),
    }
}

/// Vector ids in set `name`, sorted ascending, as a CBOR array of text. When `has_prefix != 0`, only ids
/// starting with `prefix` are returned. `NOT_FOUND` if the set does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `prefix` valid when
/// `has_prefix != 0`; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_ids_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    prefix: *const c_char,
    has_prefix: i32,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_ids_cbor");
    let workspace = arg_str!(workspace, "loom_vector_ids_cbor");
    let name = arg_str!(name, "loom_vector_ids_cbor");
    let prefix = if has_prefix != 0 {
        Some(arg_str!(prefix, "loom_vector_ids_cbor"))
    } else {
        None
    };
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        vector_ids_cbor(loom_core::vector_ids(&loom, ns, name, prefix)?)
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Metadata equality index keys declared for set `name`, sorted ascending, as a CBOR array of text.
/// `NOT_FOUND` if the set does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `out_ptr`/`out_len`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_metadata_index_keys_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_metadata_index_keys_cbor");
    let workspace = arg_str!(workspace, "loom_vector_metadata_index_keys_cbor");
    let name = arg_str!(name, "loom_vector_metadata_index_keys_cbor");
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        vector_ids_cbor(loom_core::vector_metadata_index_keys(&loom, ns, name)?)
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Declare and build a metadata equality index for `key` in set `name`; writes whether a new index was
/// declared (`1`/`0`) to `*out_changed` and returns `0`. `NOT_FOUND` if the set does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`key` valid C strings; `out_changed`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_create_metadata_index(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    key: *const c_char,
    out_changed: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_create_metadata_index");
    let workspace = arg_str!(workspace, "loom_vector_create_metadata_index");
    let name = arg_str!(name, "loom_vector_create_metadata_index");
    let key = arg_str!(key, "loom_vector_create_metadata_index");
    let result = (|| -> LoomResult<bool> {
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        let changed = loom_core::vector_create_metadata_index(&mut loom, ns, name, key)?;
        if changed {
            save_loom(&mut loom)?;
        }
        Ok(changed)
    })();
    match result {
        Ok(changed) => {
            if !out_changed.is_null() {
                // SAFETY: `out_changed` is writable per fn docs.
                unsafe { *out_changed = i32::from(changed) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Drop the metadata equality index for `key` in set `name`; writes whether an index was present
/// (`1`/`0`) to `*out_changed` and returns `0`. `NOT_FOUND` if the set does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`key` valid C strings; `out_changed`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_drop_metadata_index(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    key: *const c_char,
    out_changed: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_drop_metadata_index");
    let workspace = arg_str!(workspace, "loom_vector_drop_metadata_index");
    let name = arg_str!(name, "loom_vector_drop_metadata_index");
    let key = arg_str!(key, "loom_vector_drop_metadata_index");
    let result = (|| -> LoomResult<bool> {
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        let changed = loom_core::vector_drop_metadata_index(&mut loom, ns, name, key)?;
        if changed {
            save_loom(&mut loom)?;
        }
        Ok(changed)
    })();
    match result {
        Ok(changed) => {
            if !out_changed.is_null() {
                // SAFETY: `out_changed` is writable per fn docs.
                unsafe { *out_changed = i32::from(changed) };
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Remove `id` from set `name`; writes whether it was present (`1`/`0`) to `*out_found` and returns `0`.
/// `NOT_FOUND` if the set does not exist.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings; `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_delete(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_delete");
    let workspace = arg_str!(workspace, "loom_vector_delete");
    let name = arg_str!(name, "loom_vector_delete");
    let id = arg_str!(id, "loom_vector_delete");
    let result = (|| -> LoomResult<bool> {
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        let present = loom_core::vector_delete(&mut loom, ns, name, id)?;
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

/// Exact top-`k` nearest neighbours of `query` (`query_len` bytes of little-endian `f32`) in set `name`
/// among vectors passing `filter`, as a CBOR array of `[id, score_cell]`, highest score first. The
/// filter is a recursive CBOR array: `[0]` all, `[1, key, value_cell]` equality, `[2, a, b]` AND; an
/// empty buffer is all. Writes owned bytes to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and
/// returns `0`. `NOT_FOUND` if the set does not exist; `DIMENSION_MISMATCH` on a wrong-width query.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `query`/`filter` null or
/// their `_len` bytes readable; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_search_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    query: *const c_uchar,
    query_len: usize,
    k: usize,
    filter: *const c_uchar,
    filter_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_search_cbor");
    let workspace = arg_str!(workspace, "loom_vector_search_cbor");
    let name = arg_str!(name, "loom_vector_search_cbor");
    // SAFETY: caller guarantees `(query, query_len)` and `(filter, filter_len)` are readable/null.
    let query_bytes = unsafe { byte_slice(query, query_len) };
    let filter_bytes = unsafe { byte_slice(filter, filter_len) };
    let result = (|| -> LoomResult<Vec<u8>> {
        let query = floats_from_bytes(query_bytes)?;
        let filter = meta_filter_from_cbor(filter_bytes)?;
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(hits_cbor(loom_core::vector_search(
            &loom, ns, name, &query, k, &filter,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Top-`k` nearest neighbours of `query` with an explicit accelerator policy over the built-in PQ
/// accelerator. `policy` is 0 for exact and 1 for approximate-above-threshold. `threshold` controls
/// when the PQ accelerator is used; `ef` is the PQ shortlist size, with small values widened by the
/// engine. `pq_m`, `pq_k`, and `pq_iters` control the derived PQ index built for this call. Results
/// are the same CBOR shape as [`loom_vector_search_cbor`].
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name` valid C strings; `query`/`filter` null or
/// their `_len` bytes readable; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_vector_search_policy_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    query: *const c_uchar,
    query_len: usize,
    k: usize,
    filter: *const c_uchar,
    filter_len: usize,
    policy: i32,
    threshold: usize,
    ef: usize,
    pq_m: usize,
    pq_k: usize,
    pq_iters: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_vector_search_policy_cbor");
    let workspace = arg_str!(workspace, "loom_vector_search_policy_cbor");
    let name = arg_str!(name, "loom_vector_search_policy_cbor");
    // SAFETY: caller guarantees `(query, query_len)` and `(filter, filter_len)` are readable/null.
    let query_bytes = unsafe { byte_slice(query, query_len) };
    let filter_bytes = unsafe { byte_slice(filter, filter_len) };
    let result = (|| -> LoomResult<Vec<u8>> {
        let query = floats_from_bytes(query_bytes)?;
        let filter = meta_filter_from_cbor(filter_bytes)?;
        let policy = accelerator_policy_from_int(policy, threshold)?;
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(hits_cbor(loom_core::vector_search_with_pq_policy(
            &loom, ns, name, &query, k, &filter, policy, ef, pq_m, pq_k, pq_iters,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}
