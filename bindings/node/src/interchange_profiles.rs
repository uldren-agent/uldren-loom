//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::InterchangeProfiles as GeneratedInterchangeProfiles;

#[napi]
pub fn import_table_csv(
    loom_path: String,
    workspace: String,
    source_scope: String,
    csv_payload: Uint8Array,
    database: String,
    table: String,
    schema: String,
    primary_key: String,
    mode: String,
    commit: bool,
    author: Option<String>,
    message: Option<String>,
    dry_run: bool,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_table_csv(
            &generated.client,
            generated.session.clone(),
            workspace,
            source_scope,
            csv_payload.to_vec(),
            database,
            table,
            schema,
            primary_key,
            mode,
            commit,
            author,
            message,
            dry_run,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn import_redmine(
    loom_path: String,
    workspace: String,
    profile: String,
    source_scope: String,
    snapshot_payload: Uint8Array,
    field_policy: String,
    dry_run: bool,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_redmine(
            &generated.client,
            generated.session.clone(),
            workspace,
            profile,
            source_scope,
            snapshot_payload.to_vec(),
            field_policy,
            dry_run,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn import_asana(
    loom_path: String,
    workspace: String,
    profile: String,
    source_scope: String,
    snapshot_payload: Uint8Array,
    field_policy: String,
    dry_run: bool,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_asana(
            &generated.client,
            generated.session.clone(),
            workspace,
            profile,
            source_scope,
            snapshot_payload.to_vec(),
            field_policy,
            dry_run,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn import_jira(
    loom_path: String,
    workspace: String,
    profile: String,
    source_scope: String,
    snapshot_payload: Uint8Array,
    field_policy: String,
    dry_run: bool,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_jira(
            &generated.client,
            generated.session.clone(),
            workspace,
            profile,
            source_scope,
            snapshot_payload.to_vec(),
            field_policy,
            dry_run,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn import_confluence(
    loom_path: String,
    workspace: String,
    profile: String,
    source_scope: String,
    snapshot_payload: Uint8Array,
    default_space: String,
    dry_run: bool,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_confluence(
            &generated.client,
            generated.session.clone(),
            workspace,
            profile,
            source_scope,
            snapshot_payload.to_vec(),
            default_space,
            dry_run,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn import_slack(
    loom_path: String,
    workspace: String,
    profile: String,
    source_scope: String,
    snapshot_payload: Uint8Array,
    dry_run: bool,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_slack(
            &generated.client,
            generated.session.clone(),
            workspace,
            profile,
            source_scope,
            snapshot_payload.to_vec(),
            dry_run,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn import_drive(
    loom_path: String,
    workspace: String,
    profile: String,
    source_scope: String,
    archive_payload: Uint8Array,
    dry_run: bool,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_drive(
            &generated.client,
            generated.session.clone(),
            workspace,
            profile,
            source_scope,
            archive_payload.to_vec(),
            dry_run,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn import_markdown(
    loom_path: String,
    workspace: String,
    profile: String,
    source_scope: String,
    archive_payload: Uint8Array,
    space: String,
    dry_run: bool,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_markdown(
            &generated.client,
            generated.session.clone(),
            workspace,
            profile,
            source_scope,
            archive_payload.to_vec(),
            space,
            dry_run,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn import_notion(
    loom_path: String,
    workspace: String,
    profile: String,
    source_scope: String,
    snapshot_payload: Uint8Array,
    default_space: String,
    dry_run: bool,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedInterchangeProfiles>::import_notion(
            &generated.client,
            generated.session.clone(),
            workspace,
            profile,
            source_scope,
            snapshot_payload.to_vec(),
            default_space,
            dry_run,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}
