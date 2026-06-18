//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[wasm_bindgen]
impl LoomSql {
    /// Create (or replace the metadata of) mailbox `mailbox` under `principal` in workspace
    /// `workspace` (UUID or name, created with the `mail` facet if absent). `display_name` is the
    /// mailbox's display name.
    pub fn mail_create_mailbox(
        &mut self,
        workspace: String,
        principal: String,
        mailbox: String,
        display_name: String,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_mail_ns(&mut self.loom, &workspace)?;
        let meta = MailboxMeta { display_name };
        mail::create_mailbox(&mut self.loom, ns, &principal, &mailbox, &meta).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Delete mailbox `mailbox` under `principal` and every message in it; returns whether it
    /// existed.
    pub fn mail_delete_mailbox(
        &mut self,
        workspace: String,
        principal: String,
        mailbox: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let existed = mail::delete_mailbox(&mut self.loom, ns, &principal, &mailbox).map_err(le)?;
        if existed {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(existed)
    }

    /// List the mailbox ids under `principal` as the Loom Canonical CBOR array of text strings
    /// (sorted; an absent principal is the empty array).
    pub fn mail_list_mailboxes(
        &self,
        workspace: String,
        principal: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        strings_cbor(mail::list_mailboxes(&self.loom, ns, &principal)?)
    }

    /// Ingest the raw RFC 5322 message `raw` into mailbox `mailbox` under `principal`, keyed by
    /// `uid`; returns the body's content address as a `"algo:hex"` string.
    pub fn mail_ingest_message(
        &mut self,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        raw: Vec<u8>,
    ) -> Result<String, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let body = mail::ingest_message(&mut self.loom, ns, &principal, &mailbox, &uid, &raw)
            .map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(body.to_string())
    }

    /// Fetch the message index record at `uid` in mailbox `mailbox` as its `MailMessage` canonical
    /// CBOR, or null if absent.
    pub fn mail_get_message(
        &self,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(
            mail::get_message(&self.loom, ns, &principal, &mailbox, &uid)
                .map_err(le)?
                .map(|m| Uint8Array::from(m.encode().as_slice())),
        )
    }

    /// Fetch the raw RFC 5322 body at `uid` in mailbox `mailbox`, or null if absent.
    pub fn mail_to_eml(
        &self,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(mail::to_eml(&self.loom, ns, &principal, &mailbox, &uid)
            .map_err(le)?
            .map(|b| Uint8Array::from(b.as_slice())))
    }

    /// Remove the message at `uid` in mailbox `mailbox`; returns whether it was present.
    pub fn mail_delete_message(
        &mut self,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let present =
            mail::delete_message(&mut self.loom, ns, &principal, &mailbox, &uid).map_err(le)?;
        if present {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(present)
    }

    /// List mailbox `mailbox` as the Loom Canonical CBOR array of per-message `MailMessage` canonical
    /// CBOR byte strings (UID order; an absent mailbox is the empty array).
    pub fn mail_list_messages(
        &self,
        workspace: String,
        principal: String,
        mailbox: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let records = mail::list_messages(&self.loom, ns, &principal, &mailbox)
            .map_err(le)?
            .iter()
            .map(MailMessage::encode)
            .collect();
        records_cbor(records)
    }

    /// The flags of the message at `uid` in mailbox `mailbox` as the Loom Canonical CBOR array of
    /// text strings.
    pub fn mail_get_flags(
        &self,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        strings_cbor(mail::get_flags(&self.loom, ns, &principal, &mailbox, &uid).map_err(le)?)
    }

    /// Set the flags of the message at `uid` in mailbox `mailbox`. `flags` is a canonical-CBOR
    /// `Array(Text)` byte buffer.
    pub fn mail_set_flags(
        &mut self,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        flags: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let flags = flags_from_cbor(&flags)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        mail::set_flags(&mut self.loom, ns, &principal, &mailbox, &uid, &flags).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Search mailbox `mailbox` by a case-insensitive substring `text` over the message headers.
    /// Returns the Loom Canonical CBOR array of per-message `MailMessage` canonical CBOR byte strings.
    pub fn mail_search(
        &self,
        workspace: String,
        principal: String,
        mailbox: String,
        text: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let records = mail::search(&self.loom, ns, &principal, &mailbox, &text)
            .map_err(le)?
            .iter()
            .map(MailMessage::encode)
            .collect();
        records_cbor(records)
    }
}
