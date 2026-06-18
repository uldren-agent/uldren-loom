//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::tabular::{CmpOp, ColumnType, cell_from, cell_value};
use loom_core::{ColumnarAggregate, ColumnarAggregateOp, ColumnarInspect};

/// Map a comparison-op tag to [`CmpOp`]: 0 eq, 1 ne, 2 lt, 3 le, 4 gt, 5 ge.
fn cmp_op_from_int(op: u64) -> PyResult<CmpOp> {
    match op {
        0 => Ok(CmpOp::Eq),
        1 => Ok(CmpOp::Ne),
        2 => Ok(CmpOp::Lt),
        3 => Ok(CmpOp::Le),
        4 => Ok(CmpOp::Gt),
        5 => Ok(CmpOp::Ge),
        other => Err(PyRuntimeError::new_err(format!(
            "unknown columnar op tag {other}"
        ))),
    }
}

fn aggregate_op_from_int(op: u64) -> PyResult<ColumnarAggregateOp> {
    match op {
        0 => Ok(ColumnarAggregateOp::Count),
        1 => Ok(ColumnarAggregateOp::CountNonNull),
        2 => Ok(ColumnarAggregateOp::Min),
        3 => Ok(ColumnarAggregateOp::Max),
        4 => Ok(ColumnarAggregateOp::Sum),
        other => Err(PyRuntimeError::new_err(format!(
            "unknown columnar aggregate op tag {other}"
        ))),
    }
}

/// Decode a CBOR array of `[name, type_tag]` into a column schema.
fn columns_from_cbor(bytes: &[u8]) -> PyResult<Vec<(String, ColumnType)>> {
    let value =
        loom_codec::decode(bytes).map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(PyRuntimeError::new_err(
            "columnar columns must be a CBOR array",
        ));
    };
    let mut cols = Vec::with_capacity(items.len());
    for item in items {
        let CborValue::Array(pair) = item else {
            return Err(PyRuntimeError::new_err(
                "each columnar column must be a [name, type_tag] array",
            ));
        };
        let mut it = pair.into_iter();
        let name = match it.next() {
            Some(CborValue::Text(n)) => n,
            _ => return Err(PyRuntimeError::new_err("columnar column name must be text")),
        };
        let tag = match it.next() {
            Some(CborValue::Uint(t)) => u8::try_from(t)
                .map_err(|_| PyRuntimeError::new_err("columnar column type tag out of range"))?,
            _ => {
                return Err(PyRuntimeError::new_err(
                    "columnar column type tag must be a uint",
                ));
            }
        };
        cols.push((name, ColumnType::from_tag(tag).map_err(py_err)?));
    }
    Ok(cols)
}

/// Encode a column schema as a CBOR array of `[name, type_tag]`.
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

/// Decode a CBOR cell array into a row.
fn row_from_cbor(bytes: &[u8]) -> PyResult<Vec<Value>> {
    let value =
        loom_codec::decode(bytes).map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(PyRuntimeError::new_err(
            "columnar row must be a CBOR cell array",
        ));
    };
    items
        .into_iter()
        .map(|c| cell_from(c).map_err(py_err))
        .collect()
}

/// Encode rows as a CBOR array of cell arrays.
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

/// Decode a CBOR array of text into column names for a select projection.
fn select_columns_from_cbor(bytes: &[u8]) -> PyResult<Vec<String>> {
    let value =
        loom_codec::decode(bytes).map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(PyRuntimeError::new_err(
            "columnar select columns must be a CBOR array",
        ));
    };
    items
        .into_iter()
        .map(|item| match item {
            CborValue::Text(s) => Ok(s),
            _ => Err(PyRuntimeError::new_err(
                "columnar select column must be text",
            )),
        })
        .collect()
}

/// Decode a select filter `[column, op, value_cell]`; an empty buffer scans every row.
fn select_filter_from_cbor(bytes: &[u8]) -> PyResult<Option<(String, CmpOp, Value)>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let value =
        loom_codec::decode(bytes).map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(PyRuntimeError::new_err(
            "columnar select filter must be a CBOR array",
        ));
    };
    let mut it = items.into_iter();
    let column = match it.next() {
        Some(CborValue::Text(c)) => c,
        _ => {
            return Err(PyRuntimeError::new_err(
                "columnar filter column must be text",
            ));
        }
    };
    let op = match it.next() {
        Some(CborValue::Uint(t)) => cmp_op_from_int(t)?,
        _ => return Err(PyRuntimeError::new_err("columnar filter op must be a uint")),
    };
    let cell = it
        .next()
        .ok_or_else(|| PyRuntimeError::new_err("columnar filter is missing its value cell"))?;
    Ok(Some((column, op, cell_from(cell).map_err(py_err)?)))
}

fn aggregates_from_cbor(bytes: &[u8]) -> PyResult<Vec<ColumnarAggregate>> {
    let value =
        loom_codec::decode(bytes).map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(PyRuntimeError::new_err(
            "columnar aggregates must be a CBOR array",
        ));
    };
    items
        .into_iter()
        .map(|item| {
            let CborValue::Array(fields) = item else {
                return Err(PyRuntimeError::new_err(
                    "columnar aggregate must be [op, column?]",
                ));
            };
            let mut iter = fields.into_iter();
            let op = match iter.next() {
                Some(CborValue::Uint(tag)) => aggregate_op_from_int(tag)?,
                _ => {
                    return Err(PyRuntimeError::new_err(
                        "columnar aggregate op must be a uint",
                    ));
                }
            };
            let column = match iter.next() {
                Some(CborValue::Text(column)) => Some(column),
                Some(CborValue::Null) | None => None,
                _ => {
                    return Err(PyRuntimeError::new_err(
                        "columnar aggregate column must be text or null",
                    ));
                }
            };
            if iter.next().is_some() {
                return Err(PyRuntimeError::new_err(
                    "columnar aggregate has extra fields",
                ));
            }
            Ok(ColumnarAggregate { op, column })
        })
        .collect()
}

/// Create columnar dataset `name` with `columns` (a CBOR array of `[name, type_tag]`) and
/// `target_segment_rows` (0 for the engine default) in `workspace` (UUID or name, created with the
/// `columnar` facet if absent). `CONFLICT` if the dataset already exists.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, columns, target_segment_rows, passphrase=None))]
pub(crate) fn columnar_create(
    path: &str,
    workspace: &str,
    name: &str,
    columns: &[u8],
    target_segment_rows: usize,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let columns = columns_from_cbor(columns)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_columnar_ns(&mut loom, workspace)?;
    loom_core::columnar_create(&mut loom, ns, name, columns, target_segment_rows)
        .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Append `row` (a CBOR cell array) to dataset `name`, validating arity + column types. `NOT_FOUND` if
/// the dataset was never created; `INVALID_ARGUMENT` on an arity or type mismatch.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, row, passphrase=None))]
pub(crate) fn columnar_append(
    path: &str,
    workspace: &str,
    name: &str,
    row: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let row = row_from_cbor(row)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_core::columnar_append(&mut loom, ns, name, row).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// All rows of dataset `name` in append order as a CBOR array of cell arrays. `NOT_FOUND` if the dataset
/// does not exist.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn columnar_scan<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = rows_to_cbor(loom_core::columnar_scan(&loom, ns, name).map_err(py_err)?);
    Ok(PyBytes::new(py, &bytes))
}
/// The `(name, type_tag)` columns of dataset `name` as a CBOR array of `[name, type_tag]`. `NOT_FOUND` if
/// the dataset does not exist.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn columnar_columns<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = columns_to_cbor(loom_core::columnar_columns(&loom, ns, name).map_err(py_err)?);
    Ok(PyBytes::new(py, &bytes))
}
/// The total row count of dataset `name`. `NOT_FOUND` if the dataset does not exist.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn columnar_rows(
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<u64> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::columnar_rows(&loom, ns, name).map_err(py_err)? as u64)
}
/// Compact dataset `name` at its target segment size.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn columnar_compact(
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_core::columnar_compact(&mut loom, ns, name).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Inspect dataset metadata as CBOR `[columns, rows, segment_count, target_segment_rows, source_digest]`.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn columnar_inspect<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = inspect_to_cbor(loom_core::columnar_inspect(&loom, ns, name).map_err(py_err)?);
    Ok(PyBytes::new(py, &bytes))
}
/// Source digest used by derived columnar projections as CBOR text.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, passphrase=None))]
pub(crate) fn columnar_source_digest<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = digest_to_cbor(loom_core::columnar_source_digest(&loom, ns, name).map_err(py_err)?);
    Ok(PyBytes::new(py, &bytes))
}
/// Project `columns` (a CBOR array of text) from dataset `name`'s rows matching `filter` as a CBOR array
/// of cell arrays. The filter is the CBOR array `[column, op, value_cell]` (op: 0 eq, 1 ne, 2 lt, 3 le,
/// 4 gt, 5 ge); an empty filter buffer scans every row. `NOT_FOUND` if the dataset does not exist;
/// `INVALID_ARGUMENT` on an unknown column.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, columns, filter, passphrase=None))]
pub(crate) fn columnar_select<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    columns: &[u8],
    filter: &[u8],
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let column_names = select_columns_from_cbor(columns)?;
    let filter = select_filter_from_cbor(filter)?;
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let col_refs: Vec<&str> = column_names.iter().map(String::as_str).collect();
    let filter_ref = filter.as_ref().map(|(c, op, v)| (c.as_str(), *op, v));
    let bytes = rows_to_cbor(
        loom_core::columnar_select(&loom, ns, name, &col_refs, filter_ref).map_err(py_err)?,
    );
    Ok(PyBytes::new(py, &bytes))
}
/// Evaluate aggregate expressions from CBOR `[[op, column?] ...]`, with optional select filter.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, aggregates, filter, passphrase=None))]
pub(crate) fn columnar_aggregate<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    aggregates: &[u8],
    filter: &[u8],
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let aggregates = aggregates_from_cbor(aggregates)?;
    let filter = select_filter_from_cbor(filter)?;
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let filter_ref = filter.as_ref().map(|(c, op, v)| (c.as_str(), *op, v));
    let bytes = values_to_cbor(
        loom_core::columnar_aggregate(&loom, ns, name, &aggregates, filter_ref).map_err(py_err)?,
    );
    Ok(PyBytes::new(py, &bytes))
}
