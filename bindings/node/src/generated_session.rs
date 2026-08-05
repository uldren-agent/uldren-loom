//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use loom_client::LocalLoomClient;
use loom_client::types::LoomSession;
use loom_core::WorkspaceId;

use super::*;

pub(crate) struct GeneratedSession {
    pub(crate) client: LocalLoomClient,
    pub(crate) session: LoomSession,
}

impl Drop for GeneratedSession {
    fn drop(&mut self) {
        self.client.close(&self.session);
    }
}

pub(crate) fn open_generated_session(
    path: &str,
    store_passphrase: Option<&str>,
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> napi::Result<GeneratedSession> {
    match (auth_principal, auth_passphrase) {
        (Some(_), Some(_)) | (None, None) => {}
        _ => {
            return Err(napi::Error::from_reason(
                "authPrincipal and authPassphrase must be provided together",
            ));
        }
    }
    let client = LocalLoomClient::new(path);
    let session = match store_passphrase {
        Some(passphrase) => client.open_keyed(passphrase.as_bytes()).map_err(reason)?,
        None => client.open().map_err(reason)?,
    };
    let generated = GeneratedSession { client, session };
    if let (Some(principal), Some(passphrase)) = (auth_principal, auth_passphrase) {
        let principal = WorkspaceId::parse(principal).map_err(reason)?;
        generated
            .client
            .authenticate_passphrase(&generated.session, principal, passphrase.as_bytes())
            .map_err(reason)?;
    }
    Ok(generated)
}
