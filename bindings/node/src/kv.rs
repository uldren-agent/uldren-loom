//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Put `value` at the typed `key` (Loom Canonical CBOR cell) in map `collection` of `workspace` (UUID or name,
/// created with the `kv` facet if absent). A later put at the same key replaces the value.
#[napi]
pub fn kv_put(
    loom_path: String,
    workspace: String,
    collection: String,
    key: Uint8Array,
    value: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_kv_ns(&mut loom, &workspace)?;
    let key = loom_core::key_from_cbor(&key).map_err(reason)?;
    loom_core::kv_put(&mut loom, ns, &collection, key, value.to_vec()).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Fetch the value at typed `key` in map `collection` of `workspace`, or `null` if the key or map is absent.
#[napi]
pub fn kv_get(
    loom_path: String,
    workspace: String,
    collection: String,
    key: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let key = loom_core::key_from_cbor(&key).map_err(reason)?;
    Ok(loom_core::kv_get(&loom, ns, &collection, &key)
        .map_err(reason)?
        .map(Uint8Array::from))
}
/// Remove typed `key` from map `collection` of `workspace`; returns whether it was present.
#[napi]
pub fn kv_delete(
    loom_path: String,
    workspace: String,
    collection: String,
    key: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let key = loom_core::key_from_cbor(&key).map_err(reason)?;
    let present = loom_core::kv_delete(&mut loom, ns, &collection, &key).map_err(reason)?;
    if present {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(present)
}
/// List map `collection` of `workspace` as the Loom Canonical CBOR array of `[key, value]` pairs in key order
/// (an absent map is the empty array).
#[napi]
pub fn kv_list(
    loom_path: String,
    workspace: String,
    collection: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(
        loom_core::kv_list(&loom, ns, &collection)
            .map_err(reason)?
            .encode(),
    ))
}
/// The entries of map `collection` with `lo <= key < hi` (half-open, key order) as the Loom Canonical CBOR
/// array of `[key, value]` pairs. `lo`/`hi` are typed-cell CBOR keys.
#[napi]
pub fn kv_range(
    loom_path: String,
    workspace: String,
    collection: String,
    lo: Uint8Array,
    hi: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let lo = loom_core::key_from_cbor(&lo).map_err(reason)?;
    let hi = loom_core::key_from_cbor(&hi).map_err(reason)?;
    Ok(Uint8Array::from(
        loom_core::kv_range(&loom, ns, &collection, &lo, &hi)
            .map_err(reason)?
            .encode(),
    ))
}
