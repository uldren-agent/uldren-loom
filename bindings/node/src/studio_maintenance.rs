//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::StudioMaintenance as GeneratedStudioMaintenance;

#[napi]
pub fn studio_reindex_json(
    loom_path: String,
    workspace: String,
    profile: String,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedStudioMaintenance>::studio_reindex_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            profile,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn studio_revisions_rebuild_json(
    loom_path: String,
    workspace: String,
    profile: String,
    dry_run: bool,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<String> {
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedStudioMaintenance>::studio_revisions_rebuild_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            profile,
            dry_run,
        ),
    )
    .map_err(reason)
}
