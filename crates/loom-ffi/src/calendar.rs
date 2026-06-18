//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use loom_core::calendar;

// ---------------------------------------------------------------------------------------------------
// Calendar (Calendar facet, 0037) - collections under a principal, typed iCalendar records keyed by UID.
//
// ABI shape. Every entry/record crosses as its own canonical CBOR (`CalendarEntry::encode`/`decode`); a
// binding decodes each record with its own decoder. List/search/range return a single canonical-CBOR
// buffer: `loom_cal_list_entries`/`loom_cal_search` return `Array(Bytes(record))`; `loom_cal_range`
// returns `Array(Array([Text(uid), Text("YYYYMMDDTHHMMSS")]))`; `loom_cal_list_collections` returns
// `Array(Text)`. Collection metadata crosses as the simplest pair the binding already has: a
// `display_name` C string plus a `components` C string like "event,todo" ("" -> empty set).
// ---------------------------------------------------------------------------------------------------

/// Resolve a workspace for a calendar write by UUID or name, ensuring the `calendar` facet exists. A name
/// not yet present is created carrying the `calendar` facet; an unknown UUID is `NOT_FOUND`.
fn ensure_cal_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Calendar,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Calendar)?;
    Ok(ns)
}

/// Parse a comma-separated component list ("event,todo"; an empty string is the empty set) into the
/// `component_set` of a [`CollectionMeta`]. An unknown token is `INVALID_ARGUMENT`.
fn parse_component_set(components: &str) -> LoomResult<Vec<Component>> {
    let mut out = Vec::new();
    for tok in components.split(',') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        match tok {
            "event" => out.push(Component::Event),
            "todo" => out.push(Component::Todo),
            other => {
                return Err(LoomError::invalid(format!(
                    "loom_cal: unknown component {other:?}"
                )));
            }
        }
    }
    Ok(out)
}

fn cal_create_collection_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    collection: &str,
    display_name: &str,
    components: &str,
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = ensure_cal_ns(&mut loom, workspace)?;
    let meta = CollectionMeta {
        display_name: display_name.to_string(),
        component_set: parse_component_set(components)?,
    };
    calendar::create_collection(&mut loom, ns, principal, collection, &meta)?;
    save_loom(&mut loom)?;
    Ok(())
}

fn cal_delete_collection_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    collection: &str,
) -> LoomResult<bool> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let existed = calendar::delete_collection(&mut loom, ns, principal, collection)?;
    if existed {
        save_loom(&mut loom)?;
    }
    Ok(existed)
}

fn cal_list_collections_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_wire::string_list_to_cbor(calendar::list_collections(&loom, ns, principal)?)
}

fn cal_put_entry_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    collection: &str,
    entry: &[u8],
) -> LoomResult<()> {
    let entry = CalendarEntry::decode(entry)?;
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    calendar::put_entry(&mut loom, ns, principal, collection, &entry)?;
    save_loom(&mut loom)?;
    Ok(())
}

fn cal_get_entry_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    collection: &str,
    uid: &str,
) -> LoomResult<Option<Vec<u8>>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(calendar::get_entry(&loom, ns, principal, collection, uid)?.map(|e| e.encode()))
}

fn cal_delete_entry_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    collection: &str,
    uid: &str,
) -> LoomResult<bool> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let present = calendar::delete_entry(&mut loom, ns, principal, collection, uid)?;
    if present {
        save_loom(&mut loom)?;
    }
    Ok(present)
}

fn cal_list_entries_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    collection: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let records = calendar::list_entries(&loom, ns, principal, collection)?
        .iter()
        .map(CalendarEntry::encode)
        .collect();
    records_cbor(records)
}

fn cal_range_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    collection: &str,
    from: &str,
    to: &str,
) -> LoomResult<Vec<u8>> {
    let from = loom_wire::calendar::parse_window_bound(from, "from")?;
    let to = loom_wire::calendar::parse_window_bound(to, "to")?;
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_wire::calendar::occurrences_to_cbor(calendar::range(
        &loom, ns, principal, collection, from, to,
    )?)
}

fn cal_search_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    collection: &str,
    component: &str,
    text: &str,
) -> LoomResult<Vec<u8>> {
    let component = loom_wire::calendar::parse_component_filter(component)?;
    let text = if text.is_empty() { None } else { Some(text) };
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let records = calendar::search(&loom, ns, principal, collection, component, text)?
        .iter()
        .map(CalendarEntry::encode)
        .collect();
    records_cbor(records)
}

fn cal_entry_ics_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    collection: &str,
    uid: &str,
) -> LoomResult<Option<String>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    calendar::entry_ics(&loom, ns, principal, collection, uid)
}

fn cal_put_ics_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    collection: &str,
    ics: &str,
) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let etag = calendar::put_ics(&mut loom, ns, principal, collection, ics)?;
    save_loom(&mut loom)?;
    Ok(etag.to_string())
}

/// Create (or replace the metadata of) calendar collection `collection` under `principal` in workspace
/// `workspace` (UUID or name, created with the `calendar` facet if absent). `display_name` is the
/// collection's display name; `components` is a comma-separated component set ("event,todo"; "" is the
/// empty set). Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`collection`/`display_name`/`components`
/// valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cal_create_collection(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    collection: *const c_char,
    display_name: *const c_char,
    components: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cal_create_collection");
    let workspace = arg_str!(workspace, "loom_cal_create_collection");
    let principal = arg_str!(principal, "loom_cal_create_collection");
    let collection = arg_str!(collection, "loom_cal_create_collection");
    let display_name = arg_str!(display_name, "loom_cal_create_collection");
    let components = arg_str!(components, "loom_cal_create_collection");
    match cal_create_collection_ns(
        h,
        workspace,
        principal,
        collection,
        display_name,
        components,
    ) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Delete calendar collection `collection` under `principal` and every entry in it; writes whether it
/// existed (`1`/`0`) to `*out_found` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`collection` valid C strings;
/// `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cal_delete_collection(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    collection: *const c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cal_delete_collection");
    let workspace = arg_str!(workspace, "loom_cal_delete_collection");
    let principal = arg_str!(principal, "loom_cal_delete_collection");
    let collection = arg_str!(collection, "loom_cal_delete_collection");
    match cal_delete_collection_ns(h, workspace, principal, collection) {
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

/// List the calendar collection ids under `principal` as the Loom Canonical CBOR array of text strings
/// (sorted; an absent principal is the empty array). Writes owned bytes to `(*out_ptr, *out_len)` (free
/// with [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal` valid C strings; `out_ptr`/`out_len`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cal_list_collections(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cal_list_collections");
    let workspace = arg_str!(workspace, "loom_cal_list_collections");
    let principal = arg_str!(principal, "loom_cal_list_collections");
    match cal_list_collections_ns(h, workspace, principal) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Put the calendar `entry` (its `CalendarEntry` canonical CBOR) into the existing collection
/// `collection` under `principal`, keyed by its UID. Returns `0`. A later put at the same UID replaces it.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`collection` valid C strings; `entry`
/// null or `entry_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cal_put_entry(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    collection: *const c_char,
    entry: *const c_uchar,
    entry_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cal_put_entry");
    let workspace = arg_str!(workspace, "loom_cal_put_entry");
    let principal = arg_str!(principal, "loom_cal_put_entry");
    let collection = arg_str!(collection, "loom_cal_put_entry");
    // SAFETY: caller guarantees `(entry, entry_len)` is readable/null (see docs).
    let entry = unsafe { byte_slice(entry, entry_len) };
    match cal_put_entry_ns(h, workspace, principal, collection, entry) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Fetch the calendar entry at `uid` in collection `collection` as its `CalendarEntry` canonical CBOR. On
/// success returns `0` and sets `*out_found`: present -> `1` and bytes at `(*out_ptr, *out_len)` (free
/// with [`loom_bytes_free`]); absent -> `0` and `(null, 0)`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`collection`/`uid` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cal_get_entry(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    collection: *const c_char,
    uid: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cal_get_entry");
    let workspace = arg_str!(workspace, "loom_cal_get_entry");
    let principal = arg_str!(principal, "loom_cal_get_entry");
    let collection = arg_str!(collection, "loom_cal_get_entry");
    let uid = arg_str!(uid, "loom_cal_get_entry");
    match cal_get_entry_ns(h, workspace, principal, collection, uid) {
        Ok(Some(bytes)) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` writable per docs.
                unsafe { *out_found = 1 };
            }
            // SAFETY: `out_ptr`/`out_len` writable per docs.
            unsafe { ok_bytes(out_ptr, out_len, bytes) }
        }
        Ok(None) => {
            // SAFETY: each non-null out-pointer is writable per docs.
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

/// Remove the calendar entry at `uid` in collection `collection`; writes presence (`1`/`0`) to
/// `*out_found` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`collection`/`uid` valid C strings;
/// `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cal_delete_entry(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    collection: *const c_char,
    uid: *const c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cal_delete_entry");
    let workspace = arg_str!(workspace, "loom_cal_delete_entry");
    let principal = arg_str!(principal, "loom_cal_delete_entry");
    let collection = arg_str!(collection, "loom_cal_delete_entry");
    let uid = arg_str!(uid, "loom_cal_delete_entry");
    match cal_delete_entry_ns(h, workspace, principal, collection, uid) {
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

/// List collection `collection` as the Loom Canonical CBOR array of per-entry `CalendarEntry` canonical
/// CBOR byte strings (UID order; an absent collection is the empty array). Writes owned bytes to
/// `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`collection` valid C strings;
/// `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cal_list_entries(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    collection: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cal_list_entries");
    let workspace = arg_str!(workspace, "loom_cal_list_entries");
    let principal = arg_str!(principal, "loom_cal_list_entries");
    let collection = arg_str!(collection, "loom_cal_list_entries");
    match cal_list_entries_ns(h, workspace, principal, collection) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Expand collection `collection` into occurrences within the half-open wall-clock window `[from, to)`
/// (both `YYYYMMDDTHHMMSS`). Returns the Loom Canonical CBOR array of `[uid, "YYYYMMDDTHHMMSS"]` pairs
/// (start order, then UID). Writes owned bytes to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`])
/// and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`collection`/`from`/`to` valid C strings;
/// `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cal_range(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    collection: *const c_char,
    from: *const c_char,
    to: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cal_range");
    let workspace = arg_str!(workspace, "loom_cal_range");
    let principal = arg_str!(principal, "loom_cal_range");
    let collection = arg_str!(collection, "loom_cal_range");
    let from = arg_str!(from, "loom_cal_range");
    let to = arg_str!(to, "loom_cal_range");
    match cal_range_ns(h, workspace, principal, collection, from, to) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Search collection `collection` by component filter and substring. `component` is "" (any),
/// "event", or "todo"; `text` is a case-insensitive substring over the summary ("" matches any).
/// Returns the Loom Canonical CBOR array of per-entry `CalendarEntry` canonical CBOR byte strings (UID
/// order). Writes owned bytes to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`collection`/`component`/`text` valid C
/// strings; `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cal_search(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    collection: *const c_char,
    component: *const c_char,
    text: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cal_search");
    let workspace = arg_str!(workspace, "loom_cal_search");
    let principal = arg_str!(principal, "loom_cal_search");
    let collection = arg_str!(collection, "loom_cal_search");
    let component = arg_str!(component, "loom_cal_search");
    let text = arg_str!(text, "loom_cal_search");
    match cal_search_ns(h, workspace, principal, collection, component, text) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// The on-demand iCalendar (`.ics`) projection of the entry at `uid`. On success returns `0` and sets
/// `*out_found`: present -> `1` and an owned C string at `*out` (free with [`loom_string_free`]); absent
/// -> `0` and `*out = null`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`collection`/`uid` valid C strings;
/// `out`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cal_entry_ics(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    collection: *const c_char,
    uid: *const c_char,
    out: *mut *mut c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cal_entry_ics");
    let workspace = arg_str!(workspace, "loom_cal_entry_ics");
    let principal = arg_str!(principal, "loom_cal_entry_ics");
    let collection = arg_str!(collection, "loom_cal_entry_ics");
    let uid = arg_str!(uid, "loom_cal_entry_ics");
    match cal_entry_ics_ns(h, workspace, principal, collection, uid) {
        Ok(Some(s)) => {
            if !out_found.is_null() {
                // SAFETY: `out_found` writable per docs.
                unsafe { *out_found = 1 };
            }
            // SAFETY: `out` writable per docs.
            unsafe { ok_str(out, &s) }
        }
        Ok(None) => {
            // SAFETY: each non-null out-pointer is writable per docs.
            unsafe {
                if !out_found.is_null() {
                    *out_found = 0;
                }
                if !out.is_null() {
                    *out = core::ptr::null_mut();
                }
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Parse iCalendar document `ics` and store it as a record in collection `collection` (the validated
/// write-in path); writes the new ETag as a `"algo:hex"` owned C string to `*out` (free with
/// [`loom_string_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`collection`/`ics` valid C strings;
/// `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_cal_put_ics(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    collection: *const c_char,
    ics: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_cal_put_ics");
    let workspace = arg_str!(workspace, "loom_cal_put_ics");
    let principal = arg_str!(principal, "loom_cal_put_ics");
    let collection = arg_str!(collection, "loom_cal_put_ics");
    let ics = arg_str!(ics, "loom_cal_put_ics");
    match cal_put_ics_ns(h, workspace, principal, collection, ics) {
        // SAFETY: `out` writable per docs.
        Ok(etag) => unsafe { ok_str(out, &etag) },
        Err(e) => fail(e),
    }
}
