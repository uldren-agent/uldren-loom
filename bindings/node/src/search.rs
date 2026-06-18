//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::{Document, FieldValue, Mapping, QueryRequest, QueryResponse};

// A mapping crosses as a Loom Canonical CBOR map of `field -> [type_tag, stored, faceted]` (type 0
// text, 1 keyword); a document as `field -> value` where each value is CBOR text (analyzed) or CBOR
// bytes (exact); a document id is opaque bytes. A query is a recursive CBOR array tagged by node kind,
// a request is `[query, limit, offset]`, and a response is
// `[reduced, [[id, score_cell, highlights] ...], facets, aggregations]`.

fn mapping_from_cbor(bytes: &[u8]) -> napi::Result<Mapping> {
    loom_core::search_mapping_from_cbor(bytes)
        .map_err(|err| napi::Error::from_reason(err.to_string()))
}

fn field_value_cbor(value: &FieldValue) -> CborValue {
    match value {
        FieldValue::Text(s) => CborValue::Text(s.clone()),
        FieldValue::Bytes(b) => CborValue::Bytes(b.clone()),
    }
}

fn document_to_cbor(doc: &Document) -> Vec<u8> {
    let pairs = doc
        .iter()
        .map(|(field, v)| (CborValue::Text(field.clone()), field_value_cbor(v)))
        .collect();
    cbor_encode(&CborValue::Map(pairs)).unwrap_or_default()
}

fn document_from_cbor(bytes: &[u8]) -> napi::Result<Document> {
    let value =
        loom_codec::decode(bytes).map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))?;
    let CborValue::Map(pairs) = value else {
        return Err(napi::Error::from_reason(
            "search document must be a CBOR map",
        ));
    };
    let mut doc = Document::new();
    for (k, v) in pairs {
        let CborValue::Text(field) = k else {
            return Err(napi::Error::from_reason(
                "search document field name must be text",
            ));
        };
        let value = match v {
            CborValue::Text(s) => FieldValue::Text(s),
            CborValue::Bytes(b) => FieldValue::Bytes(b),
            _ => {
                return Err(napi::Error::from_reason(
                    "search document value must be text or bytes",
                ));
            }
        };
        doc.insert(field, value);
    }
    Ok(doc)
}

fn query_request_from_cbor(bytes: &[u8]) -> napi::Result<QueryRequest> {
    loom_core::search_request_from_cbor(bytes)
        .map_err(|err| napi::Error::from_reason(err.to_string()))
}

fn response_to_cbor(response: &QueryResponse) -> Vec<u8> {
    loom_core::search_response_cbor(response)
}

fn ids_to_cbor(ids: Vec<Vec<u8>>) -> Vec<u8> {
    let items = ids.into_iter().map(CborValue::Bytes).collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

/// Create search collection `name` with the field `mapping` (CBOR `field -> [type_tag, stored, faceted]`,
/// type 0 text, 1 keyword) in workspace `workspace` (UUID or name, created with the `search` facet if
/// absent). `CONFLICT` if the collection already exists.
#[napi]
pub fn search_create(
    loom_path: String,
    workspace: String,
    name: String,
    mapping: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mapping = mapping_from_cbor(&mapping)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_search_ns(&mut loom, &workspace)?;
    loom_core::search_create(&mut loom, ns, &name, mapping).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Insert or replace the document at `id` (opaque bytes) in collection `name`; `doc` is a CBOR
/// `field -> value` map (each value text or bytes). `NOT_FOUND` if the collection was never created.
#[napi]
pub fn search_index(
    loom_path: String,
    workspace: String,
    name: String,
    id: Uint8Array,
    doc: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let doc = document_from_cbor(&doc)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom_core::search_index(&mut loom, ns, &name, id.to_vec(), doc).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Fetch the document at `id` in collection `name` as a CBOR `field -> value` map, or `null` if absent.
/// `NOT_FOUND` if the collection does not exist.
#[napi]
pub fn search_get(
    loom_path: String,
    workspace: String,
    name: String,
    id: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::search_get(&loom, ns, &name, &id)
        .map_err(reason)?
        .map(|d| Uint8Array::from(document_to_cbor(&d))))
}
/// Remove `id` from collection `name`; returns whether it was present. `NOT_FOUND` if the collection
/// does not exist.
#[napi]
pub fn search_delete(
    loom_path: String,
    workspace: String,
    name: String,
    id: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let present = loom_core::search_delete(&mut loom, ns, &name, &id).map_err(reason)?;
    if present {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(present)
}
/// Document ids in collection `name` as the Loom Canonical CBOR array of byte strings. When `hasPrefix`
/// is true only ids starting with `prefix` are returned; otherwise every id is returned. `NOT_FOUND` if
/// the collection does not exist.
#[napi]
pub fn search_ids(
    loom_path: String,
    workspace: String,
    name: String,
    prefix: Uint8Array,
    has_prefix: bool,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let prefix = has_prefix.then(|| prefix.as_ref());
    Ok(Uint8Array::from(ids_to_cbor(
        loom_core::search_ids(&loom, ns, &name, prefix).map_err(reason)?,
    )))
}
/// Replace the field mapping of collection `name` (CBOR `field -> [type_tag, stored, faceted]`).
/// `NOT_FOUND` if the collection does not exist.
#[napi]
pub fn search_remap(
    loom_path: String,
    workspace: String,
    name: String,
    mapping: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mapping = mapping_from_cbor(&mapping)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom_core::search_remap(&mut loom, ns, &name, mapping).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Run the portable linear-scan query over collection `name`. `request` is the CBOR array
/// `[query, limit, offset]` (`query` a recursive node: `[0, field, text]` match, `[1, field, value]`
/// term, `[2, field, [terms], slop]` phrase, `[3, field, lower, upper, incl_lower, incl_upper]` range,
/// `[4, [must], [should], [must_not]]` bool). The response is the CBOR array
/// `[reduced, [[id, score_cell, highlights] ...], facets, aggregations]`. `NOT_FOUND` if the collection
/// does not exist; `NO_SUCH_FIELD` for an unmapped query field.
#[napi]
pub fn search_query(
    loom_path: String,
    workspace: String,
    name: String,
    request: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let request = query_request_from_cbor(&request)?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(response_to_cbor(
        &loom_core::search_query(&loom, ns, &name, &request).map_err(reason)?,
    )))
}
