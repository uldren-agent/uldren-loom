//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::tabular::{cell_from, cell_value};
use loom_core::vector::{MetaFilter, Metric};
use loom_core::{
    EmbeddingModel, vector_create, vector_create_metadata_index, vector_delete,
    vector_drop_metadata_index, vector_embedding_model, vector_get, vector_ids,
    vector_metadata_index_keys, vector_search, vector_source_text, vector_upsert,
    vector_upsert_with_source,
};

// Vector set (Vector facet) - dense embeddings + metadata in a named set within a workspace, with
// exact top-k search. An embedding crosses as raw little-endian `f32` bytes (4 bytes per component);
// metadata crosses as a Loom Canonical CBOR map of `text -> cell`; a get crosses as
// `[vector_bytes, metadata]`; a search result crosses as a CBOR array of `[id, score_cell]`, highest
// score first. The metric tags match the engine: 1 cosine, 2 negative-squared-L2, 3 dot.

/// Resolve a workspace for a vector write by UUID or name, ensuring the `vector` facet exists.
fn ensure_vector_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Vector,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Vector)
        .map_err(le)?;
    Ok(ns)
}

/// Map a metric tag (1 cosine, 2 L2, 3 dot) to a [`Metric`].
fn metric_from_int(metric: i32) -> Result<Metric, JsError> {
    match metric {
        1 => Ok(Metric::Cosine),
        2 => Ok(Metric::L2),
        3 => Ok(Metric::Dot),
        other => Err(JsError::new(&format!("unknown vector metric {other}"))),
    }
}

/// Decode raw little-endian `f32` bytes (4 per component) into a vector.
fn floats_from_bytes(bytes: &[u8]) -> Result<Vec<f32>, JsError> {
    if !bytes.len().is_multiple_of(4) {
        return Err(JsError::new(
            "vector bytes length must be a multiple of 4 (little-endian f32)",
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

/// Encode a vector as raw little-endian `f32` bytes (4 per component).
fn floats_to_bytes(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for v in vector {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Encode a metadata map as the canonical-CBOR `text -> cell` map wire form.
fn metadata_to_value(metadata: &BTreeMap<String, Value>) -> CborValue {
    let pairs = metadata
        .iter()
        .map(|(k, v)| (CborValue::Text(k.clone()), cell_value(v)))
        .collect();
    CborValue::Map(pairs)
}

/// Decode a canonical-CBOR `text -> cell` map (empty buffer is the empty map) into a metadata map.
fn metadata_from_cbor(bytes: &[u8]) -> Result<BTreeMap<String, Value>, JsError> {
    if bytes.is_empty() {
        return Ok(BTreeMap::new());
    }
    let value = loom_codec::decode(bytes).map_err(|e| JsError::new(&format!("cbor: {e}")))?;
    let CborValue::Map(pairs) = value else {
        return Err(JsError::new("vector metadata must be a CBOR map"));
    };
    let mut out = BTreeMap::new();
    for (k, v) in pairs {
        let CborValue::Text(key) = k else {
            return Err(JsError::new("vector metadata keys must be text"));
        };
        out.insert(key, cell_from(v).map_err(le)?);
    }
    Ok(out)
}

/// Decode a recursive CBOR filter value: `[0]` all, `[1, key, value_cell]` equality, `[2, a, b]` AND.
fn meta_filter_from_value(value: CborValue) -> Result<MetaFilter, JsError> {
    let CborValue::Array(items) = value else {
        return Err(JsError::new("vector filter must be a CBOR array"));
    };
    let mut it = items.into_iter();
    let tag = match it.next() {
        Some(CborValue::Uint(t)) => t,
        _ => return Err(JsError::new("vector filter tag must be a uint")),
    };
    match tag {
        0 => Ok(MetaFilter::All),
        1 => {
            let key = match it.next() {
                Some(CborValue::Text(k)) => k,
                _ => return Err(JsError::new("vector filter Eq key must be text")),
            };
            let cell = it
                .next()
                .ok_or_else(|| JsError::new("vector filter Eq is missing its value"))?;
            Ok(MetaFilter::Eq(key, cell_from(cell).map_err(le)?))
        }
        2 => {
            let a = it
                .next()
                .ok_or_else(|| JsError::new("vector filter And is missing its left operand"))?;
            let b = it
                .next()
                .ok_or_else(|| JsError::new("vector filter And is missing its right operand"))?;
            Ok(MetaFilter::And(
                Box::new(meta_filter_from_value(a)?),
                Box::new(meta_filter_from_value(b)?),
            ))
        }
        other => Err(JsError::new(&format!("unknown vector filter tag {other}"))),
    }
}

/// Decode the search filter buffer (empty buffer is `All`).
fn meta_filter_from_cbor(bytes: &[u8]) -> Result<MetaFilter, JsError> {
    if bytes.is_empty() {
        return Ok(MetaFilter::All);
    }
    let value = loom_codec::decode(bytes).map_err(|e| JsError::new(&format!("cbor: {e}")))?;
    meta_filter_from_value(value)
}

/// Encode search hits as a canonical-CBOR array of `[id, score_cell]`, highest score first.
fn hits_cbor(hits: Vec<loom_core::Hit>) -> Vec<u8> {
    let items = hits
        .into_iter()
        .map(|h| {
            CborValue::Array(vec![
                CborValue::Text(h.id),
                cell_value(&Value::F32(h.score)),
            ])
        })
        .collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

/// Encode vector ids as a canonical-CBOR array of text.
fn strings_cbor(ids: Vec<String>) -> Vec<u8> {
    let items = ids.into_iter().map(CborValue::Text).collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

fn embedding_model_cbor(model: &EmbeddingModel) -> Vec<u8> {
    cbor_encode(&CborValue::Array(vec![
        CborValue::Uint(1),
        CborValue::Text(model.model_id.clone()),
        CborValue::Uint(model.dimension as u64),
        CborValue::Text(model.weights_digest.clone().unwrap_or_default()),
    ]))
    .unwrap_or_default()
}

#[wasm_bindgen]
impl LoomSql {
    /// Create vector set `name` of width `dim` and `metric` (1 cosine, 2 L2, 3 dot) in `workspace`
    /// (UUID or name, created with the `vector` facet if absent).
    pub fn vector_create(
        &mut self,
        workspace: String,
        name: String,
        dim: usize,
        metric: i32,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let metric = metric_from_int(metric)?;
        let ns = ensure_vector_ns(&mut self.loom, &workspace)?;
        vector_create(&mut self.loom, ns, &name, dim, metric).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Insert or replace the vector at `id` in set `name`: `vector` is little-endian `f32` bytes (4
    /// per component); `metadata` is a CBOR `text -> cell` map (or empty).
    pub fn vector_upsert(
        &mut self,
        workspace: String,
        name: String,
        id: String,
        vector: Vec<u8>,
        metadata: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let vector = floats_from_bytes(&vector)?;
        let metadata = metadata_from_cbor(&metadata)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        vector_upsert(&mut self.loom, ns, &name, &id, vector, metadata).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Insert or replace a vector with UTF-8 source text and optional embedding model profile. The
    /// profile crosses as `[1, model_id, dimension, weights_digest]`.
    pub fn vector_upsert_source(
        &mut self,
        workspace: String,
        name: String,
        id: String,
        vector: Vec<u8>,
        metadata: Vec<u8>,
        source_text: Vec<u8>,
        model_id: Option<String>,
        weights_digest: Option<String>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let vector = floats_from_bytes(&vector)?;
        let metadata = metadata_from_cbor(&metadata)?;
        let source_text = std::str::from_utf8(&source_text)
            .map_err(|e| JsError::new(&format!("sourceText must be UTF-8: {e}")))?;
        let embedding_model =
            model_id.map(|model_id| EmbeddingModel::new(model_id, vector.len(), weights_digest));
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        vector_upsert_with_source(
            &mut self.loom,
            ns,
            &name,
            &id,
            vector,
            metadata,
            source_text,
            embedding_model,
        )
        .map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Fetch the vector + metadata at `id` in set `name` as the CBOR array `[vector_bytes, metadata]`
    /// (`vector_bytes` little-endian `f32`; `metadata` a `text -> cell` map), or null if absent.
    pub fn vector_get(
        &self,
        workspace: String,
        name: String,
        id: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(vector_get(&self.loom, ns, &name, &id)
            .map_err(le)?
            .map(|(vector, metadata)| {
                let value = CborValue::Array(vec![
                    CborValue::Bytes(floats_to_bytes(&vector)),
                    metadata_to_value(&metadata),
                ]);
                Uint8Array::from(cbor_encode(&value).unwrap_or_default().as_slice())
            }))
    }

    /// Fetch UTF-8 source text for vector `id`, or null if no source text is stored.
    pub fn vector_source_text(
        &self,
        workspace: String,
        name: String,
        id: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(vector_source_text(&self.loom, ns, &name, &id)
            .map_err(le)?
            .map(|s| Uint8Array::from(s.into_bytes().as_slice())))
    }

    /// Fetch the set embedding model profile as CBOR `[1, model_id, dimension, weights_digest]`.
    pub fn vector_embedding_model(
        &self,
        workspace: String,
        name: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(vector_embedding_model(&self.loom, ns, &name)
            .map_err(le)?
            .map(|m| Uint8Array::from(embedding_model_cbor(&m).as_slice())))
    }

    /// Vector ids in set `name`, sorted ascending, as a CBOR array of text. `prefix`, when present,
    /// restricts results to ids with that string prefix.
    pub fn vector_ids(
        &self,
        workspace: String,
        name: String,
        prefix: Option<String>,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(strings_cbor(
            vector_ids(&self.loom, ns, &name, prefix.as_deref()).map_err(le)?,
        ))
    }

    /// Metadata equality index keys declared for set `name`, sorted ascending, as a CBOR array of text.
    pub fn vector_metadata_index_keys(
        &self,
        workspace: String,
        name: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(strings_cbor(
            vector_metadata_index_keys(&self.loom, ns, &name).map_err(le)?,
        ))
    }

    /// Declare and build a metadata equality index for `key`; returns whether a new index was
    /// declared.
    pub fn vector_create_metadata_index(
        &mut self,
        workspace: String,
        name: String,
        key: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let changed = vector_create_metadata_index(&mut self.loom, ns, &name, &key).map_err(le)?;
        if changed {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(changed)
    }

    /// Drop the metadata equality index for `key`; returns whether an index was present.
    pub fn vector_drop_metadata_index(
        &mut self,
        workspace: String,
        name: String,
        key: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let changed = vector_drop_metadata_index(&mut self.loom, ns, &name, &key).map_err(le)?;
        if changed {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(changed)
    }

    /// Remove `id` from set `name`; returns whether it was present.
    pub fn vector_delete(
        &mut self,
        workspace: String,
        name: String,
        id: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let present = vector_delete(&mut self.loom, ns, &name, &id).map_err(le)?;
        if present {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(present)
    }

    /// Exact top-`k` nearest neighbours of `query` (little-endian `f32` bytes) in set `name` among
    /// vectors passing `filter`, as a CBOR array of `[id, score_cell]`, highest score first. The
    /// filter is a recursive CBOR array: `[0]` all, `[1, key, value_cell]` equality, `[2, a, b]` AND;
    /// an empty buffer is all.
    pub fn vector_search(
        &self,
        workspace: String,
        name: String,
        query: Vec<u8>,
        k: usize,
        filter: Vec<u8>,
    ) -> Result<Vec<u8>, JsError> {
        let query = floats_from_bytes(&query)?;
        let filter = meta_filter_from_cbor(&filter)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(hits_cbor(
            vector_search(&self.loom, ns, &name, &query, k, &filter).map_err(le)?,
        ))
    }

    /// Top-`k` nearest neighbours with explicit accelerator policy over built-in PQ. `policy` is 0
    /// for exact and 1 for approximate-above-threshold. Result CBOR matches `vector_search`.
    pub fn vector_search_policy(
        &self,
        workspace: String,
        name: String,
        query: Vec<u8>,
        k: usize,
        filter: Vec<u8>,
        policy: i32,
        threshold: usize,
        ef: usize,
        pq_m: usize,
        pq_k: usize,
        pq_iters: usize,
    ) -> Result<Vec<u8>, JsError> {
        let query = floats_from_bytes(&query)?;
        let filter = meta_filter_from_cbor(&filter)?;
        let policy = match policy {
            0 => loom_core::AcceleratorPolicy::ExactAlways,
            1 => loom_core::AcceleratorPolicy::ApproximateAbove { threshold },
            other => {
                return Err(JsError::new(&format!(
                    "unknown vector accelerator policy {other}"
                )));
            }
        };
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(hits_cbor(
            loom_core::vector_search_with_pq_policy(
                &self.loom, ns, &name, &query, k, &filter, policy, ef, pq_m, pq_k, pq_iters,
            )
            .map_err(le)?,
        ))
    }
}
