//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use futures::executor::block_on;
use loom_client::generated_api::Sql as GeneratedSql;

#[napi]
pub fn sql_exec_result(
    loom_path: String,
    workspace: String,
    db: String,
    sql: String,
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
        <loom_client::LocalLoomClient as GeneratedSql>::sql_exec_result(
            &generated.client,
            generated.session.clone(),
            workspace,
            db,
            sql,
        ),
    )
    .map(Uint8Array::from)
    .map_err(reason)
}
