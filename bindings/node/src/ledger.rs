//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Append `payload` to ledger `collection` of `workspace` (created with the `ledger` facet if absent); returns
/// the new zero-based sequence.
#[napi]
pub fn ledger_append(
    loom_path: String,
    workspace: String,
    collection: String,
    payload: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<BigInt> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_ledger_ns(&mut loom, &workspace)?;
    let seq =
        loom_core::ledger_append(&mut loom, ns, &collection, payload.to_vec()).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(BigInt::from(seq))
}
/// The payload at `seq` in ledger `collection`, or `null` if absent.
#[napi]
pub fn ledger_get(
    loom_path: String,
    workspace: String,
    collection: String,
    seq: BigInt,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let seq = bigint_to_usize(seq, "seq")? as u64;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::ledger_get(&loom, ns, &collection, seq)
        .map_err(reason)?
        .map(Uint8Array::from))
}
/// The head chain hash (32 bytes) of ledger `collection`, or `null` when absent or empty.
#[napi]
pub fn ledger_head(
    loom_path: String,
    workspace: String,
    collection: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::ledger_head(&loom, ns, &collection)
        .map_err(reason)?
        .map(|d| Uint8Array::from(d.bytes().to_vec())))
}
/// The number of entries in ledger `collection` (0 when absent).
#[napi]
pub fn ledger_len(
    loom_path: String,
    workspace: String,
    collection: String,
    passphrase: Option<String>,
) -> napi::Result<BigInt> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(BigInt::from(
        loom_core::ledger_len(&loom, ns, &collection).map_err(reason)?,
    ))
}
/// Recompute and verify ledger `collection`'s hash chain; an altered payload or broken link is an error.
#[napi]
pub fn ledger_verify(
    loom_path: String,
    workspace: String,
    collection: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom_core::ledger_verify(&loom, ns, &collection).map_err(reason)?;
    Ok(())
}
