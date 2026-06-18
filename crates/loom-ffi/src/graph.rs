//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use loom_wire::graph::{
    edge_to_cbor, edges_array_cbor, props_from_cbor, props_to_cbor, strings_array_cbor,
};

// ---------------------------------------------------------------------------------------------------
// Property graph (Graph facet) - nodes and directed labelled edges in a named graph within a workspace.
// Node/edge properties cross as a Loom Canonical CBOR map of `text -> bytes`; an edge crosses as the
// CBOR array `[src, dst, label, props]`; neighbour/reachability/path results cross as a CBOR array of
// node-id text; out-/in-edge results cross as a CBOR array of `[edge_id, edge]` pairs in edge-id order.
// ---------------------------------------------------------------------------------------------------

fn ensure_graph_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Graph,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Graph)?;
    Ok(ns)
}

/// Insert or replace node `id` (with `props` as a CBOR `text -> bytes` map, or empty for none) in graph
/// `name` of workspace `workspace` (UUID or name, created with the `graph` facet if absent). Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings; `props` null or
/// `props_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_graph_upsert_node(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    props: *const c_uchar,
    props_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_graph_upsert_node");
    let workspace = arg_str!(workspace, "loom_graph_upsert_node");
    let name = arg_str!(name, "loom_graph_upsert_node");
    let id = arg_str!(id, "loom_graph_upsert_node");
    // SAFETY: caller guarantees `(props, props_len)` is readable/null (see docs).
    let props_bytes = unsafe { byte_slice(props, props_len) };
    let result = (|| -> LoomResult<()> {
        let props = props_from_cbor(props_bytes)?;
        let mut loom = open_h_write(h)?;
        let ns = ensure_graph_ns(&mut loom, workspace)?;
        loom_core::graph_upsert_node(&mut loom, ns, name, id, props)?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Fetch node `id`'s properties in graph `name` as a CBOR `text -> bytes` map. On success returns `0`
/// and sets `*out_found`: present -> `1` and bytes at `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]); absent -> `0` and `(null, 0)`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_graph_get_node(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_graph_get_node");
    let workspace = arg_str!(workspace, "loom_graph_get_node");
    let name = arg_str!(name, "loom_graph_get_node");
    let id = arg_str!(id, "loom_graph_get_node");
    let result = (|| -> LoomResult<Option<Vec<u8>>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(loom_core::graph_get_node(&loom, ns, name, id)?.map(|p| props_to_cbor(&p)))
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

/// Remove node `id` from graph `name`. `cascade=0` rejects with `CONFLICT` while incident edges exist;
/// `cascade!=0` removes the node and its incident edges. `NOT_FOUND` if the node is absent. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_graph_remove_node(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    cascade: i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_graph_remove_node");
    let workspace = arg_str!(workspace, "loom_graph_remove_node");
    let name = arg_str!(name, "loom_graph_remove_node");
    let id = arg_str!(id, "loom_graph_remove_node");
    let result = (|| -> LoomResult<()> {
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        loom_core::graph_remove_node(&mut loom, ns, name, id, cascade != 0)?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Insert or replace edge `id` from `src` to `dst` (both endpoints must already exist, else `NOT_FOUND`)
/// with `label` and `props` (CBOR `text -> bytes` map, or empty) in graph `name`. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id`/`src`/`dst`/`label` valid C strings;
/// `props` null or `props_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_graph_upsert_edge(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    src: *const c_char,
    dst: *const c_char,
    label: *const c_char,
    props: *const c_uchar,
    props_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_graph_upsert_edge");
    let workspace = arg_str!(workspace, "loom_graph_upsert_edge");
    let name = arg_str!(name, "loom_graph_upsert_edge");
    let id = arg_str!(id, "loom_graph_upsert_edge");
    let src = arg_str!(src, "loom_graph_upsert_edge");
    let dst = arg_str!(dst, "loom_graph_upsert_edge");
    let label = arg_str!(label, "loom_graph_upsert_edge");
    // SAFETY: caller guarantees `(props, props_len)` is readable/null (see docs).
    let props_bytes = unsafe { byte_slice(props, props_len) };
    let result = (|| -> LoomResult<()> {
        let props = props_from_cbor(props_bytes)?;
        let mut loom = open_h_write(h)?;
        let ns = ensure_graph_ns(&mut loom, workspace)?;
        loom_core::graph_upsert_edge(&mut loom, ns, name, id, src, dst, label, props)?;
        save_loom(&mut loom)?;
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Fetch edge `id` in graph `name` as the CBOR array `[src, dst, label, props]`. On success returns `0`
/// and sets `*out_found`: present -> `1` and bytes at `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]); absent -> `0` and `(null, 0)`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_graph_get_edge(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_graph_get_edge");
    let workspace = arg_str!(workspace, "loom_graph_get_edge");
    let name = arg_str!(name, "loom_graph_get_edge");
    let id = arg_str!(id, "loom_graph_get_edge");
    let result = (|| -> LoomResult<Option<Vec<u8>>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(loom_core::graph_get_edge(&loom, ns, name, id)?.map(|e| edge_to_cbor(&e)))
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

/// Remove edge `id` from graph `name`; writes whether it was present (`1`/`0`) to `*out_found` and
/// returns `0`. An absent edge or graph is a no-op.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings; `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_graph_remove_edge(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_graph_remove_edge");
    let workspace = arg_str!(workspace, "loom_graph_remove_edge");
    let name = arg_str!(name, "loom_graph_remove_edge");
    let id = arg_str!(id, "loom_graph_remove_edge");
    let result = (|| -> LoomResult<bool> {
        let mut loom = open_h_write(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        let present = loom_core::graph_remove_edge(&mut loom, ns, name, id)?;
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

/// The distinct adjacent node ids of `id` in graph `name`, sorted, as a CBOR array of text (empty when
/// the node or graph is absent). Writes owned bytes to `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_graph_neighbors_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_graph_neighbors_cbor");
    let workspace = arg_str!(workspace, "loom_graph_neighbors_cbor");
    let name = arg_str!(name, "loom_graph_neighbors_cbor");
    let id = arg_str!(id, "loom_graph_neighbors_cbor");
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(strings_array_cbor(loom_core::graph_neighbors(
            &loom, ns, name, id,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Out-edges of `id` in graph `name` as a CBOR array of `[edge_id, [src, dst, label, props]]` in
/// edge-id order. Writes owned bytes to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and
/// returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_graph_out_edges_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_graph_out_edges_cbor");
    let workspace = arg_str!(workspace, "loom_graph_out_edges_cbor");
    let name = arg_str!(name, "loom_graph_out_edges_cbor");
    let id = arg_str!(id, "loom_graph_out_edges_cbor");
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(edges_array_cbor(loom_core::graph_out_edges(
            &loom, ns, name, id,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// In-edges of `id` in graph `name` as a CBOR array of `[edge_id, [src, dst, label, props]]` in
/// edge-id order. Writes owned bytes to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and
/// returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`id` valid C strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_graph_in_edges_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    id: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_graph_in_edges_cbor");
    let workspace = arg_str!(workspace, "loom_graph_in_edges_cbor");
    let name = arg_str!(name, "loom_graph_in_edges_cbor");
    let id = arg_str!(id, "loom_graph_in_edges_cbor");
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(edges_array_cbor(loom_core::graph_in_edges(
            &loom, ns, name, id,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Node ids reachable from `start` in graph `name` as a CBOR array of text. `max_depth < 0` is no limit;
/// `via_label` null follows every edge, else only edges with that label. Writes owned bytes to
/// `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`start` valid C strings; `via_label` null or a
/// valid C string; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_graph_reachable_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    start: *const c_char,
    max_depth: i64,
    via_label: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_graph_reachable_cbor");
    let workspace = arg_str!(workspace, "loom_graph_reachable_cbor");
    let name = arg_str!(name, "loom_graph_reachable_cbor");
    let start = arg_str!(start, "loom_graph_reachable_cbor");
    // SAFETY: caller guarantees `via_label` is null or a valid C string (see docs).
    let via = unsafe { cstr(via_label) };
    let depth = (max_depth >= 0).then_some(max_depth as usize);
    let result = (|| -> LoomResult<Vec<u8>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(strings_array_cbor(loom_core::graph_reachable(
            &loom, ns, name, start, depth, via,
        )?))
    })();
    match result {
        // SAFETY: `out_ptr`/`out_len` are writable per fn docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// A shortest directed path from `from` to `to` in graph `name` as a CBOR array of node-id text. On
/// success returns `0` and sets `*out_found`: a path exists -> `1` and bytes at `(*out_ptr, *out_len)`
/// (free with [`loom_bytes_free`]); no path (or a missing endpoint or graph) -> `0` and `(null, 0)`.
/// `via_label` null follows every edge, else only edges with that label.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`name`/`from`/`to` valid C strings; `via_label` null
/// or a valid C string; `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_graph_shortest_path_cbor(
    handle: *mut LoomSession,
    workspace: *const c_char,
    name: *const c_char,
    from: *const c_char,
    to: *const c_char,
    via_label: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_graph_shortest_path_cbor");
    let workspace = arg_str!(workspace, "loom_graph_shortest_path_cbor");
    let name = arg_str!(name, "loom_graph_shortest_path_cbor");
    let from = arg_str!(from, "loom_graph_shortest_path_cbor");
    let to = arg_str!(to, "loom_graph_shortest_path_cbor");
    // SAFETY: caller guarantees `via_label` is null or a valid C string (see docs).
    let via = unsafe { cstr(via_label) };
    let result = (|| -> LoomResult<Option<Vec<u8>>> {
        let loom = open_h_read(h)?;
        let ns = resolve_workspace_arg(&loom, workspace)?;
        Ok(loom_core::graph_shortest_path(&loom, ns, name, from, to, via)?.map(strings_array_cbor))
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
