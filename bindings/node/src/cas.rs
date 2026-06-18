//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Store bytes in the `cas` facet of a workspace (by UUID or name, created if absent); returns the
/// content address (`"algo:hex"`). Idempotent: identical bytes yield the same address.
#[napi]
pub fn cas_put(
    loom_path: String,
    workspace: String,
    content: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_cas_ns(&mut loom, &workspace)?;
    let digest = loom_core::cas_put(&mut loom, ns, &content).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(digest.to_string())
}
/// Fetch the blob addressed by `digest` from a workspace, or `null` if absent. An invalid digest throws
/// `INVALID_ARGUMENT`; a content/digest mismatch throws `INTEGRITY_FAILURE`.
#[napi]
pub fn cas_get(
    loom_path: String,
    workspace: String,
    digest: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let digest = Digest::parse(&digest).map_err(reason)?;
    let found = loom_core::cas_get(&loom, ns, &digest).map_err(reason)?;
    Ok(found.map(Uint8Array::from))
}
/// Whether a blob addressed by `digest` is present in a workspace. An invalid digest throws
/// `INVALID_ARGUMENT`.
#[napi]
pub fn cas_has(
    loom_path: String,
    workspace: String,
    digest: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let digest = Digest::parse(&digest).map_err(reason)?;
    loom_core::cas_has(&loom, ns, &digest).map_err(reason)
}
/// Drop the blob addressed by `digest` from a workspace's working tree, making it unreachable going
/// forward; returns whether it was present. CAS stays immutable: the bytes are reclaimed by GC once
/// unreferenced, and an earlier commit that held the blob still restores it.
#[napi]
pub fn cas_delete(
    loom_path: String,
    workspace: String,
    digest: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let digest = Digest::parse(&digest).map_err(reason)?;
    let present = loom_core::cas_delete(&mut loom, ns, &digest).map_err(reason)?;
    if present {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(present)
}
/// List the content addresses (`"algo:hex"`) reachable in a workspace's `cas` facet as a JSON string
/// array, sorted. Enumeration is within the workspace, not a global index.
#[napi]
pub fn cas_list_json(
    loom_path: String,
    workspace: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let list = loom_core::cas_list(&loom, ns).map_err(reason)?;
    let mut out = String::from("[");
    for (i, d) in list.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&d.to_string());
        out.push('"');
    }
    out.push(']');
    Ok(out)
}
