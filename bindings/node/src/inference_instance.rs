//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::InferenceInstance as GeneratedInferenceInstance;

#[napi]
pub fn inference_instance_create_json(
    loom_path: String,
    workspace: String,
    name: String,
    model: String,
    kind: String,
    runtime: String,
    preset: Option<String>,
    settings_json: String,
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
        <loom_client::LocalLoomClient as GeneratedInferenceInstance>::inference_instance_create_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            name,
            model,
            kind,
            runtime,
            preset,
            settings_json,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn inference_instance_update_json(
    loom_path: String,
    workspace: String,
    name: String,
    preset: Option<String>,
    settings_json: String,
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
        <loom_client::LocalLoomClient as GeneratedInferenceInstance>::inference_instance_update_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            name,
            preset,
            settings_json,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn inference_instance_delete_json(
    loom_path: String,
    workspace: String,
    name: String,
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
        <loom_client::LocalLoomClient as GeneratedInferenceInstance>::inference_instance_delete_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            name,
        ),
    )
    .map_err(reason)
}
