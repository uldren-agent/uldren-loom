//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::ServeConfig as GeneratedServeConfig;

#[napi]
pub fn serve_listener_configure_json(
    loom_path: String,
    request_json: String,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_listener_configure_json(
            &generated.client,
            generated.session.clone(),
            request_json,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn serve_listener_list_json(
    loom_path: String,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_listener_list_json(
            &generated.client,
            generated.session.clone(),
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn serve_listener_set_enabled_json(
    loom_path: String,
    listener_id: String,
    enabled: bool,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_listener_set_enabled_json(
            &generated.client,
            generated.session.clone(),
            listener_id,
            enabled,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn serve_listener_remove_json(
    loom_path: String,
    listener_id: String,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_listener_remove_json(
            &generated.client,
            generated.session.clone(),
            listener_id,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn serve_web_route_list_json(
    loom_path: String,
    listener_id: String,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_web_route_list_json(
            &generated.client,
            generated.session.clone(),
            listener_id,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn serve_web_route_set_json(
    loom_path: String,
    request_json: String,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_web_route_set_json(
            &generated.client,
            generated.session.clone(),
            request_json,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn serve_web_route_remove_json(
    loom_path: String,
    listener_id: String,
    route_id: String,
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
        <loom_client::LocalLoomClient as GeneratedServeConfig>::serve_web_route_remove_json(
            &generated.client,
            generated.session.clone(),
            listener_id,
            route_id,
        ),
    )
    .map_err(reason)
}
