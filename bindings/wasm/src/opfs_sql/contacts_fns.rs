//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[wasm_bindgen]
impl LoomSql {
    /// Create (or replace the metadata of) address book `book` under `principal` in workspace
    /// `workspace` (UUID or name, created with the `contacts` facet if absent). `display_name` is the
    /// book's display name.
    pub fn card_create_book(
        &mut self,
        workspace: String,
        principal: String,
        book: String,
        display_name: String,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_card_ns(&mut self.loom, &workspace)?;
        let meta = BookMeta { display_name };
        contacts::create_book(&mut self.loom, ns, &principal, &book, &meta).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Delete address book `book` under `principal` and every contact in it; returns whether it
    /// existed.
    pub fn card_delete_book(
        &mut self,
        workspace: String,
        principal: String,
        book: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let existed = contacts::delete_book(&mut self.loom, ns, &principal, &book).map_err(le)?;
        if existed {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(existed)
    }

    /// List the address-book ids under `principal` as the Loom Canonical CBOR array of text strings
    /// (sorted; an absent principal is the empty array).
    pub fn card_list_books(
        &self,
        workspace: String,
        principal: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        strings_cbor(contacts::list_books(&self.loom, ns, &principal)?)
    }

    /// Put the contact `entry` (its `ContactEntry` canonical CBOR) into the existing address book
    /// `book` under `principal`, keyed by its UID. A later put at the same UID replaces it.
    pub fn card_put_entry(
        &mut self,
        workspace: String,
        principal: String,
        book: String,
        entry: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let entry = ContactEntry::decode(&entry).map_err(le)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        contacts::put_entry(&mut self.loom, ns, &principal, &book, &entry).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Fetch the contact at `uid` in address book `book` as its `ContactEntry` canonical CBOR, or
    /// null if absent.
    pub fn card_get_entry(
        &self,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(contacts::get_entry(&self.loom, ns, &principal, &book, &uid)
            .map_err(le)?
            .map(|e| Uint8Array::from(e.encode().as_slice())))
    }

    /// Remove the contact at `uid` in address book `book`; returns whether it was present.
    pub fn card_delete_entry(
        &mut self,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let present =
            contacts::delete_entry(&mut self.loom, ns, &principal, &book, &uid).map_err(le)?;
        if present {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(present)
    }

    /// List address book `book` as the Loom Canonical CBOR array of per-contact `ContactEntry`
    /// canonical CBOR byte strings (UID order; an absent book is the empty array).
    pub fn card_list_entries(
        &self,
        workspace: String,
        principal: String,
        book: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let records = contacts::list_entries(&self.loom, ns, &principal, &book)
            .map_err(le)?
            .iter()
            .map(ContactEntry::encode)
            .collect();
        records_cbor(records)
    }

    /// Search address book `book` by a case-insensitive substring `text` over the formatted name,
    /// organization, and email values. Returns the Loom Canonical CBOR array of per-contact
    /// `ContactEntry` canonical CBOR byte strings (UID order).
    pub fn card_search(
        &self,
        workspace: String,
        principal: String,
        book: String,
        text: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let records = contacts::search(&self.loom, ns, &principal, &book, &text)
            .map_err(le)?
            .iter()
            .map(ContactEntry::encode)
            .collect();
        records_cbor(records)
    }

    /// The on-demand vCard (`.vcf`) projection of the contact at `uid`, or null if absent.
    pub fn card_entry_vcard(
        &self,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
    ) -> Result<Option<String>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        contacts::entry_vcard(&self.loom, ns, &principal, &book, &uid).map_err(le)
    }

    /// Parse vCard document `vcf` and store it as a record in address book `book` (the validated
    /// write-in path); returns the new ETag as a `"algo:hex"` string.
    pub fn card_put_vcard(
        &mut self,
        workspace: String,
        principal: String,
        book: String,
        vcf: String,
    ) -> Result<String, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let etag = contacts::put_vcard(&mut self.loom, ns, &principal, &book, &vcf).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(etag.to_string())
    }
}
