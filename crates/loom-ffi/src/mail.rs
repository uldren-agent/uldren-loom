//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use loom_core::mail;

// ---------------------------------------------------------------------------------------------------
// Mail (Mail facet, 0039) - mailboxes under a principal; immutable RFC 5322 bodies plus a structured
// index and mutable flags, keyed by UID.
//
// ABI shape. The index record crosses as `MailMessage::encode`/`decode` canonical CBOR; the raw body is
// ingested and fetched as bare bytes. `loom_mail_list_messages` returns `Array(Bytes(record))`;
// `loom_mail_list_mailboxes`/`loom_mail_get_flags` return `Array(Text)`. `loom_mail_set_flags` takes the
// flags as a canonical-CBOR `Array(Text)` byte buffer (mirroring how byte args are taken). Mailbox
// metadata is just a `display_name` C string.
// ---------------------------------------------------------------------------------------------------

/// Resolve a workspace for a mail write by UUID or name, ensuring the `mail` facet exists.
fn ensure_mail_ns(loom: &mut Loom<FileStore>, workspace: &str) -> LoomResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Mail,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)?;
    loom.registry_mut().add_facet(ns, FacetKind::Mail)?;
    Ok(ns)
}

/// Decode a canonical-CBOR `Array(Text)` flag-set buffer into the owned strings `set_flags` expects.
fn flags_from_cbor(bytes: &[u8]) -> LoomResult<Vec<String>> {
    loom_wire::string_list_from_cbor(bytes)
}

fn mail_create_mailbox_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    display_name: &str,
) -> LoomResult<()> {
    let mut loom = open_h_write(h)?;
    let ns = ensure_mail_ns(&mut loom, workspace)?;
    let meta = MailboxMeta {
        display_name: display_name.to_string(),
    };
    mail::create_mailbox(&mut loom, ns, principal, mailbox, &meta)?;
    save_loom(&mut loom)?;
    Ok(())
}

fn mail_delete_mailbox_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    mailbox: &str,
) -> LoomResult<bool> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let existed = mail::delete_mailbox(&mut loom, ns, principal, mailbox)?;
    if existed {
        save_loom(&mut loom)?;
    }
    Ok(existed)
}

fn mail_list_mailboxes_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_wire::string_list_to_cbor(mail::list_mailboxes(&loom, ns, principal)?)
}

fn mail_ingest_message_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
    raw: &[u8],
) -> LoomResult<String> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let body = mail::ingest_message(&mut loom, ns, principal, mailbox, uid, raw)?;
    save_loom(&mut loom)?;
    Ok(body.to_string())
}

fn mail_get_message_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
) -> LoomResult<Option<Vec<u8>>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(mail::get_message(&loom, ns, principal, mailbox, uid)?.map(|m| m.encode()))
}

fn mail_to_eml_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
) -> LoomResult<Option<Vec<u8>>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    mail::to_eml(&loom, ns, principal, mailbox, uid)
}

fn mail_delete_message_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
) -> LoomResult<bool> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let present = mail::delete_message(&mut loom, ns, principal, mailbox, uid)?;
    if present {
        save_loom(&mut loom)?;
    }
    Ok(present)
}

fn mail_list_messages_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    mailbox: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let records = mail::list_messages(&loom, ns, principal, mailbox)?
        .iter()
        .map(MailMessage::encode)
        .collect();
    records_cbor(records)
}

fn mail_get_flags_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_wire::string_list_to_cbor(mail::get_flags(&loom, ns, principal, mailbox, uid)?)
}

fn mail_set_flags_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
    flags: &[u8],
) -> LoomResult<()> {
    let flags = flags_from_cbor(flags)?;
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    mail::set_flags(&mut loom, ns, principal, mailbox, uid, &flags)?;
    save_loom(&mut loom)?;
    Ok(())
}

fn mail_search_ns(
    h: &LoomSession,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    text: &str,
) -> LoomResult<Vec<u8>> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let records = mail::search(&loom, ns, principal, mailbox, text)?
        .iter()
        .map(MailMessage::encode)
        .collect();
    records_cbor(records)
}

/// Create (or replace the metadata of) mailbox `mailbox` under `principal` in workspace `workspace`
/// (UUID or name, created with the `mail` facet if absent). `display_name` is the mailbox's display name.
/// Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`mailbox`/`display_name` valid C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mail_create_mailbox(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    mailbox: *const c_char,
    display_name: *const c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_mail_create_mailbox");
    let workspace = arg_str!(workspace, "loom_mail_create_mailbox");
    let principal = arg_str!(principal, "loom_mail_create_mailbox");
    let mailbox = arg_str!(mailbox, "loom_mail_create_mailbox");
    let display_name = arg_str!(display_name, "loom_mail_create_mailbox");
    match mail_create_mailbox_ns(h, workspace, principal, mailbox, display_name) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Delete mailbox `mailbox` under `principal` and every message index and flag set in it (immutable
/// bodies stay in the CAS until GC); writes whether it existed (`1`/`0`) to `*out_found` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`mailbox` valid C strings; `out_found`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mail_delete_mailbox(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    mailbox: *const c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_mail_delete_mailbox");
    let workspace = arg_str!(workspace, "loom_mail_delete_mailbox");
    let principal = arg_str!(principal, "loom_mail_delete_mailbox");
    let mailbox = arg_str!(mailbox, "loom_mail_delete_mailbox");
    match mail_delete_mailbox_ns(h, workspace, principal, mailbox) {
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

/// List the mailbox ids under `principal` as the Loom Canonical CBOR array of text strings (sorted; an
/// absent principal is the empty array). Writes owned bytes to `(*out_ptr, *out_len)` (free with
/// [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal` valid C strings; `out_ptr`/`out_len`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mail_list_mailboxes(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_mail_list_mailboxes");
    let workspace = arg_str!(workspace, "loom_mail_list_mailboxes");
    let principal = arg_str!(principal, "loom_mail_list_mailboxes");
    match mail_list_mailboxes_ns(h, workspace, principal) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Ingest the raw RFC 5322 message `(raw, raw_len)` into mailbox `mailbox` under `uid`: store the
/// immutable body in the CAS, parse the headers into a structured index, and write it. Writes the body's
/// content address as a `"algo:hex"` owned C string to `*out` (free with [`loom_string_free`]) and
/// returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`mailbox`/`uid` valid C strings; `raw`
/// null or `raw_len` readable bytes; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mail_ingest_message(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    mailbox: *const c_char,
    uid: *const c_char,
    raw: *const c_uchar,
    raw_len: usize,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_mail_ingest_message");
    let workspace = arg_str!(workspace, "loom_mail_ingest_message");
    let principal = arg_str!(principal, "loom_mail_ingest_message");
    let mailbox = arg_str!(mailbox, "loom_mail_ingest_message");
    let uid = arg_str!(uid, "loom_mail_ingest_message");
    // SAFETY: caller guarantees `(raw, raw_len)` is readable/null (see docs).
    let raw = unsafe { byte_slice(raw, raw_len) };
    match mail_ingest_message_ns(h, workspace, principal, mailbox, uid, raw) {
        // SAFETY: `out` writable per docs.
        Ok(addr) => unsafe { ok_str(out, &addr) },
        Err(e) => fail(e),
    }
}

/// Fetch the structured index of the message at `uid` in mailbox `mailbox` as its `MailMessage` canonical
/// CBOR. On success returns `0` and sets `*out_found`: present -> `1` and bytes at `(*out_ptr, *out_len)`
/// (free with [`loom_bytes_free`]); absent -> `0` and `(null, 0)`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`mailbox`/`uid` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mail_get_message(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    mailbox: *const c_char,
    uid: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_mail_get_message");
    let workspace = arg_str!(workspace, "loom_mail_get_message");
    let principal = arg_str!(principal, "loom_mail_get_message");
    let mailbox = arg_str!(mailbox, "loom_mail_get_message");
    let uid = arg_str!(uid, "loom_mail_get_message");
    match mail_get_message_ns(h, workspace, principal, mailbox, uid) {
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

/// Fetch the raw RFC 5322 message (`.eml` bytes) at `uid`, from the CAS and digest-verified. This is the
/// format-specific serialization; the parsed structured record is `loom_mail_get_message`.
/// On success returns `0` and sets `*out_found`: present -> `1` and bytes at `(*out_ptr, *out_len)` (free
/// with [`loom_bytes_free`]); absent -> `0` and `(null, 0)`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`mailbox`/`uid` valid C strings;
/// `out_ptr`/`out_len`/`out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mail_to_eml(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    mailbox: *const c_char,
    uid: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_mail_to_eml");
    let workspace = arg_str!(workspace, "loom_mail_to_eml");
    let principal = arg_str!(principal, "loom_mail_to_eml");
    let mailbox = arg_str!(mailbox, "loom_mail_to_eml");
    let uid = arg_str!(uid, "loom_mail_to_eml");
    match mail_to_eml_ns(h, workspace, principal, mailbox, uid) {
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

/// Remove the message index and its flags at `uid` (the immutable body stays in the CAS until GC); writes
/// presence (`1`/`0`) to `*out_found` and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`mailbox`/`uid` valid C strings;
/// `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mail_delete_message(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    mailbox: *const c_char,
    uid: *const c_char,
    out_found: *mut i32,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_mail_delete_message");
    let workspace = arg_str!(workspace, "loom_mail_delete_message");
    let principal = arg_str!(principal, "loom_mail_delete_message");
    let mailbox = arg_str!(mailbox, "loom_mail_delete_message");
    let uid = arg_str!(uid, "loom_mail_delete_message");
    match mail_delete_message_ns(h, workspace, principal, mailbox, uid) {
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

/// List mailbox `mailbox` as the Loom Canonical CBOR array of per-message `MailMessage` canonical CBOR
/// byte strings (UID order; an absent mailbox is the empty array). Writes owned bytes to
/// `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`mailbox` valid C strings;
/// `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mail_list_messages(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    mailbox: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_mail_list_messages");
    let workspace = arg_str!(workspace, "loom_mail_list_messages");
    let principal = arg_str!(principal, "loom_mail_list_messages");
    let mailbox = arg_str!(mailbox, "loom_mail_list_messages");
    match mail_list_messages_ns(h, workspace, principal, mailbox) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// The flags/labels on the message at `uid` as the Loom Canonical CBOR array of text strings (sorted,
/// deduplicated; an absent flag set is the empty array). Writes owned bytes to `(*out_ptr, *out_len)`
/// (free with [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`mailbox`/`uid` valid C strings;
/// `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mail_get_flags(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    mailbox: *const c_char,
    uid: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_mail_get_flags");
    let workspace = arg_str!(workspace, "loom_mail_get_flags");
    let principal = arg_str!(principal, "loom_mail_get_flags");
    let mailbox = arg_str!(mailbox, "loom_mail_get_flags");
    let uid = arg_str!(uid, "loom_mail_get_flags");
    match mail_get_flags_ns(h, workspace, principal, mailbox, uid) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}

/// Replace the flags/labels on the message at `uid` with `(flags, flags_len)`, a Loom Canonical CBOR
/// `Array(Text)` buffer. The message must exist. Returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`mailbox`/`uid` valid C strings; `flags`
/// null or `flags_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mail_set_flags(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    mailbox: *const c_char,
    uid: *const c_char,
    flags: *const c_uchar,
    flags_len: usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_mail_set_flags");
    let workspace = arg_str!(workspace, "loom_mail_set_flags");
    let principal = arg_str!(principal, "loom_mail_set_flags");
    let mailbox = arg_str!(mailbox, "loom_mail_set_flags");
    let uid = arg_str!(uid, "loom_mail_set_flags");
    // SAFETY: caller guarantees `(flags, flags_len)` is readable/null (see docs).
    let flags = unsafe { byte_slice(flags, flags_len) };
    match mail_set_flags_ns(h, workspace, principal, mailbox, uid, flags) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

/// Search mailbox `mailbox` by a case-insensitive substring `text` over the subject and from values.
/// Returns the Loom Canonical CBOR array of per-message `MailMessage` canonical CBOR byte strings (UID
/// order). Writes owned bytes to `(*out_ptr, *out_len)` (free with [`loom_bytes_free`]) and returns `0`.
///
/// # Safety
/// `handle` must be from [`loom_open`]; `workspace`/`principal`/`mailbox`/`text` valid C strings;
/// `out_ptr`/`out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mail_search(
    handle: *mut LoomSession,
    workspace: *const c_char,
    principal: *const c_char,
    mailbox: *const c_char,
    text: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_mail_search");
    let workspace = arg_str!(workspace, "loom_mail_search");
    let principal = arg_str!(principal, "loom_mail_search");
    let mailbox = arg_str!(mailbox, "loom_mail_search");
    let text = arg_str!(text, "loom_mail_search");
    match mail_search_ns(h, workspace, principal, mailbox, text) {
        // SAFETY: `out_ptr`/`out_len` writable per docs.
        Ok(bytes) => unsafe { ok_bytes(out_ptr, out_len, bytes) },
        Err(e) => fail(e),
    }
}
