//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[napi]
pub fn workspace_create(
    loom_path: String,
    name: Option<String>,
    facet: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let id = random_workspace_id()?;
    let name = name.as_deref().filter(|value| !value.is_empty());
    let ns = match facet.as_deref().filter(|value| !value.is_empty()) {
        Some(facet) => loom
            .registry_mut()
            .create(FacetKind::parse(facet).map_err(reason)?, name, id)
            .map_err(reason)?,
        None => loom
            .registry_mut()
            .create_workspace(name, id)
            .map_err(reason)?,
    };
    save_loom(&mut loom).map_err(reason)?;
    Ok(ns.to_string())
}
#[napi]
pub fn workspace_list_json(loom_path: String, passphrase: Option<String>) -> napi::Result<String> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    Ok(workspace_list_json_inner(&loom))
}
#[napi]
pub fn workspace_rename(
    loom_path: String,
    workspace: String,
    new_name: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom.registry_mut().rename(ns, &new_name).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)
}
#[napi]
pub fn workspace_delete(
    loom_path: String,
    workspace: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom.registry_mut().delete(ns).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)
}
