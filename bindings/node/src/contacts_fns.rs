//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Create (or replace the metadata of) address book `book` under `principal` in workspace `workspace`
/// (UUID or name, created with the `contacts` facet if absent). `displayName` is the book's display name.
#[napi]
pub fn card_create_book(
    loom_path: String,
    workspace: String,
    principal: String,
    book: String,
    display_name: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_card_ns(&mut loom, &workspace)?;
    let meta = BookMeta { display_name };
    contacts::create_book(&mut loom, ns, &principal, &book, &meta).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Delete address book `book` under `principal` and every contact in it; returns whether it existed.
#[napi]
pub fn card_delete_book(
    loom_path: String,
    workspace: String,
    principal: String,
    book: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let existed = contacts::delete_book(&mut loom, ns, &principal, &book).map_err(reason)?;
    if existed {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(existed)
}
/// List the address-book ids under `principal` as the Loom Canonical CBOR array of text strings (sorted;
/// an absent principal is the empty array).
#[napi]
pub fn card_list_books(
    loom_path: String,
    workspace: String,
    principal: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(strings_cbor(
        contacts::list_books(&loom, ns, &principal).map_err(reason)?,
    )?))
}
/// Put the contact `entry` (its `ContactEntry` canonical CBOR) into the existing address book `book` under
/// `principal`, keyed by its UID. A later put at the same UID replaces it.
#[napi]
pub fn card_put_entry(
    loom_path: String,
    workspace: String,
    principal: String,
    book: String,
    entry: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let entry = ContactEntry::decode(&entry).map_err(reason)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    contacts::put_entry(&mut loom, ns, &principal, &book, &entry).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Fetch the contact at `uid` in address book `book` as its `ContactEntry` canonical CBOR, or `null` if
/// absent.
#[napi]
pub fn card_get_entry(
    loom_path: String,
    workspace: String,
    principal: String,
    book: String,
    uid: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(contacts::get_entry(&loom, ns, &principal, &book, &uid)
        .map_err(reason)?
        .map(|e| e.encode())
        .map(Uint8Array::from))
}
/// Remove the contact at `uid` in address book `book`; returns whether it was present.
#[napi]
pub fn card_delete_entry(
    loom_path: String,
    workspace: String,
    principal: String,
    book: String,
    uid: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let present = contacts::delete_entry(&mut loom, ns, &principal, &book, &uid).map_err(reason)?;
    if present {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(present)
}
/// List address book `book` as the Loom Canonical CBOR array of per-contact `ContactEntry` canonical CBOR
/// byte strings (UID order; an absent book is the empty array).
#[napi]
pub fn card_list_entries(
    loom_path: String,
    workspace: String,
    principal: String,
    book: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let records = contacts::list_entries(&loom, ns, &principal, &book)
        .map_err(reason)?
        .iter()
        .map(ContactEntry::encode)
        .collect();
    Ok(Uint8Array::from(records_cbor(records)?))
}
/// Search address book `book` by a case-insensitive substring `text` over the formatted name,
/// organization, and email values. Returns the Loom Canonical CBOR array of per-contact `ContactEntry`
/// canonical CBOR byte strings (UID order).
#[napi]
pub fn card_search(
    loom_path: String,
    workspace: String,
    principal: String,
    book: String,
    text: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let records = contacts::search(&loom, ns, &principal, &book, &text)
        .map_err(reason)?
        .iter()
        .map(ContactEntry::encode)
        .collect();
    Ok(Uint8Array::from(records_cbor(records)?))
}
/// The on-demand vCard (`.vcf`) projection of the contact at `uid`, or `null` if absent.
#[napi]
pub fn card_entry_vcard(
    loom_path: String,
    workspace: String,
    principal: String,
    book: String,
    uid: String,
    passphrase: Option<String>,
) -> napi::Result<Option<String>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    contacts::entry_vcard(&loom, ns, &principal, &book, &uid).map_err(reason)
}
/// Parse vCard document `vcf` and store it as a record in address book `book` (the validated write-in
/// path); returns the new ETag as a `"algo:hex"` string.
#[napi]
pub fn card_put_vcard(
    loom_path: String,
    workspace: String,
    principal: String,
    book: String,
    vcf: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let etag = contacts::put_vcard(&mut loom, ns, &principal, &book, &vcf).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(etag.to_string())
}
