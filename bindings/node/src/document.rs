//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[napi(object)]
pub struct DocText {
    pub text: String,
    pub digest: String,
    pub entity_tag: String,
}

#[napi(object)]
pub struct DocBinary {
    pub bytes: Uint8Array,
    pub digest: String,
    pub entity_tag: String,
}

#[napi(object)]
pub struct DocPutResult {
    pub digest: String,
    pub entity_tag: String,
}

/// Put UTF-8 text at string `id` in collection `collection` and return the new document tags.
#[napi]
pub fn doc_put_text(
    loom_path: String,
    workspace: String,
    collection: String,
    id: String,
    text: String,
    expected_entity_tag: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<DocPutResult> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_doc_ns(&mut loom, &workspace)?;
    let result = loom_core::document_put_text_with_entity_tag(
        &mut loom,
        ns,
        &collection,
        &id,
        &text,
        expected_entity_tag.as_deref(),
    )
    .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(DocPutResult {
        digest: result.digest.to_string(),
        entity_tag: result.entity_tag,
    })
}

/// Fetch `id` as UTF-8 text with its content digest, or `null` if absent.
#[napi]
pub fn doc_get_text(
    loom_path: String,
    workspace: String,
    collection: String,
    id: String,
    passphrase: Option<String>,
) -> napi::Result<Option<DocText>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::document_get_text(&loom, ns, &collection, &id)
        .map_err(reason)?
        .map(|document| DocText {
            text: document.text,
            digest: document.digest.to_string(),
            entity_tag: document.entity_tag,
        }))
}

/// Put binary bytes at string `id` in collection `collection` and return the new document tags.
#[napi]
pub fn doc_put_binary(
    loom_path: String,
    workspace: String,
    collection: String,
    id: String,
    bytes: Uint8Array,
    expected_entity_tag: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<DocPutResult> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_doc_ns(&mut loom, &workspace)?;
    let result = loom_core::document_put_binary_with_entity_tag(
        &mut loom,
        ns,
        &collection,
        &id,
        bytes.to_vec(),
        expected_entity_tag.as_deref(),
    )
    .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(DocPutResult {
        digest: result.digest.to_string(),
        entity_tag: result.entity_tag,
    })
}

/// Fetch `id` as binary bytes with its content digest, or `null` if absent.
#[napi]
pub fn doc_get_binary(
    loom_path: String,
    workspace: String,
    collection: String,
    id: String,
    passphrase: Option<String>,
) -> napi::Result<Option<DocBinary>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::document_get_binary(&loom, ns, &collection, &id)
        .map_err(reason)?
        .map(|document| DocBinary {
            bytes: Uint8Array::from(document.bytes),
            digest: document.digest.to_string(),
            entity_tag: document.entity_tag,
        }))
}

/// Remove `id` from collection `collection`; returns whether it was present.
#[napi]
pub fn doc_delete(
    loom_path: String,
    workspace: String,
    collection: String,
    id: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let present = loom_core::doc_delete(&mut loom, ns, &collection, &id).map_err(reason)?;
    if present {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(present)
}
/// List collection `collection` as its canonical binary representation.
#[napi]
pub fn doc_list_binary(
    loom_path: String,
    workspace: String,
    collection: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(
        loom_core::document_list_binary(&loom, ns, &collection).map_err(reason)?,
    ))
}

#[napi]
pub fn doc_index_create(
    loom_path: String,
    workspace: String,
    collection: String,
    name: String,
    field_path: String,
    unique: bool,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let index = loom_core::DocumentIndexDef::new(
        &name,
        loom_core::DocumentFieldPath::dotted(&field_path).map_err(reason)?,
        unique,
    )
    .map_err(reason)?;
    loom_core::doc_create_index(&mut loom, ns, &collection, index).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)
}

#[napi]
pub fn doc_index_create_json(
    loom_path: String,
    workspace: String,
    collection: String,
    declaration_json: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let value = serde_json::from_slice::<serde_json::Value>(&declaration_json)
        .map_err(|err| reason(loom_core::LoomError::invalid(err.to_string())))?;
    let declaration = loom_core::document_index_declaration_from_json(&value).map_err(reason)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom_core::doc_create_index_declaration(&mut loom, ns, &collection, declaration)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)
}

#[napi]
pub fn doc_index_drop(
    loom_path: String,
    workspace: String,
    collection: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let dropped = loom_core::doc_drop_index(&mut loom, ns, &collection, &name).map_err(reason)?;
    if dropped {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(dropped)
}

#[napi]
pub fn doc_index_rebuild(
    loom_path: String,
    workspace: String,
    collection: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom_core::doc_rebuild_index(&mut loom, ns, &collection, &name).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)
}

#[napi]
pub fn doc_index_list_json(
    loom_path: String,
    workspace: String,
    collection: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    serde_json::to_string(&loom_core::document_index_declarations_json(
        loom_core::doc_list_index_declarations(&loom, ns, &collection).map_err(reason)?,
    ))
    .map_err(|err| napi::Error::from_reason(err.to_string()))
}

#[napi]
pub fn doc_index_status_json(
    loom_path: String,
    workspace: String,
    collection: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    serde_json::to_string(&loom_core::document_index_statuses_json(
        loom_core::doc_index_statuses(&loom, ns, &collection).map_err(reason)?,
    ))
    .map_err(|err| napi::Error::from_reason(err.to_string()))
}

#[napi]
pub fn doc_find_json(
    loom_path: String,
    workspace: String,
    collection: String,
    index: String,
    value_json: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let value = serde_json::from_str::<serde_json::Value>(&value_json)
        .map_err(|err| napi::Error::from_reason(err.to_string()))?;
    let value = loom_core::document_index_value_from_json(&value).map_err(reason)?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let ids = loom_core::doc_find(&loom, ns, &collection, &index, &value).map_err(reason)?;
    serde_json::to_string(&loom_core::document_ids_json(ids))
        .map_err(|err| napi::Error::from_reason(err.to_string()))
}

#[napi]
pub fn doc_query_json(
    loom_path: String,
    workspace: String,
    collection: String,
    query_json: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let query = serde_json::from_str::<serde_json::Value>(&query_json)
        .map_err(|err| napi::Error::from_reason(err.to_string()))?;
    let query = loom_core::document_query_from_json(&query).map_err(reason)?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let result = loom_core::doc_query(&loom, ns, &collection, &query).map_err(reason)?;
    serde_json::to_string(&loom_core::document_query_result_json(result))
        .map_err(|err| napi::Error::from_reason(err.to_string()))
}
