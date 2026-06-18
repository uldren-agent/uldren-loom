//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Put `value` at timestamp `ts` in series `collection` of `workspace` (created with the `time-series` facet if
/// absent); a repeated timestamp replaces the point.
#[napi]
pub fn ts_put(
    loom_path: String,
    workspace: String,
    collection: String,
    ts: i64,
    value: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_ts_ns(&mut loom, &workspace)?;
    loom_core::ts_put(&mut loom, ns, &collection, ts, value.to_vec()).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// The point at timestamp `ts` in series `collection`, or `null` if absent.
#[napi]
pub fn ts_get(
    loom_path: String,
    workspace: String,
    collection: String,
    ts: i64,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::ts_get(&loom, ns, &collection, ts)
        .map_err(reason)?
        .map(Uint8Array::from))
}
/// The points of series `collection` with `from <= ts < to` (half-open, time order) as the Loom Canonical CBOR
/// array of `[ts, value]` pairs.
#[napi]
pub fn ts_range(
    loom_path: String,
    workspace: String,
    collection: String,
    from: i64,
    to: i64,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(
        loom_core::ts_range(&loom, ns, &collection, from, to)
            .map_err(reason)?
            .encode(),
    ))
}
/// The most recent point of series `collection` as a one-point Loom Canonical CBOR array, or `null` if the
/// series is absent or empty.
#[napi]
pub fn ts_latest(
    loom_path: String,
    workspace: String,
    collection: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::ts_latest(&loom, ns, &collection)
        .map_err(reason)?
        .map(|(ts, v)| {
            let mut s = loom_core::Series::new();
            s.put(ts, v);
            Uint8Array::from(s.encode())
        }))
}
