//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Create (or replace the metadata of) calendar collection `collection` under `principal` in workspace
/// `workspace` (UUID or name, created with the `calendar` facet if absent). `displayName` is the
/// collection's display name; `components` is a comma-separated component set ("event,todo"; "" is the
/// empty set).
#[napi]
pub fn cal_create_collection(
    loom_path: String,
    workspace: String,
    principal: String,
    collection: String,
    display_name: String,
    components: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_cal_ns(&mut loom, &workspace)?;
    let meta = CollectionMeta {
        display_name,
        component_set: parse_component_set(&components)?,
    };
    calendar::create_collection(&mut loom, ns, &principal, &collection, &meta).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Delete calendar collection `collection` under `principal` and every entry in it; returns whether it
/// existed.
#[napi]
pub fn cal_delete_collection(
    loom_path: String,
    workspace: String,
    principal: String,
    collection: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let existed =
        calendar::delete_collection(&mut loom, ns, &principal, &collection).map_err(reason)?;
    if existed {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(existed)
}
/// List the calendar collection ids under `principal` as the Loom Canonical CBOR array of text strings
/// (sorted; an absent principal is the empty array).
#[napi]
pub fn cal_list_collections(
    loom_path: String,
    workspace: String,
    principal: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(strings_cbor(
        calendar::list_collections(&loom, ns, &principal).map_err(reason)?,
    )?))
}
/// Put the calendar `entry` (its `CalendarEntry` canonical CBOR) into the existing collection
/// `collection` under `principal`, keyed by its UID. A later put at the same UID replaces it.
#[napi]
pub fn cal_put_entry(
    loom_path: String,
    workspace: String,
    principal: String,
    collection: String,
    entry: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let entry = CalendarEntry::decode(&entry).map_err(reason)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    calendar::put_entry(&mut loom, ns, &principal, &collection, &entry).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Fetch the calendar entry at `uid` in collection `collection` as its `CalendarEntry` canonical CBOR, or
/// `null` if absent.
#[napi]
pub fn cal_get_entry(
    loom_path: String,
    workspace: String,
    principal: String,
    collection: String,
    uid: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(
        calendar::get_entry(&loom, ns, &principal, &collection, &uid)
            .map_err(reason)?
            .map(|e| e.encode())
            .map(Uint8Array::from),
    )
}
/// Remove the calendar entry at `uid` in collection `collection`; returns whether it was present.
#[napi]
pub fn cal_delete_entry(
    loom_path: String,
    workspace: String,
    principal: String,
    collection: String,
    uid: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let present =
        calendar::delete_entry(&mut loom, ns, &principal, &collection, &uid).map_err(reason)?;
    if present {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(present)
}
/// List collection `collection` as the Loom Canonical CBOR array of per-entry `CalendarEntry` canonical
/// CBOR byte strings (UID order; an absent collection is the empty array).
#[napi]
pub fn cal_list_entries(
    loom_path: String,
    workspace: String,
    principal: String,
    collection: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let records = calendar::list_entries(&loom, ns, &principal, &collection)
        .map_err(reason)?
        .iter()
        .map(CalendarEntry::encode)
        .collect();
    Ok(Uint8Array::from(records_cbor(records)?))
}
/// Expand collection `collection` into occurrences within the half-open wall-clock window `[from, to)`
/// (both `YYYYMMDDTHHMMSS`). Returns the Loom Canonical CBOR array of `[uid, "YYYYMMDDTHHMMSS"]` pairs
/// (start order, then UID).
#[napi]
pub fn cal_range(
    loom_path: String,
    workspace: String,
    principal: String,
    collection: String,
    from: String,
    to: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let from = parse_window_bound(&from, "from")?;
    let to = parse_window_bound(&to, "to")?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let occ = calendar::range(&loom, ns, &principal, &collection, from, to).map_err(reason)?;
    let items = occ
        .into_iter()
        .map(|o| {
            CborValue::Array(vec![
                CborValue::Text(o.uid),
                CborValue::Text(format_window_bound(&o.start)),
            ])
        })
        .collect();
    let bytes = cbor_encode(&CborValue::Array(items))
        .map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))?;
    Ok(Uint8Array::from(bytes))
}
/// Search collection `collection` by component filter and substring. `component` is "" (any), "event", or
/// "todo"; `text` is a case-insensitive substring over the summary ("" matches any). Returns the Loom
/// Canonical CBOR array of per-entry `CalendarEntry` canonical CBOR byte strings (UID order).
#[napi]
pub fn cal_search(
    loom_path: String,
    workspace: String,
    principal: String,
    collection: String,
    component: String,
    text: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let component = parse_component_filter(&component)?;
    let text = if text.is_empty() {
        None
    } else {
        Some(text.as_str())
    };
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let records = calendar::search(&loom, ns, &principal, &collection, component, text)
        .map_err(reason)?
        .iter()
        .map(CalendarEntry::encode)
        .collect();
    Ok(Uint8Array::from(records_cbor(records)?))
}
/// The on-demand iCalendar (`.ics`) projection of the entry at `uid`, or `null` if absent.
#[napi]
pub fn cal_entry_ics(
    loom_path: String,
    workspace: String,
    principal: String,
    collection: String,
    uid: String,
    passphrase: Option<String>,
) -> napi::Result<Option<String>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    calendar::entry_ics(&loom, ns, &principal, &collection, &uid).map_err(reason)
}
/// Parse iCalendar document `ics` and store it as a record in collection `collection` (the validated
/// write-in path); returns the new ETag as a `"algo:hex"` string.
#[napi]
pub fn cal_put_ics(
    loom_path: String,
    workspace: String,
    principal: String,
    collection: String,
    ics: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let etag = calendar::put_ics(&mut loom, ns, &principal, &collection, &ics).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(etag.to_string())
}
