//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[wasm_bindgen]
impl LoomSql {
    /// Create (or replace the metadata of) calendar collection `collection` under `principal` in
    /// workspace `workspace` (UUID or name, created with the `calendar` facet if absent).
    /// `display_name` is the collection's display name; `components` is a comma-separated component
    /// set ("event,todo"; "" is the empty set).
    pub fn cal_create_collection(
        &mut self,
        workspace: String,
        principal: String,
        collection: String,
        display_name: String,
        components: String,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_cal_ns(&mut self.loom, &workspace)?;
        let meta = CollectionMeta {
            display_name,
            component_set: parse_component_set(&components)?,
        };
        calendar::create_collection(&mut self.loom, ns, &principal, &collection, &meta)
            .map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Delete calendar collection `collection` under `principal` and every entry in it; returns
    /// whether it existed.
    pub fn cal_delete_collection(
        &mut self,
        workspace: String,
        principal: String,
        collection: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let existed =
            calendar::delete_collection(&mut self.loom, ns, &principal, &collection).map_err(le)?;
        if existed {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(existed)
    }

    /// List the calendar collection ids under `principal` as the Loom Canonical CBOR array of text
    /// strings (sorted; an absent principal is the empty array).
    pub fn cal_list_collections(
        &self,
        workspace: String,
        principal: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        strings_cbor(calendar::list_collections(&self.loom, ns, &principal)?)
    }

    /// Put the calendar `entry` (its `CalendarEntry` canonical CBOR) into the existing collection
    /// `collection` under `principal`, keyed by its UID. A later put at the same UID replaces it.
    pub fn cal_put_entry(
        &mut self,
        workspace: String,
        principal: String,
        collection: String,
        entry: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let entry = CalendarEntry::decode(&entry).map_err(le)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        calendar::put_entry(&mut self.loom, ns, &principal, &collection, &entry).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Fetch the calendar entry at `uid` in collection `collection` as its `CalendarEntry` canonical
    /// CBOR, or null if absent.
    pub fn cal_get_entry(
        &self,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(
            calendar::get_entry(&self.loom, ns, &principal, &collection, &uid)
                .map_err(le)?
                .map(|e| Uint8Array::from(e.encode().as_slice())),
        )
    }

    /// Remove the calendar entry at `uid` in collection `collection`; returns whether it was present.
    pub fn cal_delete_entry(
        &mut self,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let present = calendar::delete_entry(&mut self.loom, ns, &principal, &collection, &uid)
            .map_err(le)?;
        if present {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(present)
    }

    /// List collection `collection` as the Loom Canonical CBOR array of per-entry `CalendarEntry`
    /// canonical CBOR byte strings (UID order; an absent collection is the empty array).
    pub fn cal_list_entries(
        &self,
        workspace: String,
        principal: String,
        collection: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let records = calendar::list_entries(&self.loom, ns, &principal, &collection)
            .map_err(le)?
            .iter()
            .map(CalendarEntry::encode)
            .collect();
        records_cbor(records)
    }

    /// Expand collection `collection` into occurrences within the half-open wall-clock window
    /// `[from, to)` (both `YYYYMMDDTHHMMSS`). Returns the Loom Canonical CBOR array of
    /// `[uid, "YYYYMMDDTHHMMSS"]` pairs (start order, then UID).
    pub fn cal_range(
        &self,
        workspace: String,
        principal: String,
        collection: String,
        from: String,
        to: String,
    ) -> Result<Vec<u8>, JsError> {
        let from = parse_window_bound(&from, "from")?;
        let to = parse_window_bound(&to, "to")?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let occ = calendar::range(&self.loom, ns, &principal, &collection, from, to).map_err(le)?;
        let items = occ
            .into_iter()
            .map(|o| {
                CborValue::Array(vec![
                    CborValue::Text(o.uid),
                    CborValue::Text(format_window_bound(&o.start)),
                ])
            })
            .collect();
        cbor_encode(&CborValue::Array(items)).map_err(|e| JsError::new(&format!("cbor: {e}")))
    }

    /// Search collection `collection` by component filter and substring. `component` is "" (any),
    /// "event", or "todo"; `text` is a case-insensitive substring over the summary ("" matches any).
    /// Returns the Loom Canonical CBOR array of per-entry `CalendarEntry` canonical CBOR byte strings.
    pub fn cal_search(
        &self,
        workspace: String,
        principal: String,
        collection: String,
        component: String,
        text: String,
    ) -> Result<Vec<u8>, JsError> {
        let component = parse_component_filter(&component)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let records = calendar::search(
            &self.loom,
            ns,
            &principal,
            &collection,
            component,
            if text.is_empty() {
                None
            } else {
                Some(text.as_str())
            },
        )
        .map_err(le)?
        .iter()
        .map(CalendarEntry::encode)
        .collect();
        records_cbor(records)
    }

    /// The on-demand iCalendar (`.ics`) projection of the entry at `uid`, or null if absent.
    pub fn cal_entry_ics(
        &self,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
    ) -> Result<Option<String>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        calendar::entry_ics(&self.loom, ns, &principal, &collection, &uid).map_err(le)
    }

    /// Parse iCalendar document `ics` and store it as a record in collection `collection` (the
    /// validated write-in path); returns the new ETag as a `"algo:hex"` string.
    pub fn cal_put_ics(
        &mut self,
        workspace: String,
        principal: String,
        collection: String,
        ics: String,
    ) -> Result<String, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let etag =
            calendar::put_ics(&mut self.loom, ns, &principal, &collection, &ics).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(etag.to_string())
    }
}
