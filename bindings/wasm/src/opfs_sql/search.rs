//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::{
    Document, FieldValue, Mapping, QueryRequest, QueryResponse, search_create, search_delete,
    search_get, search_ids, search_index, search_query, search_remap,
};

// Search collection (Search facet) - a versioned field mapping plus an id-keyed document map in a
// named collection within a workspace, with the portable linear-scan query fallback. A mapping
// crosses as a Loom Canonical CBOR map of `field -> [type_tag, stored, faceted]` (type 0 text, 1
// keyword); a document as `field -> value` where each value is CBOR text (analyzed) or CBOR bytes
// (exact); a document id is opaque bytes. A query is a recursive CBOR array tagged by node kind, a
// request is `[query, limit, offset]`, and a response is
// `[reduced, [[id, score_cell, highlights] ...], facets, aggregations]`.

/// Resolve a workspace for a search write by UUID or name, ensuring the `search` facet exists.
fn ensure_search_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Search,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Search)
        .map_err(le)?;
    Ok(ns)
}

/// Decode a canonical-CBOR `field -> [type_tag, stored, faceted]` map into a [`Mapping`].
fn mapping_from_cbor(bytes: &[u8]) -> Result<Mapping, JsError> {
    loom_core::search_mapping_from_cbor(bytes).map_err(|err| JsError::new(&err.to_string()))
}

/// One [`FieldValue`] as canonical CBOR: text (analyzed) or bytes (exact).
fn field_value_cbor(value: &FieldValue) -> CborValue {
    match value {
        FieldValue::Text(s) => CborValue::Text(s.clone()),
        FieldValue::Bytes(b) => CborValue::Bytes(b.clone()),
    }
}

/// Encode a [`Document`] as the canonical-CBOR `field -> value` map wire form.
fn document_to_cbor(doc: &Document) -> Vec<u8> {
    let pairs = doc
        .iter()
        .map(|(field, v)| (CborValue::Text(field.clone()), field_value_cbor(v)))
        .collect();
    cbor_encode(&CborValue::Map(pairs)).unwrap_or_default()
}

/// Decode a canonical-CBOR `field -> value` map (each value text or bytes) into a [`Document`].
fn document_from_cbor(bytes: &[u8]) -> Result<Document, JsError> {
    let value = loom_codec::decode(bytes).map_err(|e| JsError::new(&format!("cbor: {e}")))?;
    let CborValue::Map(pairs) = value else {
        return Err(JsError::new("search document must be a CBOR map"));
    };
    let mut doc = Document::new();
    for (k, v) in pairs {
        let CborValue::Text(field) = k else {
            return Err(JsError::new("search document field name must be text"));
        };
        let value = match v {
            CborValue::Text(s) => FieldValue::Text(s),
            CborValue::Bytes(b) => FieldValue::Bytes(b),
            _ => return Err(JsError::new("search document value must be text or bytes")),
        };
        doc.insert(field, value);
    }
    Ok(doc)
}

/// Decode the request buffer (the CBOR array `[query, limit, offset]`) into a [`QueryRequest`].
fn query_request_from_cbor(bytes: &[u8]) -> Result<QueryRequest, JsError> {
    loom_core::search_request_from_cbor(bytes).map_err(|err| JsError::new(&err.to_string()))
}

/// Encode a [`QueryResponse`] as the canonical-CBOR array
/// `[reduced, [[id, score_cell, highlights] ...], facets, aggregations]`.
fn response_to_cbor(response: &QueryResponse) -> Vec<u8> {
    loom_core::search_response_cbor(response)
}

/// Encode a list of document ids as a canonical-CBOR array of byte strings.
fn ids_to_cbor(ids: Vec<Vec<u8>>) -> Vec<u8> {
    let items = ids.into_iter().map(CborValue::Bytes).collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

#[wasm_bindgen]
impl LoomSql {
    /// Create search collection `name` with the field `mapping` (CBOR `field -> [type_tag, stored,
    /// faceted]`, type 0 text, 1 keyword) in `workspace` (UUID or name, created with the `search`
    /// facet if absent).
    pub fn search_create(
        &mut self,
        workspace: String,
        name: String,
        mapping: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let mapping = mapping_from_cbor(&mapping)?;
        let ns = ensure_search_ns(&mut self.loom, &workspace)?;
        search_create(&mut self.loom, ns, &name, mapping).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Insert or replace the document at `id` (opaque bytes) in collection `name`; `doc` is a CBOR
    /// `field -> value` map (each value text or bytes).
    pub fn search_index(
        &mut self,
        workspace: String,
        name: String,
        id: Vec<u8>,
        doc: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let doc = document_from_cbor(&doc)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        search_index(&mut self.loom, ns, &name, id, doc).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Fetch the document at `id` in collection `name` as a CBOR `field -> value` map, or null if
    /// absent.
    pub fn search_get(
        &self,
        workspace: String,
        name: String,
        id: Vec<u8>,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(search_get(&self.loom, ns, &name, &id)
            .map_err(le)?
            .map(|d| Uint8Array::from(document_to_cbor(&d).as_slice())))
    }

    /// Remove `id` from collection `name`; returns whether it was present.
    pub fn search_delete(
        &mut self,
        workspace: String,
        name: String,
        id: Vec<u8>,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let present = search_delete(&mut self.loom, ns, &name, &id).map_err(le)?;
        if present {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(present)
    }

    /// Document ids in collection `name` as a CBOR array of byte strings. A null `prefix` returns
    /// every id; otherwise only ids starting with `prefix`.
    pub fn search_ids(
        &self,
        workspace: String,
        name: String,
        prefix: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(ids_to_cbor(
            search_ids(&self.loom, ns, &name, prefix.as_deref()).map_err(le)?,
        ))
    }

    /// Replace the field mapping of collection `name` (CBOR `field -> [type_tag, stored, faceted]`).
    pub fn search_remap(
        &mut self,
        workspace: String,
        name: String,
        mapping: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let mapping = mapping_from_cbor(&mapping)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        search_remap(&mut self.loom, ns, &name, mapping).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Run the portable linear-scan query over collection `name`. `request` is the CBOR array
    /// `[query, limit, offset]` (`query` a recursive node: `[0, field, text]` match, `[1, field,
    /// value]` term, `[2, field, [terms], slop]` phrase, `[3, field, lower, upper, incl_lower,
    /// incl_upper]` range, `[4, [must], [should], [must_not]]` bool). The response is the CBOR array
    /// `[reduced, [[id, score_cell, highlights] ...], facets, aggregations]`.
    pub fn search_query(
        &self,
        workspace: String,
        name: String,
        request: Vec<u8>,
    ) -> Result<Vec<u8>, JsError> {
        let request = query_request_from_cbor(&request)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(response_to_cbor(
            &search_query(&self.loom, ns, &name, &request).map_err(le)?,
        ))
    }
}
