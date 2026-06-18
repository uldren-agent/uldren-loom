//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use loom_core::contacts;

// ---------------------------------------------------------------------------------------------------
// Contacts (Contacts facet, 0038) - address books under a principal, typed vCard records keyed by UID.
//
// ABI shape mirrors calendar: records cross as `ContactEntry::encode`/`decode` canonical CBOR; list and
// search return `Array(Bytes(record))`; `loom_card_list_books` returns `Array(Text)`. Book metadata is
// just a `display_name` C string.
// ---------------------------------------------------------------------------------------------------

/// Resolve a workspace for a contacts write by UUID or name, ensuring the `contacts` facet exists.
fn ensure_card_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Contacts,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Contacts)?;
    Ok(ns)
}

fn card_create_book_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    book: &str,
    display_name: &str,
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = ensure_card_ns(&mut loom, workspace)?;
    let meta = BookMeta {
        display_name: display_name.to_string(),
    };
    contacts::create_book(&mut loom, ns, principal, book, &meta)?;
    save_loom(&mut loom)?;
    Ok(())
}

fn card_delete_book_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    book: &str,
) -> LoomResult<bool> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let existed = contacts::delete_book(&mut loom, ns, principal, book)?;
    if existed {
        save_loom(&mut loom)?;
    }
    Ok(existed)
}

fn card_list_books_ns(h: &LoomSession, workspace: &str, principal: &str) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_wire::string_list_to_cbor(contacts::list_books(&loom, ns, principal)?)
}

fn card_put_entry_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    book: &str,
    entry: &[u8],
) -> LoomResult<()> {
    let entry = ContactEntry::decode(entry)?;
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    contacts::put_entry(&mut loom, ns, principal, book, &entry)?;
    save_loom(&mut loom)?;
    Ok(())
}

fn card_get_entry_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    book: &str,
    uid: &str,
) -> LoomResult<Option<Vec<u8>>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(contacts::get_entry(&loom, ns, principal, book, uid)?.map(|e| e.encode()))
}

fn card_delete_entry_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    book: &str,
    uid: &str,
) -> LoomResult<bool> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let present = contacts::delete_entry(&mut loom, ns, principal, book, uid)?;
    if present {
        save_loom(&mut loom)?;
    }
    Ok(present)
}

fn card_list_entries_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    book: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let records = contacts::list_entries(&loom, ns, principal, book)?
        .iter()
        .map(ContactEntry::encode)
        .collect();
    records_cbor(records)
}

fn card_search_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    book: &str,
    text: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let records = contacts::search(&loom, ns, principal, book, text)?
        .iter()
        .map(ContactEntry::encode)
        .collect();
    records_cbor(records)
}

fn card_entry_vcard_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    book: &str,
    uid: &str,
) -> LoomResult<Option<String>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    contacts::entry_vcard(&loom, ns, principal, book, uid)
}

fn card_put_vcard_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    book: &str,
    vcf: &str,
) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let etag = contacts::put_vcard(&mut loom, ns, principal, book, vcf)?;
    save_loom(&mut loom)?;
    Ok(etag.to_string())
}

/// Create (or replace the metadata of) address book `book` under `principal` in workspace `workspace`
/// (UUID or name, created with the `contacts` facet if absent). `display_name` is the book's display
/// name. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`book`/`display_name` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_card_create_book(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    book: *const c_char,
    display_name: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_card_create_book");
    let workspace = arg_str!(workspace, "loom_card_create_book");
    let principal = arg_str!(principal, "loom_card_create_book");
    let book = arg_str!(book, "loom_card_create_book");
    let display_name = arg_str!(display_name, "loom_card_create_book");
    match card_create_book_ns(h, workspace, principal, book, display_name) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Delete address book `book` under `principal` and every contact in it; writes whether it existed
/// (`1`/`0`) to `*out_found` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`book` valid C strings; `out_found`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_card_delete_book(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    book: *const c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_card_delete_book");
    let workspace = arg_str!(workspace, "loom_card_delete_book");
    let principal = arg_str!(principal, "loom_card_delete_book");
    let book = arg_str!(book, "loom_card_delete_book");
    match card_delete_book_ns(h, workspace, principal, book) {
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

/// List the address-book ids under `principal` as the Loom Canonical CBOR array of text strings (sorted;
/// an absent principal is the empty array). Writes owned bytes to `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal` valid C strings; `out_ptr`/`out_len`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_card_list_books(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_card_list_books");
    let workspace = arg_str!(workspace, "loom_card_list_books");
    let principal = arg_str!(principal, "loom_card_list_books");
    match card_list_books_ns(h, workspace, principal) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Put the contact `entry` (its `ContactEntry` canonical CBOR) into the existing address book `book`
/// under `principal`, keyed by its UID. Returns `0`. A later put at the same UID replaces it.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`book` valid C strings; `entry` null or
/// `entry_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_card_put_entry(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    book: *const c_char,
    entry: *const c_uchar,
    entry_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_card_put_entry");
    let workspace = arg_str!(workspace, "loom_card_put_entry");
    let principal = arg_str!(principal, "loom_card_put_entry");
    let book = arg_str!(book, "loom_card_put_entry");
    // SAFETY: caller guarantees `(entry, entry_len)` is readable/null (see docs).
    let entry = unsafe { byte_slice(entry, entry_len) };
    match card_put_entry_ns(h, workspace, principal, book, entry) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Fetch the contact at `uid` in address book `book` as its `ContactEntry` canonical CBOR. On success
/// returns `0` and sets `*out_found`: present -> `1` and bytes at `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]); absent -> `0` and `(null, 0)`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`book`/`uid` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_card_get_entry(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    book: *const c_char,
    uid: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_card_get_entry");
    let workspace = arg_str!(workspace, "loom_card_get_entry");
    let principal = arg_str!(principal, "loom_card_get_entry");
    let book = arg_str!(book, "loom_card_get_entry");
    let uid = arg_str!(uid, "loom_card_get_entry");
    match card_get_entry_ns(h, workspace, principal, book, uid) {
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

/// Remove the contact at `uid` in address book `book`; writes presence (`1`/`0`) to `*out_found` and
/// returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`book`/`uid` valid C strings;
/// `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_card_delete_entry(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    book: *const c_char,
    uid: *const c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_card_delete_entry");
    let workspace = arg_str!(workspace, "loom_card_delete_entry");
    let principal = arg_str!(principal, "loom_card_delete_entry");
    let book = arg_str!(book, "loom_card_delete_entry");
    let uid = arg_str!(uid, "loom_card_delete_entry");
    match card_delete_entry_ns(h, workspace, principal, book, uid) {
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

/// List address book `book` as the Loom Canonical CBOR array of per-contact `ContactEntry` canonical CBOR
/// byte strings (UID order; an absent book is the empty array). Writes owned bytes to
/// `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`book` valid C strings;
/// `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_card_list_entries(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    book: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_card_list_entries");
    let workspace = arg_str!(workspace, "loom_card_list_entries");
    let principal = arg_str!(principal, "loom_card_list_entries");
    let book = arg_str!(book, "loom_card_list_entries");
    match card_list_entries_ns(h, workspace, principal, book) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Search address book `book` by a case-insensitive substring `text` over the formatted name,
/// organization, and email values. Returns the Loom Canonical CBOR array of per-contact `ContactEntry`
/// canonical CBOR byte strings (UID order). Writes owned bytes to `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`book`/`text` valid C strings;
/// `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_card_search(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    book: *const c_char,
    text: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_card_search");
    let workspace = arg_str!(workspace, "loom_card_search");
    let principal = arg_str!(principal, "loom_card_search");
    let book = arg_str!(book, "loom_card_search");
    let text = arg_str!(text, "loom_card_search");
    match card_search_ns(h, workspace, principal, book, text) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// The on-demand vCard (`.vcf`) projection of the contact at `uid`. On success returns `0` and sets
/// `*out_found`: present -> `1` and an owned C string at `*out` (free with [`loom_string_free`]); absent
/// -> `0` and `*out = null`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`book`/`uid` valid C strings;
/// `out`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_card_entry_vcard(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    book: *const c_char,
    uid: *const c_char,
    out: *mut *mut c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_card_entry_vcard");
    let workspace = arg_str!(workspace, "loom_card_entry_vcard");
    let principal = arg_str!(principal, "loom_card_entry_vcard");
    let book = arg_str!(book, "loom_card_entry_vcard");
    let uid = arg_str!(uid, "loom_card_entry_vcard");
    match card_entry_vcard_ns(h, workspace, principal, book, uid) {
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

/// Parse vCard document `vcf` and store it as a record in address book `book` (the validated write-in
/// path); writes the new ETag as a `"algo:hex"` owned C string to `*out` (free with
/// [`loom_string_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`book`/`vcf` valid C strings; `out`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_card_put_vcard(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    book: *const c_char,
    vcf: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_card_put_vcard");
    let workspace = arg_str!(workspace, "loom_card_put_vcard");
    let principal = arg_str!(principal, "loom_card_put_vcard");
    let book = arg_str!(book, "loom_card_put_vcard");
    let vcf = arg_str!(vcf, "loom_card_put_vcard");
    match card_put_vcard_ns(h, workspace, principal, book, vcf) {
        // SAFETY: `out` writable per docs.
        Ok(etag) => unsafe { ok_str(out, &etag) },
        Err(e) => fail(e),
    }
}
