//! Canonical wire CBOR codecs for the vector facet, shared by the C ABI, the in-process client
//! service impl, and the server dispatch. A vector crosses as little-endian `f32` bytes; metadata as a
//! CBOR `text -> cell` map; a fetched entry as `[vector_bytes, metadata]`; search hits as a CBOR array
//! of `[id, score_cell]`; the metadata filter as a recursive tagged CBOR array; the embedding-model
//! profile as `[1, model_id, dimension, weights_digest]`.

use loom_codec::{Value as CborValue, decode, encode};
use loom_core::tabular::{Value, cell_from, cell_value};
use loom_core::vector::{MetaFilter, Metric};
use loom_core::{AcceleratorPolicy, EmbeddingModel, Hit};
use loom_types::LoomError;
use std::collections::BTreeMap;

/// Decode a distance-metric tag (1 cosine, 2 L2, 3 dot).
pub fn metric_from_int(metric: i32) -> Result<Metric, LoomError> {
    match metric {
        1 => Ok(Metric::Cosine),
        2 => Ok(Metric::L2),
        3 => Ok(Metric::Dot),
        other => Err(LoomError::invalid(format!("unknown vector metric {other}"))),
    }
}

/// Decode an accelerator-policy tag (0 exact-always, 1 approximate-above-threshold).
pub fn accelerator_policy_from_int(
    policy: i32,
    threshold: usize,
) -> Result<AcceleratorPolicy, LoomError> {
    match policy {
        0 => Ok(AcceleratorPolicy::ExactAlways),
        1 => Ok(AcceleratorPolicy::ApproximateAbove { threshold }),
        other => Err(LoomError::invalid(format!(
            "unknown vector accelerator policy {other}"
        ))),
    }
}

/// Decode a vector from little-endian `f32` bytes (4 per component).
pub fn floats_from_bytes(bytes: &[u8]) -> Result<Vec<f32>, LoomError> {
    if !bytes.len().is_multiple_of(4) {
        return Err(LoomError::invalid(
            "vector bytes length must be a multiple of 4 (little-endian f32)",
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

/// Encode a vector as little-endian `f32` bytes.
pub fn floats_to_bytes(vector: &[f32]) -> Vec<u8> {
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

/// Decode a metadata map from a CBOR `text -> cell` map. Empty input is an empty map.
pub fn metadata_from_cbor(bytes: &[u8]) -> Result<BTreeMap<String, Value>, LoomError> {
    if bytes.is_empty() {
        return Ok(BTreeMap::new());
    }
    let value = decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))?;
    let CborValue::Map(pairs) = value else {
        return Err(LoomError::invalid("vector metadata must be a CBOR map"));
    };
    let mut out = BTreeMap::new();
    for (k, v) in pairs {
        let CborValue::Text(key) = k else {
            return Err(LoomError::invalid("vector metadata keys must be text"));
        };
        out.insert(key, cell_from(v)?);
    }
    Ok(out)
}

fn meta_filter_from_value(value: CborValue) -> Result<MetaFilter, LoomError> {
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid("vector filter must be a CBOR array"));
    };
    let mut it = items.into_iter();
    let tag = match it.next() {
        Some(CborValue::Uint(t)) => t,
        _ => return Err(LoomError::invalid("vector filter tag must be a uint")),
    };
    match tag {
        0 => Ok(MetaFilter::All),
        1 => {
            let key = meta_filter_key(&mut it, "Eq")?;
            let cell = meta_filter_cell(&mut it, "Eq")?;
            Ok(MetaFilter::Eq(key, cell_from(cell)?))
        }
        2 => {
            let (a, b) = meta_filter_operands(&mut it, "And")?;
            Ok(MetaFilter::And(
                Box::new(meta_filter_from_value(a)?),
                Box::new(meta_filter_from_value(b)?),
            ))
        }
        3 => {
            let (a, b) = meta_filter_operands(&mut it, "Or")?;
            Ok(MetaFilter::Or(
                Box::new(meta_filter_from_value(a)?),
                Box::new(meta_filter_from_value(b)?),
            ))
        }
        4 => {
            let inner = it
                .next()
                .ok_or_else(|| LoomError::invalid("vector filter Not is missing its operand"))?;
            Ok(MetaFilter::Not(Box::new(meta_filter_from_value(inner)?)))
        }
        5 => {
            let key = meta_filter_key(&mut it, "Exists")?;
            Ok(MetaFilter::Exists(key))
        }
        6 => {
            let key = meta_filter_key(&mut it, "Ne")?;
            let cell = meta_filter_cell(&mut it, "Ne")?;
            Ok(MetaFilter::Ne(key, cell_from(cell)?))
        }
        7 => {
            let key = meta_filter_key(&mut it, "Lt")?;
            let cell = meta_filter_cell(&mut it, "Lt")?;
            Ok(MetaFilter::Lt(key, cell_from(cell)?))
        }
        8 => {
            let key = meta_filter_key(&mut it, "Le")?;
            let cell = meta_filter_cell(&mut it, "Le")?;
            Ok(MetaFilter::Le(key, cell_from(cell)?))
        }
        9 => {
            let key = meta_filter_key(&mut it, "Gt")?;
            let cell = meta_filter_cell(&mut it, "Gt")?;
            Ok(MetaFilter::Gt(key, cell_from(cell)?))
        }
        10 => {
            let key = meta_filter_key(&mut it, "Ge")?;
            let cell = meta_filter_cell(&mut it, "Ge")?;
            Ok(MetaFilter::Ge(key, cell_from(cell)?))
        }
        11 => {
            let key = meta_filter_key(&mut it, "In")?;
            let values = match it.next() {
                Some(CborValue::Array(values)) => values
                    .into_iter()
                    .map(cell_from)
                    .collect::<Result<Vec<_>, LoomError>>()?,
                _ => {
                    return Err(LoomError::invalid(
                        "vector filter In values must be an array",
                    ));
                }
            };
            Ok(MetaFilter::In(key, values))
        }
        other => Err(LoomError::invalid(format!(
            "unknown vector filter tag {other}"
        ))),
    }
}

fn meta_filter_key<I>(iter: &mut I, name: &str) -> Result<String, LoomError>
where
    I: Iterator<Item = CborValue>,
{
    match iter.next() {
        Some(CborValue::Text(key)) => Ok(key),
        _ => Err(LoomError::invalid(format!(
            "vector filter {name} key must be text"
        ))),
    }
}

fn meta_filter_cell<I>(iter: &mut I, name: &str) -> Result<CborValue, LoomError>
where
    I: Iterator<Item = CborValue>,
{
    iter.next()
        .ok_or_else(|| LoomError::invalid(format!("vector filter {name} is missing its value")))
}

fn meta_filter_operands<I>(iter: &mut I, name: &str) -> Result<(CborValue, CborValue), LoomError>
where
    I: Iterator<Item = CborValue>,
{
    let left = iter.next().ok_or_else(|| {
        LoomError::invalid(format!("vector filter {name} is missing its left operand"))
    })?;
    let right = iter.next().ok_or_else(|| {
        LoomError::invalid(format!("vector filter {name} is missing its right operand"))
    })?;
    Ok((left, right))
}

/// Decode a metadata filter. Empty input matches everything.
pub fn meta_filter_from_cbor(bytes: &[u8]) -> Result<MetaFilter, LoomError> {
    if bytes.is_empty() {
        return Ok(MetaFilter::All);
    }
    let value = decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))?;
    meta_filter_from_value(value)
}

/// Encode search hits as a CBOR array of `[id, score_cell]`.
pub fn hits_cbor(hits: Vec<Hit>) -> Vec<u8> {
    let items = hits
        .into_iter()
        .map(|h| {
            CborValue::Array(vec![
                CborValue::Text(h.id),
                cell_value(&Value::F32(h.score)),
            ])
        })
        .collect();
    encode(&CborValue::Array(items)).unwrap_or_default()
}

/// Encode the embedding-model profile as `[1, model_id, dimension, weights_digest]`.
pub fn embedding_model_cbor(model: &EmbeddingModel) -> Vec<u8> {
    encode(&CborValue::Array(vec![
        CborValue::Uint(1),
        CborValue::Text(model.model_id.clone()),
        CborValue::Uint(model.dimension as u64),
        CborValue::Text(model.weights_digest.clone().unwrap_or_default()),
    ]))
    .unwrap_or_default()
}

/// Encode a fetched entry as `[vector_bytes, metadata]`.
pub fn vector_entry_to_cbor(vector: &[f32], metadata: &BTreeMap<String, Value>) -> Vec<u8> {
    let value = CborValue::Array(vec![
        CborValue::Bytes(floats_to_bytes(vector)),
        metadata_to_value(metadata),
    ]);
    encode(&value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_round_trip_little_endian() {
        let v = vec![1.5f32, -2.0, 3.25];
        assert_eq!(floats_from_bytes(&floats_to_bytes(&v)).unwrap(), v);
    }

    #[test]
    fn odd_length_vector_bytes_rejected() {
        assert!(floats_from_bytes(&[0, 1, 2]).is_err());
    }

    #[test]
    fn empty_filter_matches_all() {
        assert!(matches!(
            meta_filter_from_cbor(&[]).unwrap(),
            MetaFilter::All
        ));
    }

    #[test]
    fn metadata_round_trip() {
        let mut meta = BTreeMap::new();
        meta.insert("k".to_string(), Value::Int(7));
        let bytes = encode(&metadata_to_value(&meta)).unwrap();
        assert_eq!(metadata_from_cbor(&bytes).unwrap(), meta);
    }
}
