//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Append `entry` to `stream` in a workspace (by UUID or name, created with the `queue` facet if
/// absent); returns the assigned zero-based sequence.
#[napi]
pub fn queue_append(
    loom_path: String,
    workspace: String,
    stream: String,
    entry: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<BigInt> {
    validate_stream_name(&stream)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_queue_ns(&mut loom, &workspace)?;
    let seq = loom.stream_append(ns, &stream, &entry).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(BigInt::from(seq as u64))
}
/// Fetch the entry at `seq` in `stream`, or `null` if out of range.
#[napi]
pub fn queue_get(
    loom_path: String,
    workspace: String,
    stream: String,
    seq: BigInt,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    validate_stream_name(&stream)?;
    let seq = bigint_to_usize(seq, "seq")?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let found = loom.stream_get(ns, &stream, seq).map_err(reason)?;
    Ok(found.map(Uint8Array::from))
}
/// Read the half-open range `[lo, hi)` of `stream`, oldest first.
#[napi]
pub fn queue_range(
    loom_path: String,
    workspace: String,
    stream: String,
    lo: BigInt,
    hi: BigInt,
    passphrase: Option<String>,
) -> napi::Result<Vec<Uint8Array>> {
    validate_stream_name(&stream)?;
    let lo = bigint_to_usize(lo, "lo")?;
    let hi = bigint_to_usize(hi, "hi")?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let entries = loom.stream_range(ns, &stream, lo, hi).map_err(reason)?;
    Ok(entries.into_iter().map(Uint8Array::from).collect())
}
/// The number of entries in `stream`.
#[napi]
pub fn queue_len(
    loom_path: String,
    workspace: String,
    stream: String,
    passphrase: Option<String>,
) -> napi::Result<BigInt> {
    validate_stream_name(&stream)?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(BigInt::from(
        loom.stream_len(ns, &stream).map_err(reason)? as u64
    ))
}
/// The named consumer's next sequence for `stream`; `0` when none is stored.
#[napi]
pub fn queue_consumer_position(
    loom_path: String,
    workspace: String,
    stream: String,
    consumer_id: String,
    passphrase: Option<String>,
) -> napi::Result<BigInt> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(BigInt::from(
        loom.consumer_position(ns, &stream, &consumer_id)
            .map_err(reason)?,
    ))
}
/// Read up to `max` entries from the consumer's stored next sequence in `stream`; does not advance.
#[napi]
pub fn queue_consumer_read(
    loom_path: String,
    workspace: String,
    stream: String,
    consumer_id: String,
    max: u32,
    passphrase: Option<String>,
) -> napi::Result<Vec<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let entries = loom
        .consumer_read(ns, &stream, &consumer_id, max as usize)
        .map_err(reason)?;
    Ok(entries.into_iter().map(Uint8Array::from).collect())
}
/// Advance the named consumer's next sequence for `stream` to `nextSeq`; rejects backward movement.
#[napi]
pub fn queue_consumer_advance(
    loom_path: String,
    workspace: String,
    stream: String,
    consumer_id: String,
    next_seq: BigInt,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let next_seq = bigint_to_u64(next_seq, "next_seq")?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom.consumer_advance(ns, &stream, &consumer_id, next_seq)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Set the named consumer's next sequence for `stream` to `nextSeq`, which may move backward.
#[napi]
pub fn queue_consumer_reset(
    loom_path: String,
    workspace: String,
    stream: String,
    consumer_id: String,
    next_seq: BigInt,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let next_seq = bigint_to_u64(next_seq, "next_seq")?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom.consumer_reset(ns, &stream, &consumer_id, next_seq)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
