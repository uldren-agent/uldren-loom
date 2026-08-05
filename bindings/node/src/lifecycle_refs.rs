//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::{Lifecycle as GeneratedLifecycle, Refs as GeneratedRefs};

#[napi]
pub fn lifecycle_define_standard_json(
    loom_path: String,
    workspace: String,
    kind: String,
    version: String,
    completion_predicate_digest: String,
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
        <loom_client::LocalLoomClient as GeneratedLifecycle>::lifecycle_define_standard_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            kind,
            version,
            completion_predicate_digest,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn lifecycle_define_json(
    loom_path: String,
    workspace: String,
    definition: Uint8Array,
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
        <loom_client::LocalLoomClient as GeneratedLifecycle>::lifecycle_define_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            definition.to_vec(),
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn lifecycle_instantiate_json(
    loom_path: String,
    workspace: String,
    instance_id: String,
    definition_id: String,
    subject_refs: Vec<String>,
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
        <loom_client::LocalLoomClient as GeneratedLifecycle>::lifecycle_instantiate_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            instance_id,
            definition_id,
            subject_refs,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn lifecycle_transition_json(
    loom_path: String,
    workspace: String,
    instance_id: String,
    transition_id: String,
    to_stage_id: String,
    actor_principal_id: Option<String>,
    gate_evaluations_json: String,
    snapshot_digest: Option<String>,
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
        <loom_client::LocalLoomClient as GeneratedLifecycle>::lifecycle_transition_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            instance_id,
            transition_id,
            to_stage_id,
            actor_principal_id,
            gate_evaluations_json,
            snapshot_digest,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn refs_reconcile_json(
    loom_path: String,
    workspace: String,
    max: BigInt,
    store_passphrase: Option<String>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<String> {
    let max = bigint_to_u64(max, "max")?;
    let generated = generated_session::open_generated_session(
        &loom_path,
        store_passphrase.as_deref(),
        auth_principal.as_deref(),
        auth_passphrase.as_deref(),
    )?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedRefs>::refs_reconcile_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            max,
        ),
    )
    .map_err(reason)
}
