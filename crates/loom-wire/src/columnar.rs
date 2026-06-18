//! Canonical wire CBOR codecs for the columnar facet, shared by the C ABI, the in-process client
//! service impl, and the server dispatch. Columns cross as a CBOR array of `[name, type_tag]`; a row
//! and a value list cross as CBOR cell arrays (the shared [`cell_value`]/[`cell_from`] codec); a scan
//! or select crosses as a CBOR array of rows; a select filter is the CBOR array `[column, op_tag,
//! value_cell]`.

use loom_codec::{Value as CborValue, decode, encode};
use loom_core::tabular::{CmpOp, ColumnType, Value, cell_from, cell_value};
use loom_core::{ColumnarAggregate, ColumnarAggregateOp, ColumnarInspect, Digest};
use loom_types::LoomError;

/// Decode a select/aggregate comparison op tag.
pub fn cmp_op_from_int(op: u64) -> Result<CmpOp, LoomError> {
    match op {
        0 => Ok(CmpOp::Eq),
        1 => Ok(CmpOp::Ne),
        2 => Ok(CmpOp::Lt),
        3 => Ok(CmpOp::Le),
        4 => Ok(CmpOp::Gt),
        5 => Ok(CmpOp::Ge),
        other => Err(LoomError::invalid(format!(
            "unknown columnar op tag {other}"
        ))),
    }
}

/// Decode an aggregate op tag.
pub fn aggregate_op_from_int(op: u64) -> Result<ColumnarAggregateOp, LoomError> {
    match op {
        0 => Ok(ColumnarAggregateOp::Count),
        1 => Ok(ColumnarAggregateOp::CountNonNull),
        2 => Ok(ColumnarAggregateOp::Min),
        3 => Ok(ColumnarAggregateOp::Max),
        4 => Ok(ColumnarAggregateOp::Sum),
        other => Err(LoomError::invalid(format!(
            "unknown columnar aggregate op tag {other}"
        ))),
    }
}

/// Decode a columnar schema from a CBOR array of `[name, type_tag]` pairs.
pub fn columns_from_cbor(bytes: &[u8]) -> Result<Vec<(String, ColumnType)>, LoomError> {
    let value = decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid("columnar columns must be a CBOR array"));
    };
    let mut cols = Vec::with_capacity(items.len());
    for item in items {
        let CborValue::Array(pair) = item else {
            return Err(LoomError::invalid(
                "each columnar column must be a [name, type_tag] array",
            ));
        };
        let mut it = pair.into_iter();
        let name = match it.next() {
            Some(CborValue::Text(n)) => n,
            _ => return Err(LoomError::invalid("columnar column name must be text")),
        };
        let tag = match it.next() {
            Some(CborValue::Uint(t)) => u8::try_from(t)
                .map_err(|_| LoomError::invalid("columnar column type tag out of range"))?,
            _ => {
                return Err(LoomError::invalid(
                    "columnar column type tag must be a uint",
                ));
            }
        };
        cols.push((name, ColumnType::from_tag(tag)?));
    }
    Ok(cols)
}

/// Encode a columnar schema as a CBOR array of `[name, type_tag]` pairs.
pub fn columns_to_cbor(columns: Vec<(String, ColumnType)>) -> Vec<u8> {
    let items = columns
        .into_iter()
        .map(|(name, ty)| {
            CborValue::Array(vec![
                CborValue::Text(name),
                CborValue::Uint(u64::from(ty.tag())),
            ])
        })
        .collect();
    encode(&CborValue::Array(items)).unwrap_or_default()
}

/// Decode a single row from a CBOR cell array.
pub fn row_from_cbor(bytes: &[u8]) -> Result<Vec<Value>, LoomError> {
    let value = decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid("columnar row must be a CBOR cell array"));
    };
    items.into_iter().map(cell_from).collect()
}

/// Encode rows as a CBOR array of cell arrays.
pub fn rows_to_cbor(rows: Vec<Vec<Value>>) -> Vec<u8> {
    let items = rows
        .into_iter()
        .map(|row| CborValue::Array(row.iter().map(cell_value).collect()))
        .collect();
    encode(&CborValue::Array(items)).unwrap_or_default()
}

/// Encode a single value list as a CBOR cell array.
pub fn values_to_cbor(values: Vec<Value>) -> Vec<u8> {
    encode(&CborValue::Array(
        values.iter().map(cell_value).collect::<Vec<_>>(),
    ))
    .unwrap_or_default()
}

/// Encode a columnar inspection report.
pub fn inspect_to_cbor(inspect: ColumnarInspect) -> Vec<u8> {
    encode(&CborValue::Array(vec![
        CborValue::Array(
            inspect
                .columns
                .into_iter()
                .map(|(name, ty)| {
                    CborValue::Array(vec![
                        CborValue::Text(name),
                        CborValue::Uint(u64::from(ty.tag())),
                    ])
                })
                .collect(),
        ),
        CborValue::Uint(inspect.rows as u64),
        CborValue::Uint(inspect.segment_count as u64),
        CborValue::Uint(inspect.target_segment_rows as u64),
        CborValue::Text(inspect.source_digest.to_string()),
    ]))
    .unwrap_or_default()
}

/// Encode a single content address as CBOR text.
pub fn digest_to_cbor(digest: Digest) -> Vec<u8> {
    encode(&CborValue::Text(digest.to_string())).unwrap_or_default()
}

/// Decode aggregate expressions from a CBOR array of `[op_tag, column?]` items.
pub fn aggregates_from_cbor(bytes: &[u8]) -> Result<Vec<ColumnarAggregate>, LoomError> {
    let value = decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid(
            "columnar aggregates must be a CBOR array",
        ));
    };
    items
        .into_iter()
        .map(|item| {
            let CborValue::Array(fields) = item else {
                return Err(LoomError::invalid(
                    "columnar aggregate must be [op, column?]",
                ));
            };
            let mut iter = fields.into_iter();
            let op = match iter.next() {
                Some(CborValue::Uint(tag)) => aggregate_op_from_int(tag)?,
                _ => return Err(LoomError::invalid("columnar aggregate op must be a uint")),
            };
            let column = match iter.next() {
                Some(CborValue::Text(column)) => Some(column),
                Some(CborValue::Null) | None => None,
                _ => {
                    return Err(LoomError::invalid(
                        "columnar aggregate column must be text or null",
                    ));
                }
            };
            if iter.next().is_some() {
                return Err(LoomError::invalid("columnar aggregate has extra fields"));
            }
            Ok(ColumnarAggregate { op, column })
        })
        .collect()
}

/// Decode the projected column names for a select from a CBOR array of text.
pub fn select_columns_from_cbor(bytes: &[u8]) -> Result<Vec<String>, LoomError> {
    let value = decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid(
            "columnar select columns must be a CBOR array",
        ));
    };
    items
        .into_iter()
        .map(|item| match item {
            CborValue::Text(s) => Ok(s),
            _ => Err(LoomError::invalid("columnar select column must be text")),
        })
        .collect()
}

/// Decode an optional select filter `[column, op_tag, value_cell]`. Empty input is no filter.
pub fn select_filter_from_cbor(bytes: &[u8]) -> Result<Option<(String, CmpOp, Value)>, LoomError> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let value = decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::invalid(
            "columnar select filter must be a CBOR array",
        ));
    };
    let mut it = items.into_iter();
    let column = match it.next() {
        Some(CborValue::Text(c)) => c,
        _ => return Err(LoomError::invalid("columnar filter column must be text")),
    };
    let op = match it.next() {
        Some(CborValue::Uint(t)) => cmp_op_from_int(t)?,
        _ => return Err(LoomError::invalid("columnar filter op must be a uint")),
    };
    let cell = it
        .next()
        .ok_or_else(|| LoomError::invalid("columnar filter is missing its value cell"))?;
    Ok(Some((column, op, cell_from(cell)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_round_trip() {
        let cols = vec![
            ("id".to_string(), ColumnType::Int),
            ("name".to_string(), ColumnType::Text),
        ];
        let bytes = columns_to_cbor(cols.clone());
        assert_eq!(columns_from_cbor(&bytes).unwrap(), cols);
    }

    #[test]
    fn row_round_trip_through_cell_codec() {
        let bytes = rows_to_cbor(vec![vec![Value::Int(1), Value::Text("a".into())]]);
        let value = decode(&bytes).unwrap();
        let CborValue::Array(rows) = value else {
            panic!("expected array of rows");
        };
        assert_eq!(rows.len(), 1);
        let CborValue::Array(cells) = rows.into_iter().next().unwrap() else {
            panic!("expected cell array");
        };
        let decoded: Vec<Value> = cells.into_iter().map(|c| cell_from(c).unwrap()).collect();
        assert_eq!(decoded, vec![Value::Int(1), Value::Text("a".into())]);
    }

    #[test]
    fn empty_filter_is_none() {
        assert!(select_filter_from_cbor(&[]).unwrap().is_none());
    }
}
