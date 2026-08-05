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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextUpsertRequest {
    pub workspace: String,
    pub name: String,
    pub id: String,
    pub vector: Vec<u8>,
    pub metadata: Vec<u8>,
    pub source_text: Vec<u8>,
    pub model_id: Option<String>,
    pub weights_digest: Option<String>,
    pub create: bool,
    pub metric: i32,
    pub expected_token: Option<Vec<u8>>,
    pub expect_absent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextUpsertReport {
    pub id: String,
    pub collection: String,
    pub current_token: Vec<u8>,
}

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

pub fn text_upsert_report_to_cbor(id: &str, collection: &str) -> Vec<u8> {
    text_upsert_report_with_token_to_cbor(id, collection, &[])
}

pub fn text_upsert_report_with_token_to_cbor(
    id: &str,
    collection: &str,
    current_token: &[u8],
) -> Vec<u8> {
    encode(&CborValue::Array(vec![
        CborValue::Text(id.to_string()),
        CborValue::Text(collection.to_string()),
        CborValue::Bytes(current_token.to_vec()),
    ]))
    .unwrap_or_default()
}

pub fn text_upsert_report_from_cbor(bytes: &[u8]) -> Result<TextUpsertReport, LoomError> {
    let value = decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid(
            "vector text upsert report must be an array",
        ));
    };
    if items.len() != 3 {
        return Err(LoomError::invalid(
            "vector text upsert report must have three fields",
        ));
    }
    let mut items = items.into_iter();
    let Some(CborValue::Text(id)) = items.next() else {
        return Err(LoomError::invalid(
            "vector text upsert report id must be text",
        ));
    };
    let Some(CborValue::Text(collection)) = items.next() else {
        return Err(LoomError::invalid(
            "vector text upsert report collection must be text",
        ));
    };
    let Some(CborValue::Bytes(current_token)) = items.next() else {
        return Err(LoomError::invalid(
            "vector text upsert report current token must be bytes",
        ));
    };
    Ok(TextUpsertReport {
        id,
        collection,
        current_token,
    })
}

pub fn text_upsert_request_to_cbor(request: &TextUpsertRequest) -> Vec<u8> {
    encode(&CborValue::Array(vec![
        CborValue::Uint(1),
        CborValue::Text(request.workspace.clone()),
        CborValue::Text(request.name.clone()),
        CborValue::Text(request.id.clone()),
        CborValue::Bytes(request.vector.clone()),
        CborValue::Bytes(request.metadata.clone()),
        CborValue::Bytes(request.source_text.clone()),
        optional_text_value(request.model_id.as_deref()),
        optional_text_value(request.weights_digest.as_deref()),
        CborValue::Bool(request.create),
        CborValue::Uint(request.metric as u64),
        optional_bytes_value(request.expected_token.as_deref()),
        CborValue::Bool(request.expect_absent),
    ]))
    .unwrap_or_default()
}

pub fn text_upsert_request_from_cbor(bytes: &[u8]) -> Result<TextUpsertRequest, LoomError> {
    let value = decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid(
            "vector text upsert request must be an array",
        ));
    };
    if items.len() != 13 {
        return Err(LoomError::invalid(
            "vector text upsert request must have thirteen fields",
        ));
    }
    let mut items = items.into_iter();
    expect_version(items.next(), "vector text upsert request", 1)?;
    Ok(TextUpsertRequest {
        workspace: text_field(items.next(), "workspace")?,
        name: text_field(items.next(), "name")?,
        id: text_field(items.next(), "id")?,
        vector: bytes_field(items.next(), "vector")?,
        metadata: bytes_field(items.next(), "metadata")?,
        source_text: bytes_field(items.next(), "source_text")?,
        model_id: optional_text_field(items.next(), "model_id")?,
        weights_digest: optional_text_field(items.next(), "weights_digest")?,
        create: bool_field(items.next(), "create")?,
        metric: i32::try_from(uint_field(items.next(), "metric")?)
            .map_err(|_| LoomError::invalid("vector text upsert metric out of range"))?,
        expected_token: optional_bytes_field(items.next(), "expected_token")?,
        expect_absent: bool_field(items.next(), "expect_absent")?,
    })
}

fn expect_version(value: Option<CborValue>, name: &str, expected: u64) -> Result<(), LoomError> {
    let actual = uint_field(value, "version")?;
    if actual == expected {
        Ok(())
    } else {
        Err(LoomError::invalid(format!(
            "{name} version must be {expected}, got {actual}"
        )))
    }
}

fn optional_text_value(value: Option<&str>) -> CborValue {
    value.map_or(CborValue::Null, |value| CborValue::Text(value.to_string()))
}

fn optional_bytes_value(value: Option<&[u8]>) -> CborValue {
    value.map_or(CborValue::Null, |value| CborValue::Bytes(value.to_vec()))
}

fn uint_field(value: Option<CborValue>, name: &str) -> Result<u64, LoomError> {
    match value {
        Some(CborValue::Uint(value)) => Ok(value),
        _ => Err(LoomError::invalid(format!(
            "vector text upsert request {name} must be uint"
        ))),
    }
}

fn text_field(value: Option<CborValue>, name: &str) -> Result<String, LoomError> {
    match value {
        Some(CborValue::Text(value)) => Ok(value),
        _ => Err(LoomError::invalid(format!(
            "vector text upsert request {name} must be text"
        ))),
    }
}

fn bytes_field(value: Option<CborValue>, name: &str) -> Result<Vec<u8>, LoomError> {
    match value {
        Some(CborValue::Bytes(value)) => Ok(value),
        _ => Err(LoomError::invalid(format!(
            "vector text upsert request {name} must be bytes"
        ))),
    }
}

fn bool_field(value: Option<CborValue>, name: &str) -> Result<bool, LoomError> {
    match value {
        Some(CborValue::Bool(value)) => Ok(value),
        _ => Err(LoomError::invalid(format!(
            "vector text upsert request {name} must be bool"
        ))),
    }
}

fn optional_text_field(value: Option<CborValue>, name: &str) -> Result<Option<String>, LoomError> {
    match value {
        Some(CborValue::Null) => Ok(None),
        Some(CborValue::Text(value)) => Ok(Some(value)),
        _ => Err(LoomError::invalid(format!(
            "vector text upsert request {name} must be text or null"
        ))),
    }
}

fn optional_bytes_field(
    value: Option<CborValue>,
    name: &str,
) -> Result<Option<Vec<u8>>, LoomError> {
    match value {
        Some(CborValue::Null) => Ok(None),
        Some(CborValue::Bytes(value)) => Ok(Some(value)),
        _ => Err(LoomError::invalid(format!(
            "vector text upsert request {name} must be bytes or null"
        ))),
    }
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
