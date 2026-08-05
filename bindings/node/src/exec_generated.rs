//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::Exec as GeneratedExec;

#[napi]
pub fn apply_cbor(
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
    block_on(<loom_client::LocalLoomClient as GeneratedExec>::apply_cbor(
        &generated.client,
        generated.session.clone(),
        request.to_vec(),
    ))
    .map(Uint8Array::from)
    .map_err(reason)
}
