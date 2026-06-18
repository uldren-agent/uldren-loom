//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Create (or replace the metadata of) mailbox `mailbox` under `principal` in workspace `workspace` (UUID
/// or name, created with the `mail` facet if absent). `displayName` is the mailbox's display name.
#[napi]
pub fn mail_create_mailbox(
    loom_path: String,
    workspace: String,
    principal: String,
    mailbox: String,
    display_name: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_mail_ns(&mut loom, &workspace)?;
    let meta = MailboxMeta { display_name };
    mail::create_mailbox(&mut loom, ns, &principal, &mailbox, &meta).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Delete mailbox `mailbox` under `principal` and every message index and flag set in it (immutable
/// bodies stay in the CAS until GC); returns whether it existed.
#[napi]
pub fn mail_delete_mailbox(
    loom_path: String,
    workspace: String,
    principal: String,
    mailbox: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let existed = mail::delete_mailbox(&mut loom, ns, &principal, &mailbox).map_err(reason)?;
    if existed {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(existed)
}
/// List the mailbox ids under `principal` as the Loom Canonical CBOR array of text strings (sorted; an
/// absent principal is the empty array).
#[napi]
pub fn mail_list_mailboxes(
    loom_path: String,
    workspace: String,
    principal: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(strings_cbor(
        mail::list_mailboxes(&loom, ns, &principal).map_err(reason)?,
    )?))
}
/// Ingest the raw RFC 5322 message `raw` into mailbox `mailbox` under `uid`: store the immutable body in
/// the CAS, parse the headers into a structured index, and write it. Returns the body's content address as
/// a `"algo:hex"` string.
#[napi]
pub fn mail_ingest_message(
    loom_path: String,
    workspace: String,
    principal: String,
    mailbox: String,
    uid: String,
    raw: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let body =
        mail::ingest_message(&mut loom, ns, &principal, &mailbox, &uid, &raw).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(body.to_string())
}
/// Fetch the structured index of the message at `uid` in mailbox `mailbox` as its `MailMessage` canonical
/// CBOR, or `null` if absent.
#[napi]
pub fn mail_get_message(
    loom_path: String,
    workspace: String,
    principal: String,
    mailbox: String,
    uid: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(mail::get_message(&loom, ns, &principal, &mailbox, &uid)
        .map_err(reason)?
        .map(|m| m.encode())
        .map(Uint8Array::from))
}
/// Fetch the raw RFC 5322 body (`.eml` bytes) of the message at `uid`, from the CAS and digest-verified,
/// or `null` if absent.
#[napi]
pub fn mail_to_eml(
    loom_path: String,
    workspace: String,
    principal: String,
    mailbox: String,
    uid: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(mail::to_eml(&loom, ns, &principal, &mailbox, &uid)
        .map_err(reason)?
        .map(Uint8Array::from))
}
/// Remove the message index and its flags at `uid` (the immutable body stays in the CAS until GC); returns
/// whether it was present.
#[napi]
pub fn mail_delete_message(
    loom_path: String,
    workspace: String,
    principal: String,
    mailbox: String,
    uid: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let present =
        mail::delete_message(&mut loom, ns, &principal, &mailbox, &uid).map_err(reason)?;
    if present {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(present)
}
/// List mailbox `mailbox` as the Loom Canonical CBOR array of per-message `MailMessage` canonical CBOR
/// byte strings (UID order; an absent mailbox is the empty array).
#[napi]
pub fn mail_list_messages(
    loom_path: String,
    workspace: String,
    principal: String,
    mailbox: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let records = mail::list_messages(&loom, ns, &principal, &mailbox)
        .map_err(reason)?
        .iter()
        .map(MailMessage::encode)
        .collect();
    Ok(Uint8Array::from(records_cbor(records)?))
}
/// The flags/labels on the message at `uid` as the Loom Canonical CBOR array of text strings (sorted,
/// deduplicated; an absent flag set is the empty array).
#[napi]
pub fn mail_get_flags(
    loom_path: String,
    workspace: String,
    principal: String,
    mailbox: String,
    uid: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(strings_cbor(
        mail::get_flags(&loom, ns, &principal, &mailbox, &uid).map_err(reason)?,
    )?))
}
/// Replace the flags/labels on the message at `uid` with `flags`, a Loom Canonical CBOR `Array(Text)`
/// buffer. The message must exist.
#[napi]
pub fn mail_set_flags(
    loom_path: String,
    workspace: String,
    principal: String,
    mailbox: String,
    uid: String,
    flags: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let flags = flags_from_cbor(&flags)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    mail::set_flags(&mut loom, ns, &principal, &mailbox, &uid, &flags).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Search mailbox `mailbox` by a case-insensitive substring `text` over the subject and from values.
/// Returns the Loom Canonical CBOR array of per-message `MailMessage` canonical CBOR byte strings (UID
/// order).
#[napi]
pub fn mail_search(
    loom_path: String,
    workspace: String,
    principal: String,
    mailbox: String,
    text: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let records = mail::search(&loom, ns, &principal, &mailbox, &text)
        .map_err(reason)?
        .iter()
        .map(MailMessage::encode)
        .collect();
    Ok(Uint8Array::from(records_cbor(records)?))
}
