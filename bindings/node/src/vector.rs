//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::EmbeddingModel;
use loom_core::tabular::{cell_from, cell_value};
use loom_core::vector::{MetaFilter, Metric};

// An embedding crosses as raw little-endian `f32` bytes (4 bytes per component); metadata crosses as a
// Loom Canonical CBOR map of `text -> cell`; a get crosses as `[vector_bytes, metadata]`; a search
// result crosses as a CBOR array of `[id, score_cell]`, highest score first. The metric tags match the
// engine: 1 cosine, 2 negative-squared-L2, 3 dot.

fn metric_from_int(metric: i32) -> napi::Result<Metric> {
    match metric {
        1 => Ok(Metric::Cosine),
        2 => Ok(Metric::L2),
        3 => Ok(Metric::Dot),
        other => Err(napi::Error::from_reason(format!(
            "unknown vector metric {other}"
        ))),
    }
}

fn floats_from_bytes(bytes: &[u8]) -> napi::Result<Vec<f32>> {
    if !bytes.len().is_multiple_of(4) {
        return Err(napi::Error::from_reason(
            "vector bytes length must be a multiple of 4 (little-endian f32)",
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

fn floats_to_bytes(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for v in vector {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn metadata_to_value(metadata: &BTreeMap<String, Value>) -> CborValue {
    let pairs = metadata
        .iter()
        .map(|(k, v)| (CborValue::Text(k.clone()), cell_value(v)))
        .collect();
    CborValue::Map(pairs)
}

fn metadata_from_cbor(bytes: &[u8]) -> napi::Result<BTreeMap<String, Value>> {
    if bytes.is_empty() {
        return Ok(BTreeMap::new());
    }
    let value =
        loom_codec::decode(bytes).map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))?;
    let CborValue::Map(pairs) = value else {
        return Err(napi::Error::from_reason(
            "vector metadata must be a CBOR map",
        ));
    };
    let mut out = BTreeMap::new();
    for (k, v) in pairs {
        let CborValue::Text(key) = k else {
            return Err(napi::Error::from_reason(
                "vector metadata keys must be text",
            ));
        };
        out.insert(key, cell_from(v).map_err(reason)?);
    }
    Ok(out)
}

fn meta_filter_from_value(value: CborValue) -> napi::Result<MetaFilter> {
    let CborValue::Array(items) = value else {
        return Err(napi::Error::from_reason(
            "vector filter must be a CBOR array",
        ));
    };
    let mut it = items.into_iter();
    let tag = match it.next() {
        Some(CborValue::Uint(t)) => t,
        _ => return Err(napi::Error::from_reason("vector filter tag must be a uint")),
    };
    match tag {
        0 => Ok(MetaFilter::All),
        1 => {
            let key = match it.next() {
                Some(CborValue::Text(k)) => k,
                _ => {
                    return Err(napi::Error::from_reason(
                        "vector filter Eq key must be text",
                    ));
                }
            };
            let cell = it
                .next()
                .ok_or_else(|| napi::Error::from_reason("vector filter Eq is missing its value"))?;
            Ok(MetaFilter::Eq(key, cell_from(cell).map_err(reason)?))
        }
        2 => {
            let a = it.next().ok_or_else(|| {
                napi::Error::from_reason("vector filter And is missing its left operand")
            })?;
            let b = it.next().ok_or_else(|| {
                napi::Error::from_reason("vector filter And is missing its right operand")
            })?;
            Ok(MetaFilter::And(
                Box::new(meta_filter_from_value(a)?),
                Box::new(meta_filter_from_value(b)?),
            ))
        }
        other => Err(napi::Error::from_reason(format!(
            "unknown vector filter tag {other}"
        ))),
    }
}

fn meta_filter_from_cbor(bytes: &[u8]) -> napi::Result<MetaFilter> {
    if bytes.is_empty() {
        return Ok(MetaFilter::All);
    }
    let value =
        loom_codec::decode(bytes).map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))?;
    meta_filter_from_value(value)
}

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

/// Create vector set `name` of width `dim` and `metric` (1 cosine, 2 L2, 3 dot) in workspace `workspace`
/// (UUID or name, created with the `vector` facet if absent). `CONFLICT` if the set already exists.
#[napi]
pub fn vector_create(
    loom_path: String,
    workspace: String,
    name: String,
    dim: BigInt,
    metric: i32,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let dim = bigint_to_usize(dim, "dim")?;
    let metric = metric_from_int(metric)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_vector_ns(&mut loom, &workspace)?;
    loom_core::vector_create(&mut loom, ns, &name, dim, metric).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Insert or replace the vector at `id` in set `name`: `vector` is bytes of little-endian `f32` (4 per
/// component); `metadata` is a CBOR `text -> cell` map (or empty). `NOT_FOUND` if the set was never
/// created; `DIMENSION_MISMATCH` on a wrong width.
#[napi]
pub fn vector_upsert(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    vector: Uint8Array,
    metadata: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let vector = floats_from_bytes(&vector)?;
    let metadata = metadata_from_cbor(&metadata)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom_core::vector_upsert(&mut loom, ns, &name, &id, vector, metadata).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}

/// Insert or replace a vector with UTF-8 source text and optional embedding model profile. The profile
/// crosses as `[1, model_id, dimension, weights_digest]`.
#[napi]
pub fn vector_upsert_source(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    vector: Uint8Array,
    metadata: Uint8Array,
    source_text: Uint8Array,
    model_id: Option<String>,
    weights_digest: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let vector = floats_from_bytes(&vector)?;
    let metadata = metadata_from_cbor(&metadata)?;
    let source_text = std::str::from_utf8(&source_text)
        .map_err(|e| napi::Error::from_reason(format!("sourceText must be UTF-8: {e}")))?;
    let embedding_model =
        model_id.map(|model_id| EmbeddingModel::new(model_id, vector.len(), weights_digest));
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom_core::vector_upsert_with_source(
        &mut loom,
        ns,
        &name,
        &id,
        vector,
        metadata,
        source_text,
        embedding_model,
    )
    .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}

/// Fetch the vector + metadata at `id` in set `name` as the Loom Canonical CBOR array `[vector_bytes,
/// metadata]` (`vector_bytes` little-endian `f32`; `metadata` a `text -> cell` map), or `null` if absent.
/// `NOT_FOUND` if the set does not exist.
#[napi]
pub fn vector_get(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::vector_get(&loom, ns, &name, &id)
        .map_err(reason)?
        .map(|(vector, metadata)| {
            let value = CborValue::Array(vec![
                CborValue::Bytes(floats_to_bytes(&vector)),
                metadata_to_value(&metadata),
            ]);
            Uint8Array::from(cbor_encode(&value).unwrap_or_default())
        }))
}

/// Fetch UTF-8 source text for vector `id`, or `null` if no source text is stored.
#[napi]
pub fn vector_source_text(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::vector_source_text(&loom, ns, &name, &id)
        .map_err(reason)?
        .map(|s| Uint8Array::from(s.into_bytes())))
}

/// Fetch the set embedding model profile as CBOR `[1, model_id, dimension, weights_digest]`, or `null`.
#[napi]
pub fn vector_embedding_model(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::vector_embedding_model(&loom, ns, &name)
        .map_err(reason)?
        .map(|m| Uint8Array::from(embedding_model_cbor(&m))))
}

/// Vector ids in set `name`, sorted ascending, as the Loom Canonical CBOR array of text. When
/// `prefix` is present, only ids starting with that string prefix are returned. `NOT_FOUND` if the set
/// does not exist.
#[napi]
pub fn vector_ids(
    loom_path: String,
    workspace: String,
    name: String,
    prefix: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(strings_cbor(
        loom_core::vector_ids(&loom, ns, &name, prefix.as_deref()).map_err(reason)?,
    )))
}
/// Metadata equality index keys declared for set `name`, sorted ascending, as the Loom Canonical CBOR
/// array of text. `NOT_FOUND` if the set does not exist.
#[napi]
pub fn vector_metadata_index_keys(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(strings_cbor(
        loom_core::vector_metadata_index_keys(&loom, ns, &name).map_err(reason)?,
    )))
}
/// Declare and build a metadata equality index for `key`; returns whether a new index was declared.
#[napi]
pub fn vector_create_metadata_index(
    loom_path: String,
    workspace: String,
    name: String,
    key: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let changed =
        loom_core::vector_create_metadata_index(&mut loom, ns, &name, &key).map_err(reason)?;
    if changed {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(changed)
}
/// Drop a metadata equality index for `key`; returns whether an index was present.
#[napi]
pub fn vector_drop_metadata_index(
    loom_path: String,
    workspace: String,
    name: String,
    key: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let changed =
        loom_core::vector_drop_metadata_index(&mut loom, ns, &name, &key).map_err(reason)?;
    if changed {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(changed)
}
/// Remove `id` from set `name`; returns whether it was present. `NOT_FOUND` if the set does not exist.
#[napi]
pub fn vector_delete(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let present = loom_core::vector_delete(&mut loom, ns, &name, &id).map_err(reason)?;
    if present {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(present)
}
/// Exact top-`k` nearest neighbours of `query` (bytes of little-endian `f32`) in set `name` among
/// vectors passing `filter`, as the Loom Canonical CBOR array of `[id, score_cell]`, highest score first.
/// The filter is a recursive CBOR array: `[0]` all, `[1, key, value_cell]` equality, `[2, a, b]` AND; an
/// empty buffer is all. `NOT_FOUND` if the set does not exist; `DIMENSION_MISMATCH` on a wrong-width query.
#[napi]
pub fn vector_search(
    loom_path: String,
    workspace: String,
    name: String,
    query: Uint8Array,
    k: BigInt,
    filter: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let query = floats_from_bytes(&query)?;
    let k = bigint_to_usize(k, "k")?;
    let filter = meta_filter_from_cbor(&filter)?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(hits_cbor(
        loom_core::vector_search(&loom, ns, &name, &query, k, &filter).map_err(reason)?,
    )))
}

/// Top-`k` nearest neighbours with an explicit accelerator policy over the built-in PQ accelerator.
/// `policy` is 0 for exact and 1 for approximate-above-threshold. Results have the same CBOR shape
/// as `vectorSearch`.
#[napi]
pub fn vector_search_policy(
    loom_path: String,
    workspace: String,
    name: String,
    query: Uint8Array,
    k: BigInt,
    filter: Uint8Array,
    policy: i32,
    threshold: BigInt,
    ef: BigInt,
    pq_m: BigInt,
    pq_k: BigInt,
    pq_iters: BigInt,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let query = floats_from_bytes(&query)?;
    let k = bigint_to_usize(k, "k")?;
    let filter = meta_filter_from_cbor(&filter)?;
    let threshold = bigint_to_usize(threshold, "threshold")?;
    let ef = bigint_to_usize(ef, "ef")?;
    let pq_m = bigint_to_usize(pq_m, "pq_m")?;
    let pq_k = bigint_to_usize(pq_k, "pq_k")?;
    let pq_iters = bigint_to_usize(pq_iters, "pq_iters")?;
    let policy = match policy {
        0 => loom_core::AcceleratorPolicy::ExactAlways,
        1 => loom_core::AcceleratorPolicy::ApproximateAbove { threshold },
        other => {
            return Err(napi::Error::from_reason(format!(
                "unknown vector accelerator policy {other}"
            )));
        }
    };
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(hits_cbor(
        loom_core::vector_search_with_pq_policy(
            &loom, ns, &name, &query, k, &filter, policy, ef, pq_m, pq_k, pq_iters,
        )
        .map_err(reason)?,
    )))
}
