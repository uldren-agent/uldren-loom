//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::Audit as GeneratedAudit;
use loom_client::generated_api::StoreAdmin as GeneratedStoreAdmin;

#[napi]
pub fn audit_compact(
    loom_path: String,
    through_seq: BigInt,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let through_seq = bigint_to_u64(through_seq, "throughSeq")?;
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedAudit>::audit_compact(
            &generated.client,
            generated.session.clone(),
            through_seq,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn store_bundle_import(
    loom_path: String,
    bundle: Uint8Array,
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
        <loom_client::LocalLoomClient as GeneratedStoreAdmin>::store_bundle_import(
            &generated.client,
            generated.session.clone(),
            bundle.to_vec(),
            dry_run,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn store_maintenance_status(
    loom_path: String,
    request: Uint8Array,
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
        <loom_client::LocalLoomClient as GeneratedStoreAdmin>::store_maintenance_status(
            &generated.client,
            generated.session.clone(),
            request.to_vec(),
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn store_maintenance_policy_set(
    loom_path: String,
    update: Uint8Array,
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
        <loom_client::LocalLoomClient as GeneratedStoreAdmin>::store_maintenance_policy_set(
            &generated.client,
            generated.session.clone(),
            update.to_vec(),
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}

#[napi]
pub fn store_maintenance_run(
    loom_path: String,
    request: Uint8Array,
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
        <loom_client::LocalLoomClient as GeneratedStoreAdmin>::store_maintenance_run(
            &generated.client,
            generated.session.clone(),
            request.to_vec(),
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}
