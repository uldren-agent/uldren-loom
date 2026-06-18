//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::tabular::{CmpOp, ColumnType, cell_from, cell_value};
use loom_core::{
    ColumnarAggregate, ColumnarAggregateOp, ColumnarInspect, columnar_aggregate, columnar_append,
    columnar_columns, columnar_compact, columnar_create, columnar_inspect, columnar_rows,
    columnar_scan, columnar_select, columnar_source_digest,
};

// Columnar dataset (Columnar facet) - typed, append-only segmented rows in a named dataset within a
// workspace. A column schema crosses as a CBOR array of `[name, type_tag]` (the `ColumnType` wire
// tag); a row crosses as a CBOR cell array (the shared Value cell codec); a scan/select crosses as a
// CBOR array of rows. The select filter is the CBOR array `[column, op, value_cell]`, with op tags
// 0 eq, 1 ne, 2 lt, 3 le, 4 gt, 5 ge; an empty filter buffer scans every row.

/// Resolve a workspace for a columnar write by UUID or name, ensuring the `columnar` facet exists.
fn ensure_columnar_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Columnar,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Columnar)
        .map_err(le)?;
    Ok(ns)
}

/// Map an op tag (0 eq, 1 ne, 2 lt, 3 le, 4 gt, 5 ge) to a [`CmpOp`].
fn cmp_op_from_int(op: u64) -> Result<CmpOp, JsError> {
    match op {
        0 => Ok(CmpOp::Eq),
        1 => Ok(CmpOp::Ne),
        2 => Ok(CmpOp::Lt),
        3 => Ok(CmpOp::Le),
        4 => Ok(CmpOp::Gt),
        5 => Ok(CmpOp::Ge),
        other => Err(JsError::new(&format!("unknown columnar op tag {other}"))),
    }
}

fn aggregate_op_from_int(op: u64) -> Result<ColumnarAggregateOp, JsError> {
    match op {
        0 => Ok(ColumnarAggregateOp::Count),
        1 => Ok(ColumnarAggregateOp::CountNonNull),
        2 => Ok(ColumnarAggregateOp::Min),
        3 => Ok(ColumnarAggregateOp::Max),
        4 => Ok(ColumnarAggregateOp::Sum),
        other => Err(JsError::new(&format!(
            "unknown columnar aggregate op tag {other}"
        ))),
    }
}

/// Decode a canonical-CBOR array of `[name, type_tag]` into the column schema.
fn columns_from_cbor(bytes: &[u8]) -> Result<Vec<(String, ColumnType)>, JsError> {
    let value = loom_codec::decode(bytes).map_err(|e| JsError::new(&format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(JsError::new("columnar columns must be a CBOR array"));
    };
    let mut cols = Vec::with_capacity(items.len());
    for item in items {
        let CborValue::Array(pair) = item else {
            return Err(JsError::new(
                "each columnar column must be a [name, type_tag] array",
            ));
        };
        let mut it = pair.into_iter();
        let name = match it.next() {
            Some(CborValue::Text(n)) => n,
            _ => return Err(JsError::new("columnar column name must be text")),
        };
        let tag = match it.next() {
            Some(CborValue::Uint(t)) => u8::try_from(t)
                .map_err(|_| JsError::new("columnar column type tag out of range"))?,
            _ => return Err(JsError::new("columnar column type tag must be a uint")),
        };
        cols.push((name, ColumnType::from_tag(tag).map_err(le)?));
    }
    Ok(cols)
}

/// Encode the column schema as a canonical-CBOR array of `[name, type_tag]`.
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

/// Decode a canonical-CBOR cell array into a row of cells.
fn row_from_cbor(bytes: &[u8]) -> Result<Vec<Value>, JsError> {
    let value = loom_codec::decode(bytes).map_err(|e| JsError::new(&format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(JsError::new("columnar row must be a CBOR cell array"));
    };
    items
        .into_iter()
        .map(|c| cell_from(c).map_err(le))
        .collect()
}

/// Encode rows as a canonical-CBOR array of cell arrays.
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

/// Decode the projection column list (a canonical-CBOR array of text).
fn select_columns_from_cbor(bytes: &[u8]) -> Result<Vec<String>, JsError> {
    let value = loom_codec::decode(bytes).map_err(|e| JsError::new(&format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(JsError::new("columnar select columns must be a CBOR array"));
    };
    items
        .into_iter()
        .map(|item| match item {
            CborValue::Text(s) => Ok(s),
            _ => Err(JsError::new("columnar select column must be text")),
        })
        .collect()
}

/// Decode the select filter (empty buffer is no filter) as `[column, op, value_cell]`.
fn select_filter_from_cbor(bytes: &[u8]) -> Result<Option<(String, CmpOp, Value)>, JsError> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let value = loom_codec::decode(bytes).map_err(|e| JsError::new(&format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(JsError::new("columnar select filter must be a CBOR array"));
    };
    let mut it = items.into_iter();
    let column = match it.next() {
        Some(CborValue::Text(c)) => c,
        _ => return Err(JsError::new("columnar filter column must be text")),
    };
    let op = match it.next() {
        Some(CborValue::Uint(t)) => cmp_op_from_int(t)?,
        _ => return Err(JsError::new("columnar filter op must be a uint")),
    };
    let cell = it
        .next()
        .ok_or_else(|| JsError::new("columnar filter is missing its value cell"))?;
    Ok(Some((column, op, cell_from(cell).map_err(le)?)))
}

fn aggregates_from_cbor(bytes: &[u8]) -> Result<Vec<ColumnarAggregate>, JsError> {
    let value = loom_codec::decode(bytes).map_err(|e| JsError::new(&format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(JsError::new("columnar aggregates must be a CBOR array"));
    };
    items
        .into_iter()
        .map(|item| {
            let CborValue::Array(fields) = item else {
                return Err(JsError::new("columnar aggregate must be [op, column?]"));
            };
            let mut iter = fields.into_iter();
            let op = match iter.next() {
                Some(CborValue::Uint(tag)) => aggregate_op_from_int(tag)?,
                _ => return Err(JsError::new("columnar aggregate op must be a uint")),
            };
            let column = match iter.next() {
                Some(CborValue::Text(column)) => Some(column),
                Some(CborValue::Null) | None => None,
                _ => {
                    return Err(JsError::new(
                        "columnar aggregate column must be text or null",
                    ));
                }
            };
            if iter.next().is_some() {
                return Err(JsError::new("columnar aggregate has extra fields"));
            }
            Ok(ColumnarAggregate { op, column })
        })
        .collect()
}

#[wasm_bindgen]
impl LoomSql {
    /// Create columnar dataset `name` with `columns` (a CBOR array of `[name, type_tag]`) and
    /// `target_segment_rows` (0 for the engine default) in `workspace` (UUID or name, created with
    /// the `columnar` facet if absent).
    pub fn columnar_create(
        &mut self,
        workspace: String,
        name: String,
        columns: Vec<u8>,
        target_segment_rows: usize,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let columns = columns_from_cbor(&columns)?;
        let ns = ensure_columnar_ns(&mut self.loom, &workspace)?;
        columnar_create(&mut self.loom, ns, &name, columns, target_segment_rows).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Append `row` (a CBOR cell array) to dataset `name`, validating arity + column types.
    pub fn columnar_append(
        &mut self,
        workspace: String,
        name: String,
        row: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let row = row_from_cbor(&row)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        columnar_append(&mut self.loom, ns, &name, row).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// All rows of dataset `name` in append order as a CBOR array of cell arrays.
    pub fn columnar_scan(&self, workspace: String, name: String) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(rows_to_cbor(
            columnar_scan(&self.loom, ns, &name).map_err(le)?,
        ))
    }

    /// The `(name, type_tag)` columns of dataset `name` as a CBOR array of `[name, type_tag]`.
    pub fn columnar_columns(&self, workspace: String, name: String) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(columns_to_cbor(
            columnar_columns(&self.loom, ns, &name).map_err(le)?,
        ))
    }

    /// The total row count of dataset `name`.
    pub fn columnar_rows(&self, workspace: String, name: String) -> Result<usize, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        columnar_rows(&self.loom, ns, &name).map_err(le)
    }

    /// Compact dataset `name` at its target segment size.
    pub fn columnar_compact(&mut self, workspace: String, name: String) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        columnar_compact(&mut self.loom, ns, &name).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Inspect dataset metadata as CBOR `[columns, rows, segment_count, target_segment_rows, source_digest]`.
    pub fn columnar_inspect(&self, workspace: String, name: String) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(inspect_to_cbor(
            columnar_inspect(&self.loom, ns, &name).map_err(le)?,
        ))
    }

    /// Source digest used by derived columnar projections as CBOR text.
    pub fn columnar_source_digest(
        &self,
        workspace: String,
        name: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(digest_to_cbor(
            columnar_source_digest(&self.loom, ns, &name).map_err(le)?,
        ))
    }

    /// Project `columns` (a CBOR array of text) from dataset `name`'s rows matching `filter` as a
    /// CBOR array of cell arrays. The filter is the CBOR array `[column, op, value_cell]` (op: 0 eq,
    /// 1 ne, 2 lt, 3 le, 4 gt, 5 ge); an empty filter buffer scans every row.
    pub fn columnar_select(
        &self,
        workspace: String,
        name: String,
        columns: Vec<u8>,
        filter: Vec<u8>,
    ) -> Result<Vec<u8>, JsError> {
        let column_names = select_columns_from_cbor(&columns)?;
        let filter = select_filter_from_cbor(&filter)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let col_refs: Vec<&str> = column_names.iter().map(String::as_str).collect();
        let filter_ref = filter.as_ref().map(|(c, op, v)| (c.as_str(), *op, v));
        Ok(rows_to_cbor(
            columnar_select(&self.loom, ns, &name, &col_refs, filter_ref).map_err(le)?,
        ))
    }

    /// Evaluate aggregate expressions from CBOR `[[op, column?] ...]`, with optional select filter.
    pub fn columnar_aggregate(
        &self,
        workspace: String,
        name: String,
        aggregates: Vec<u8>,
        filter: Vec<u8>,
    ) -> Result<Vec<u8>, JsError> {
        let aggregates = aggregates_from_cbor(&aggregates)?;
        let filter = select_filter_from_cbor(&filter)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let filter_ref = filter.as_ref().map(|(c, op, v)| (c.as_str(), *op, v));
        Ok(values_to_cbor(
            columnar_aggregate(&self.loom, ns, &name, &aggregates, filter_ref).map_err(le)?,
        ))
    }
}
