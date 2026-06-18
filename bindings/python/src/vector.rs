//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::EmbeddingModel;
use loom_core::tabular::{cell_from, cell_value};
use loom_core::vector::{MetaFilter, Metric};

/// Map a metric tag to the engine metric: 1 cosine, 2 negative-squared-L2, 3 dot.
fn metric_from_int(metric: i32) -> PyResult<Metric> {
    match metric {
        1 => Ok(Metric::Cosine),
        2 => Ok(Metric::L2),
        3 => Ok(Metric::Dot),
        other => Err(PyRuntimeError::new_err(format!(
            "unknown vector metric {other}"
        ))),
    }
}

/// Decode raw little-endian `f32` bytes (4 per component) into a vector.
fn floats_from_bytes(bytes: &[u8]) -> PyResult<Vec<f32>> {
    if !bytes.len().is_multiple_of(4) {
        return Err(PyRuntimeError::new_err(
            "vector bytes length must be a multiple of 4 (little-endian f32)",
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

/// Encode a vector as raw little-endian `f32` bytes.
fn floats_to_bytes(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for v in vector {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Encode metadata as a Loom Canonical CBOR map of `text -> cell`.
fn metadata_to_value(metadata: &BTreeMap<String, Value>) -> CborValue {
    let pairs = metadata
        .iter()
        .map(|(k, v)| (CborValue::Text(k.clone()), cell_value(v)))
        .collect();
    CborValue::Map(pairs)
}

/// Decode a Loom Canonical CBOR map of `text -> cell` into metadata; an empty buffer is no metadata.
fn metadata_from_cbor(bytes: &[u8]) -> PyResult<BTreeMap<String, Value>> {
    if bytes.is_empty() {
        return Ok(BTreeMap::new());
    }
    let value =
        loom_codec::decode(bytes).map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))?;
    let CborValue::Map(pairs) = value else {
        return Err(PyRuntimeError::new_err(
            "vector metadata must be a CBOR map",
        ));
    };
    let mut out = BTreeMap::new();
    for (k, v) in pairs {
        let CborValue::Text(key) = k else {
            return Err(PyRuntimeError::new_err("vector metadata keys must be text"));
        };
        out.insert(key, cell_from(v).map_err(py_err)?);
    }
    Ok(out)
}

/// Decode a recursive CBOR filter value into a [`MetaFilter`]: `[0]` all, `[1, key, value_cell]`
/// equality, `[2, a, b]` AND.
fn meta_filter_from_value(value: CborValue) -> PyResult<MetaFilter> {
    let CborValue::Array(items) = value else {
        return Err(PyRuntimeError::new_err(
            "vector filter must be a CBOR array",
        ));
    };
    let mut it = items.into_iter();
    let tag = match it.next() {
        Some(CborValue::Uint(t)) => t,
        _ => return Err(PyRuntimeError::new_err("vector filter tag must be a uint")),
    };
    match tag {
        0 => Ok(MetaFilter::All),
        1 => {
            let key = match it.next() {
                Some(CborValue::Text(k)) => k,
                _ => return Err(PyRuntimeError::new_err("vector filter Eq key must be text")),
            };
            let cell = it
                .next()
                .ok_or_else(|| PyRuntimeError::new_err("vector filter Eq is missing its value"))?;
            Ok(MetaFilter::Eq(key, cell_from(cell).map_err(py_err)?))
        }
        2 => {
            let a = it.next().ok_or_else(|| {
                PyRuntimeError::new_err("vector filter And is missing its left operand")
            })?;
            let b = it.next().ok_or_else(|| {
                PyRuntimeError::new_err("vector filter And is missing its right operand")
            })?;
            Ok(MetaFilter::And(
                Box::new(meta_filter_from_value(a)?),
                Box::new(meta_filter_from_value(b)?),
            ))
        }
        other => Err(PyRuntimeError::new_err(format!(
            "unknown vector filter tag {other}"
        ))),
    }
}

/// Decode a CBOR filter buffer into a [`MetaFilter`]; an empty buffer is all.
fn meta_filter_from_cbor(bytes: &[u8]) -> PyResult<MetaFilter> {
    if bytes.is_empty() {
        return Ok(MetaFilter::All);
    }
    let value =
        loom_codec::decode(bytes).map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))?;
    meta_filter_from_value(value)
}

/// Encode search hits as a CBOR array of `[id, score_cell]`, highest score first.
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

/// Encode vector ids as a CBOR array of text.
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

/// Create vector set `name` of width `dim` and `metric` (1 cosine, 2 L2, 3 dot) in `workspace` (UUID or
/// name, created with the `vector` facet if absent). `CONFLICT` if the set already exists.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, dim, metric, passphrase=None))]
pub(crate) fn vector_create(
    path: &str,
    workspace: &str,
    name: &str,
    dim: usize,
    metric: i32,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let metric = metric_from_int(metric)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_vector_ns(&mut loom, workspace)?;
    loom_core::vector_create(&mut loom, ns, name, dim, metric).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Insert or replace the vector at `id` in set `name`: `vector` is little-endian `f32` bytes (4 per
/// component); `metadata` is a CBOR `text -> cell` map (or empty). `NOT_FOUND` if the set was never
/// created; `DIMENSION_MISMATCH` on a wrong width.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, vector, metadata, passphrase=None))]
pub(crate) fn vector_upsert(
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    vector: &[u8],
    metadata: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let vector = floats_from_bytes(vector)?;
    let metadata = metadata_from_cbor(metadata)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_core::vector_upsert(&mut loom, ns, name, id, vector, metadata).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}

/// Insert or replace a vector with UTF-8 source text and optional embedding model profile. The profile
/// crosses as `[1, model_id, dimension, weights_digest]`.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, vector, metadata, source_text, model_id=None, weights_digest=None, passphrase=None))]
pub(crate) fn vector_upsert_source(
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    vector: &[u8],
    metadata: &[u8],
    source_text: &[u8],
    model_id: Option<&str>,
    weights_digest: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let vector = floats_from_bytes(vector)?;
    let metadata = metadata_from_cbor(metadata)?;
    let source_text = std::str::from_utf8(source_text)
        .map_err(|e| PyRuntimeError::new_err(format!("source_text must be UTF-8: {e}")))?;
    let embedding_model = model_id.map(|model_id| {
        EmbeddingModel::new(model_id, vector.len(), weights_digest.map(str::to_string))
    });
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_core::vector_upsert_with_source(
        &mut loom,
        ns,
        name,
        id,
        vector,
        metadata,
        source_text,
        embedding_model,
    )
    .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}

/// Fetch the vector + metadata at `id` in set `name` as the CBOR array `[vector_bytes, metadata]`
/// (`vector_bytes` little-endian `f32`; `metadata` a `text -> cell` map), or `None` when `id` is absent.
/// `NOT_FOUND` if the set does not exist.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, passphrase=None))]
pub(crate) fn vector_get<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::vector_get(&loom, ns, name, id)
        .map_err(py_err)?
        .map(|(vector, metadata)| {
            let value = CborValue::Array(vec![
                CborValue::Bytes(floats_to_bytes(&vector)),
                metadata_to_value(&metadata),
            ]);
            PyBytes::new(py, &cbor_encode(&value).unwrap_or_default())
        }))
}

/// Fetch UTF-8 source text for vector `id`, or `None` if no source text is stored.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, passphrase=None))]
pub(crate) fn vector_source_text<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::vector_source_text(&loom, ns, name, id)
        .map_err(py_err)?
        .map(|s| PyBytes::new(py, &s.into_bytes())))
}

/// Fetch the set embedding model profile as CBOR `[1, model_id, dimension, weights_digest]`, or `None`.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn vector_embedding_model<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::vector_embedding_model(&loom, ns, name)
        .map_err(py_err)?
        .map(|m| PyBytes::new(py, &embedding_model_cbor(&m))))
}

/// Vector ids in set `name`, sorted ascending, as a CBOR array of text. When `prefix` is present,
/// only ids starting with that string prefix are returned. `NOT_FOUND` if the set does not exist.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, prefix=None, passphrase=None))]
pub(crate) fn vector_ids<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    prefix: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = strings_cbor(loom_core::vector_ids(&loom, ns, name, prefix).map_err(py_err)?);
    Ok(PyBytes::new(py, &bytes))
}
/// Metadata equality index keys declared for set `name`, sorted ascending, as a CBOR array of text.
/// `NOT_FOUND` if the set does not exist.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn vector_metadata_index_keys<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes =
        strings_cbor(loom_core::vector_metadata_index_keys(&loom, ns, name).map_err(py_err)?);
    Ok(PyBytes::new(py, &bytes))
}
/// Declare and build a metadata equality index for `key`; returns whether a new index was declared.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, key, passphrase=None))]
pub(crate) fn vector_create_metadata_index(
    path: &str,
    workspace: &str,
    name: &str,
    key: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let changed =
        loom_core::vector_create_metadata_index(&mut loom, ns, name, key).map_err(py_err)?;
    if changed {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(changed)
}
/// Drop a metadata equality index for `key`; returns whether an index was present.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, key, passphrase=None))]
pub(crate) fn vector_drop_metadata_index(
    path: &str,
    workspace: &str,
    name: &str,
    key: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let changed =
        loom_core::vector_drop_metadata_index(&mut loom, ns, name, key).map_err(py_err)?;
    if changed {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(changed)
}
/// Remove `id` from set `name`; returns whether it was present. `NOT_FOUND` if the set does not exist.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, passphrase=None))]
pub(crate) fn vector_delete(
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let present = loom_core::vector_delete(&mut loom, ns, name, id).map_err(py_err)?;
    if present {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(present)
}
/// Exact top-`k` nearest neighbours of `query` (little-endian `f32` bytes) in set `name` among vectors
/// passing `filter`, as a CBOR array of `[id, score_cell]`, highest score first. The filter is a
/// recursive CBOR array: `[0]` all, `[1, key, value_cell]` equality, `[2, a, b]` AND; an empty buffer is
/// all. `NOT_FOUND` if the set does not exist; `DIMENSION_MISMATCH` on a wrong-width query.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, query, k, filter, passphrase=None))]
pub(crate) fn vector_search<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    query: &[u8],
    k: usize,
    filter: &[u8],
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let query = floats_from_bytes(query)?;
    let filter = meta_filter_from_cbor(filter)?;
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes =
        hits_cbor(loom_core::vector_search(&loom, ns, name, &query, k, &filter).map_err(py_err)?);
    Ok(PyBytes::new(py, &bytes))
}

/// Top-`k` nearest neighbours with an explicit accelerator policy over the built-in PQ accelerator.
/// `policy` is 0 for exact and 1 for approximate-above-threshold. Result CBOR matches
/// `vector_search`.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, query, k, filter, policy, threshold, ef, pq_m, pq_k, pq_iters, passphrase=None))]
pub(crate) fn vector_search_policy<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    query: &[u8],
    k: usize,
    filter: &[u8],
    policy: i32,
    threshold: usize,
    ef: usize,
    pq_m: usize,
    pq_k: usize,
    pq_iters: usize,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let query = floats_from_bytes(query)?;
    let filter = meta_filter_from_cbor(filter)?;
    let policy = match policy {
        0 => loom_core::AcceleratorPolicy::ExactAlways,
        1 => loom_core::AcceleratorPolicy::ApproximateAbove { threshold },
        other => {
            return Err(PyRuntimeError::new_err(format!(
                "unknown vector accelerator policy {other}"
            )));
        }
    };
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = hits_cbor(
        loom_core::vector_search_with_pq_policy(
            &loom, ns, name, &query, k, &filter, policy, ef, pq_m, pq_k, pq_iters,
        )
        .map_err(py_err)?,
    );
    Ok(PyBytes::new(py, &bytes))
}
