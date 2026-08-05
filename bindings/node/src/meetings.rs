//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use futures::executor::block_on;
use loom_client::generated_api::Meetings as GeneratedMeetings;
use loom_interchange_io::{meetings_source_payload_path, validate_meetings_source_payload_leaf};

#[napi]
pub fn meetings_import_snapshot(
    loom_path: String,
    workspace: String,
    input_profile: String,
    snapshot: Uint8Array,
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
        <loom_client::LocalLoomClient as GeneratedMeetings>::meetings_import_snapshot(
            &generated.client,
            generated.session.clone(),
            workspace,
            input_profile,
            snapshot.to_vec(),
            dry_run,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn meetings_source_read(
    loom_path: String,
    workspace: String,
    source_id: String,
    leaf: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let workspace_id = resolve_workspace_arg(&loom, &workspace)?;
    validate_meetings_source_payload_leaf(&leaf).map_err(reason)?;
    let profile_id = workspace_id.to_string();
    let path = meetings_source_payload_path(&profile_id, &source_id, &leaf);
    let bytes = loom
        .read_file_reserved(workspace_id, &path)
        .map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}
