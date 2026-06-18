//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

fn document_put_result_js(digest: &str, entity_tag: &str) -> Result<JsValue, JsError> {
    let object = Object::new();
    Reflect::set(
        &object,
        &JsValue::from_str("digest"),
        &JsValue::from_str(digest),
    )
    .map_err(je)?;
    Reflect::set(
        &object,
        &JsValue::from_str("entity_tag"),
        &JsValue::from_str(entity_tag),
    )
    .map_err(je)?;
    Ok(object.into())
}

#[wasm_bindgen]
impl LoomSql {
    /// Put UTF-8 text at string `id` in collection `collection` and return the new document tags.
    pub fn doc_put_text(
        &mut self,
        workspace: String,
        collection: String,
        id: String,
        text: String,
        expected_entity_tag: Option<String>,
    ) -> Result<JsValue, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_doc_ns(&mut self.loom, &workspace)?;
        let result = document_put_text_with_entity_tag(
            &mut self.loom,
            ns,
            &collection,
            &id,
            &text,
            expected_entity_tag.as_deref(),
        )
        .map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        document_put_result_js(&result.digest.to_string(), &result.entity_tag)
    }

    /// Fetch `id` as UTF-8 text with its content digest, or null if absent.
    pub fn doc_get_text(
        &self,
        workspace: String,
        collection: String,
        id: String,
    ) -> Result<JsValue, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let Some(document) = document_get_text(&self.loom, ns, &collection, &id).map_err(le)?
        else {
            return Ok(JsValue::NULL);
        };
        let object = Object::new();
        Reflect::set(
            &object,
            &JsValue::from_str("text"),
            &JsValue::from_str(&document.text),
        )
        .map_err(je)?;
        Reflect::set(
            &object,
            &JsValue::from_str("digest"),
            &JsValue::from_str(&document.digest.to_string()),
        )
        .map_err(je)?;
        Reflect::set(
            &object,
            &JsValue::from_str("entity_tag"),
            &JsValue::from_str(&document.entity_tag),
        )
        .map_err(je)?;
        Ok(object.into())
    }

    /// Put binary bytes at string `id` in collection `collection` and return the new document tags.
    pub fn doc_put_binary(
        &mut self,
        workspace: String,
        collection: String,
        id: String,
        bytes: Vec<u8>,
        expected_entity_tag: Option<String>,
    ) -> Result<JsValue, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = ensure_doc_ns(&mut self.loom, &workspace)?;
        let result = document_put_binary_with_entity_tag(
            &mut self.loom,
            ns,
            &collection,
            &id,
            bytes,
            expected_entity_tag.as_deref(),
        )
        .map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        document_put_result_js(&result.digest.to_string(), &result.entity_tag)
    }

    /// Fetch `id` as binary bytes with its content digest, or null if absent.
    pub fn doc_get_binary(
        &self,
        workspace: String,
        collection: String,
        id: String,
    ) -> Result<JsValue, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let Some(document) = document_get_binary(&self.loom, ns, &collection, &id).map_err(le)?
        else {
            return Ok(JsValue::NULL);
        };
        let object = Object::new();
        Reflect::set(
            &object,
            &JsValue::from_str("bytes"),
            &Uint8Array::from(document.bytes.as_slice()).into(),
        )
        .map_err(je)?;
        Reflect::set(
            &object,
            &JsValue::from_str("digest"),
            &JsValue::from_str(&document.digest.to_string()),
        )
        .map_err(je)?;
        Reflect::set(
            &object,
            &JsValue::from_str("entity_tag"),
            &JsValue::from_str(&document.entity_tag),
        )
        .map_err(je)?;
        Ok(object.into())
    }

    /// Remove `id` from collection `collection`; returns whether it was present.
    pub fn doc_delete(
        &mut self,
        workspace: String,
        collection: String,
        id: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let present = doc_delete(&mut self.loom, ns, &collection, &id).map_err(le)?;
        if present {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(present)
    }

    /// List collection `collection` as its canonical binary representation.
    pub fn doc_list_binary(
        &self,
        workspace: String,
        collection: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        document_list_binary(&self.loom, ns, &collection).map_err(le)
    }

    pub fn doc_index_create(
        &mut self,
        workspace: String,
        collection: String,
        name: String,
        field_path: String,
        unique: bool,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let index = DocumentIndexDef::new(
            name,
            DocumentFieldPath::dotted(&field_path).map_err(le)?,
            unique,
        )
        .map_err(le)?;
        doc_create_index(&mut self.loom, ns, &collection, index).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    pub fn doc_index_create_json(
        &mut self,
        workspace: String,
        collection: String,
        declaration_json: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let value = serde_json::from_slice::<serde_json::Value>(&declaration_json)
            .map_err(|err| JsError::new(&err.to_string()))?;
        let declaration = loom_core::document_index_declaration_from_json(&value).map_err(le)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        loom_core::doc_create_index_declaration(&mut self.loom, ns, &collection, declaration)
            .map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    pub fn doc_index_drop(
        &mut self,
        workspace: String,
        collection: String,
        name: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let dropped = doc_drop_index(&mut self.loom, ns, &collection, &name).map_err(le)?;
        if dropped {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(dropped)
    }

    pub fn doc_index_rebuild(
        &mut self,
        workspace: String,
        collection: String,
        name: String,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        doc_rebuild_index(&mut self.loom, ns, &collection, &name).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    pub fn doc_index_list_json(
        &self,
        workspace: String,
        collection: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let value = loom_core::document_index_declarations_json(
            loom_core::doc_list_index_declarations(&self.loom, ns, &collection).map_err(le)?,
        );
        serde_json::to_string(&value).map_err(|err| JsError::new(&err.to_string()))
    }

    pub fn doc_index_status_json(
        &self,
        workspace: String,
        collection: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let value = document_index_statuses_json(
            doc_index_statuses(&self.loom, ns, &collection).map_err(le)?,
        );
        serde_json::to_string(&value).map_err(|err| JsError::new(&err.to_string()))
    }

    pub fn doc_find_json(
        &self,
        workspace: String,
        collection: String,
        index: String,
        value_json: String,
    ) -> Result<String, JsError> {
        let value = serde_json::from_str::<serde_json::Value>(&value_json)
            .map_err(|err| JsError::new(&err.to_string()))?;
        let value = document_index_value_from_json(&value).map_err(le)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let ids = doc_find(&self.loom, ns, &collection, &index, &value).map_err(le)?;
        serde_json::to_string(&document_ids_json(ids)).map_err(|err| JsError::new(&err.to_string()))
    }

    pub fn doc_query_json(
        &self,
        workspace: String,
        collection: String,
        query_json: String,
    ) -> Result<String, JsError> {
        let query = serde_json::from_str::<serde_json::Value>(&query_json)
            .map_err(|err| JsError::new(&err.to_string()))?;
        let query = document_query_from_json(&query).map_err(le)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let result = doc_query(&self.loom, ns, &collection, &query).map_err(le)?;
        serde_json::to_string(&document_query_result_json(result))
            .map_err(|err| JsError::new(&err.to_string()))
    }
}
