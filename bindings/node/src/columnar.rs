//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::tabular::{CmpOp, ColumnType, cell_from, cell_value};
use loom_core::{ColumnarAggregate, ColumnarAggregateOp, ColumnarInspect};

// A column schema crosses as a CBOR array of `[name, type_tag]` (the `ColumnType` wire tag); a row
// crosses as a CBOR cell array (the shared Value cell codec); a scan/select crosses as a CBOR array of
// rows. The select filter is the CBOR array `[column, op, value_cell]`, with op tags 0 eq, 1 ne, 2 lt,
// 3 le, 4 gt, 5 ge; an empty filter buffer scans every row.

fn cmp_op_from_int(op: u64) -> napi::Result<CmpOp> {
    match op {
        0 => Ok(CmpOp::Eq),
        1 => Ok(CmpOp::Ne),
        2 => Ok(CmpOp::Lt),
        3 => Ok(CmpOp::Le),
        4 => Ok(CmpOp::Gt),
        5 => Ok(CmpOp::Ge),
        other => Err(napi::Error::from_reason(format!(
            "unknown columnar op tag {other}"
        ))),
    }
}

fn aggregate_op_from_int(op: u64) -> napi::Result<ColumnarAggregateOp> {
    match op {
        0 => Ok(ColumnarAggregateOp::Count),
        1 => Ok(ColumnarAggregateOp::CountNonNull),
        2 => Ok(ColumnarAggregateOp::Min),
        3 => Ok(ColumnarAggregateOp::Max),
        4 => Ok(ColumnarAggregateOp::Sum),
        other => Err(napi::Error::from_reason(format!(
            "unknown columnar aggregate op tag {other}"
        ))),
    }
}

fn columns_from_cbor(bytes: &[u8]) -> napi::Result<Vec<(String, ColumnType)>> {
    let value =
        loom_codec::decode(bytes).map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(napi::Error::from_reason(
            "columnar columns must be a CBOR array",
        ));
    };
    let mut cols = Vec::with_capacity(items.len());
    for item in items {
        let CborValue::Array(pair) = item else {
            return Err(napi::Error::from_reason(
                "each columnar column must be a [name, type_tag] array",
            ));
        };
        let mut it = pair.into_iter();
        let name = match it.next() {
            Some(CborValue::Text(n)) => n,
            _ => {
                return Err(napi::Error::from_reason(
                    "columnar column name must be text",
                ));
            }
        };
        let tag = match it.next() {
            Some(CborValue::Uint(t)) => u8::try_from(t)
                .map_err(|_| napi::Error::from_reason("columnar column type tag out of range"))?,
            _ => {
                return Err(napi::Error::from_reason(
                    "columnar column type tag must be a uint",
                ));
            }
        };
        cols.push((name, ColumnType::from_tag(tag).map_err(reason)?));
    }
    Ok(cols)
}

fn columns_to_cbor(columns: Vec<(String, ColumnType)>) -> Vec<u8> {
    let items = columns
        .into_iter()
        .map(|(name, ty)| {
            CborValue::Array(vec![
                CborValue::Text(name),
                CborValue::Uint(u64::from(ty.tag())),
            ])
        })
        .collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

fn row_from_cbor(bytes: &[u8]) -> napi::Result<Vec<Value>> {
    let value =
        loom_codec::decode(bytes).map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(napi::Error::from_reason(
            "columnar row must be a CBOR cell array",
        ));
    };
    items
        .into_iter()
        .map(|item| cell_from(item).map_err(reason))
        .collect()
}

fn rows_to_cbor(rows: Vec<Vec<Value>>) -> Vec<u8> {
    let items = rows
        .into_iter()
        .map(|row| CborValue::Array(row.iter().map(cell_value).collect()))
        .collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

fn values_to_cbor(values: Vec<Value>) -> Vec<u8> {
    cbor_encode(&CborValue::Array(
        values.iter().map(cell_value).collect::<Vec<_>>(),
    ))
    .unwrap_or_default()
}

fn inspect_to_cbor(inspect: ColumnarInspect) -> Vec<u8> {
    cbor_encode(&CborValue::Array(vec![
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

fn digest_to_cbor(digest: loom_core::Digest) -> Vec<u8> {
    cbor_encode(&CborValue::Text(digest.to_string())).unwrap_or_default()
}

fn select_columns_from_cbor(bytes: &[u8]) -> napi::Result<Vec<String>> {
    let value =
        loom_codec::decode(bytes).map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(napi::Error::from_reason(
            "columnar select columns must be a CBOR array",
        ));
    };
    items
        .into_iter()
        .map(|item| match item {
            CborValue::Text(s) => Ok(s),
            _ => Err(napi::Error::from_reason(
                "columnar select column must be text",
            )),
        })
        .collect()
}

fn select_filter_from_cbor(bytes: &[u8]) -> napi::Result<Option<(String, CmpOp, Value)>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let value =
        loom_codec::decode(bytes).map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(napi::Error::from_reason(
            "columnar select filter must be a CBOR array",
        ));
    };
    let mut it = items.into_iter();
    let column = match it.next() {
        Some(CborValue::Text(c)) => c,
        _ => {
            return Err(napi::Error::from_reason(
                "columnar filter column must be text",
            ));
        }
    };
    let op = match it.next() {
        Some(CborValue::Uint(t)) => cmp_op_from_int(t)?,
        _ => {
            return Err(napi::Error::from_reason(
                "columnar filter op must be a uint",
            ));
        }
    };
    let cell = it
        .next()
        .ok_or_else(|| napi::Error::from_reason("columnar filter is missing its value cell"))?;
    Ok(Some((column, op, cell_from(cell).map_err(reason)?)))
}

fn aggregates_from_cbor(bytes: &[u8]) -> napi::Result<Vec<ColumnarAggregate>> {
    let value =
        loom_codec::decode(bytes).map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(napi::Error::from_reason(
            "columnar aggregates must be a CBOR array",
        ));
    };
    items
        .into_iter()
        .map(|item| {
            let CborValue::Array(fields) = item else {
                return Err(napi::Error::from_reason(
                    "columnar aggregate must be [op, column?]",
                ));
            };
            let mut iter = fields.into_iter();
            let op = match iter.next() {
                Some(CborValue::Uint(tag)) => aggregate_op_from_int(tag)?,
                _ => {
                    return Err(napi::Error::from_reason(
                        "columnar aggregate op must be a uint",
                    ));
                }
            };
            let column = match iter.next() {
                Some(CborValue::Text(column)) => Some(column),
                Some(CborValue::Null) | None => None,
                _ => {
                    return Err(napi::Error::from_reason(
                        "columnar aggregate column must be text or null",
                    ));
                }
            };
            if iter.next().is_some() {
                return Err(napi::Error::from_reason(
                    "columnar aggregate has extra fields",
                ));
            }
            Ok(ColumnarAggregate { op, column })
        })
        .collect()
}

/// Create columnar dataset `name` with `columns` (a CBOR array of `[name, type_tag]`) and
/// `targetSegmentRows` (0 for the engine default) in workspace `workspace` (UUID or name, created with
/// the `columnar` facet if absent). `CONFLICT` if the dataset already exists.
#[napi]
pub fn columnar_create(
    loom_path: String,
    workspace: String,
    name: String,
    columns: Uint8Array,
    target_segment_rows: BigInt,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let columns = columns_from_cbor(&columns)?;
    let target_segment_rows = bigint_to_usize(target_segment_rows, "targetSegmentRows")?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_columnar_ns(&mut loom, &workspace)?;
    loom_core::columnar_create(&mut loom, ns, &name, columns, target_segment_rows)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Append `row` (a CBOR cell array) to dataset `name`, validating arity + column types. `NOT_FOUND` if
/// the dataset was never created; `INVALID_ARGUMENT` on an arity or type mismatch.
#[napi]
pub fn columnar_append(
    loom_path: String,
    workspace: String,
    name: String,
    row: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let row = row_from_cbor(&row)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom_core::columnar_append(&mut loom, ns, &name, row).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// All rows of dataset `name` in append order as the Loom Canonical CBOR array of cell arrays.
/// `NOT_FOUND` if the dataset does not exist.
#[napi]
pub fn columnar_scan(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(rows_to_cbor(
        loom_core::columnar_scan(&loom, ns, &name).map_err(reason)?,
    )))
}
/// The `(name, type_tag)` columns of dataset `name` as the Loom Canonical CBOR array of `[name,
/// type_tag]`. `NOT_FOUND` if the dataset does not exist.
#[napi]
pub fn columnar_columns(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(columns_to_cbor(
        loom_core::columnar_columns(&loom, ns, &name).map_err(reason)?,
    )))
}
/// The total row count of dataset `name`. `NOT_FOUND` if the dataset does not exist.
#[napi]
pub fn columnar_rows(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<BigInt> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let count = loom_core::columnar_rows(&loom, ns, &name).map_err(reason)?;
    Ok(BigInt::from(count as u64))
}
/// Compact dataset `name` at its target segment size.
#[napi]
pub fn columnar_compact(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom_core::columnar_compact(&mut loom, ns, &name).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Inspect dataset metadata as CBOR `[columns, rows, segment_count, target_segment_rows, source_digest]`.
#[napi]
pub fn columnar_inspect(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(inspect_to_cbor(
        loom_core::columnar_inspect(&loom, ns, &name).map_err(reason)?,
    )))
}
/// Source digest used by derived columnar projections as CBOR text.
#[napi]
pub fn columnar_source_digest(
    loom_path: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(digest_to_cbor(
        loom_core::columnar_source_digest(&loom, ns, &name).map_err(reason)?,
    )))
}
/// Project `columns` (a CBOR array of text) from dataset `name`'s rows matching `filter` as the Loom
/// Canonical CBOR array of cell arrays. The filter is the CBOR array `[column, op, value_cell]` (op: 0
/// eq, 1 ne, 2 lt, 3 le, 4 gt, 5 ge); an empty filter buffer scans every row. `NOT_FOUND` if the dataset
/// does not exist; `INVALID_ARGUMENT` on an unknown column.
#[napi]
pub fn columnar_select(
    loom_path: String,
    workspace: String,
    name: String,
    columns: Uint8Array,
    filter: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let column_names = select_columns_from_cbor(&columns)?;
    let filter = select_filter_from_cbor(&filter)?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let col_refs: Vec<&str> = column_names.iter().map(String::as_str).collect();
    let filter_ref = filter.as_ref().map(|(c, op, v)| (c.as_str(), *op, v));
    Ok(Uint8Array::from(rows_to_cbor(
        loom_core::columnar_select(&loom, ns, &name, &col_refs, filter_ref).map_err(reason)?,
    )))
}
/// Evaluate aggregate expressions from CBOR `[[op, column?] ...]`, with optional select filter.
#[napi]
pub fn columnar_aggregate(
    loom_path: String,
    workspace: String,
    name: String,
    aggregates: Uint8Array,
    filter: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let aggregates = aggregates_from_cbor(&aggregates)?;
    let filter = select_filter_from_cbor(&filter)?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let filter_ref = filter.as_ref().map(|(c, op, v)| (c.as_str(), *op, v));
    Ok(Uint8Array::from(values_to_cbor(
        loom_core::columnar_aggregate(&loom, ns, &name, &aggregates, filter_ref).map_err(reason)?,
    )))
}
