//! Canonical CBOR adapters shared by MCP facet read/write projections.

use std::collections::BTreeMap;

use loom_codec::Value as WireValue;
use loom_core::error::{LoomError, Result};
use loom_core::inference::EmbeddingModel;
use loom_core::tabular::{CmpOp, ColumnType, Value, cell_from, cell_value};
use loom_core::{
    AcceleratorPolicy, ColumnarAggregate, ColumnarAggregateOp, ColumnarInspect, DataframeBatch,
    Digest, Document, Edge, FieldValue, GRAPH_GEOMETRY_TAG, GraphCrs, GraphGeometry, GraphValue,
    Hit, Mapping, MetaFilter, Metric, Props, QueryRequest, QueryResponse,
};

fn codec_err(e: impl std::fmt::Display) -> LoomError {
    LoomError::invalid(format!("cbor: {e}"))
}

fn decode(bytes: &[u8]) -> Result<WireValue> {
    loom_codec::decode(bytes).map_err(codec_err)
}

fn encode(value: &WireValue) -> Result<Vec<u8>> {
    loom_codec::encode(value).map_err(codec_err)
}

pub fn props_from_cbor(bytes: &[u8]) -> Result<Props> {
    let WireValue::Map(pairs) = decode(bytes)? else {
        return Err(LoomError::invalid("graph props must be a CBOR map"));
    };
    let mut props = Props::new();
    for (key, value) in pairs {
        let WireValue::Text(key) = key else {
            return Err(LoomError::invalid("graph prop key must be text"));
        };
        props.insert(key, graph_value_from_cbor(value)?);
    }
    Ok(props)
}

fn graph_value_from_cbor(value: WireValue) -> Result<GraphValue> {
    match value {
        WireValue::Null => Ok(GraphValue::Null),
        WireValue::Bool(value) => Ok(GraphValue::Bool(value)),
        WireValue::Uint(value) => i64::try_from(value)
            .map(GraphValue::Int)
            .map_err(|_| LoomError::invalid("graph property integer exceeds i64")),
        WireValue::Nint(value) => i64::try_from(value)
            .map(|value| GraphValue::Int(-1 - value))
            .map_err(|_| LoomError::invalid("graph property integer exceeds i64")),
        WireValue::Float(value) if value.is_finite() => Ok(GraphValue::Float(value)),
        WireValue::Float(_) => Err(LoomError::invalid("graph property float must be finite")),
        WireValue::Text(value) => Ok(GraphValue::Text(value)),
        WireValue::Bytes(value) => Ok(GraphValue::Bytes(value)),
        WireValue::Array(values) if cbor_array_has_geometry_tag(&values) => {
            graph_geometry_from_cbor(values).map(GraphValue::Geometry)
        }
        WireValue::Array(values) => values
            .into_iter()
            .map(graph_value_from_cbor)
            .collect::<Result<Vec<_>>>()
            .map(GraphValue::List),
        WireValue::Map(pairs) => {
            let mut values = BTreeMap::new();
            for (key, value) in pairs {
                let WireValue::Text(key) = key else {
                    return Err(LoomError::invalid("graph map key must be text"));
                };
                values.insert(key, graph_value_from_cbor(value)?);
            }
            Ok(GraphValue::Map(values))
        }
    }
}

fn graph_value_to_cbor(value: &GraphValue) -> WireValue {
    match value {
        GraphValue::Null => WireValue::Null,
        GraphValue::Bool(value) => WireValue::Bool(*value),
        GraphValue::Int(value) => WireValue::int(*value),
        GraphValue::Float(value) => WireValue::Float(*value),
        GraphValue::Text(value) => WireValue::Text(value.clone()),
        GraphValue::Bytes(value) => WireValue::Bytes(value.clone()),
        GraphValue::List(values) => {
            WireValue::Array(values.iter().map(graph_value_to_cbor).collect())
        }
        GraphValue::Map(values) => WireValue::Map(
            values
                .iter()
                .map(|(key, value)| (WireValue::Text(key.clone()), graph_value_to_cbor(value)))
                .collect(),
        ),
        GraphValue::Geometry(value) => graph_geometry_to_cbor(value),
    }
}

fn graph_geometry_to_cbor(value: &GraphGeometry) -> WireValue {
    match value {
        GraphGeometry::Point(point) => WireValue::Array(vec![
            WireValue::Text(GRAPH_GEOMETRY_TAG.to_string()),
            WireValue::Text("point".to_string()),
            WireValue::Text(point.crs.as_str().to_string()),
            WireValue::Float(point.x),
            WireValue::Float(point.y),
            point.z.map(WireValue::Float).unwrap_or(WireValue::Null),
        ]),
    }
}

fn graph_geometry_from_cbor(values: Vec<WireValue>) -> Result<GraphGeometry> {
    let [tag, kind, crs, x, y, z]: [WireValue; 6] = values
        .try_into()
        .map_err(|_| LoomError::invalid("malformed graph geometry value"))?;
    if cbor_text(tag)? != GRAPH_GEOMETRY_TAG {
        return Err(LoomError::invalid("malformed graph geometry tag"));
    }
    match cbor_text(kind)?.as_str() {
        "point" => {
            let crs = GraphCrs::parse(&cbor_text(crs)?)?;
            let x = cbor_finite_float(x, "graph geometry x coordinate")?;
            let y = cbor_finite_float(y, "graph geometry y coordinate")?;
            let z = match z {
                WireValue::Null => None,
                other => Some(cbor_finite_float(other, "graph geometry z coordinate")?),
            };
            GraphGeometry::point(crs, x, y, z)
        }
        _ => Err(LoomError::invalid("unsupported graph geometry kind")),
    }
}

fn cbor_array_has_geometry_tag(values: &[WireValue]) -> bool {
    matches!(values.first(), Some(WireValue::Text(tag)) if tag == GRAPH_GEOMETRY_TAG)
}

fn cbor_text(value: WireValue) -> Result<String> {
    match value {
        WireValue::Text(value) => Ok(value),
        _ => Err(LoomError::invalid("graph geometry field must be text")),
    }
}

fn cbor_finite_float(value: WireValue, name: &str) -> Result<f64> {
    match value {
        WireValue::Float(value) if value.is_finite() => Ok(value),
        WireValue::Uint(value) => Ok(value as f64),
        WireValue::Nint(value) => Ok(-1.0 - value as f64),
        _ => Err(LoomError::invalid(format!("{name} must be finite"))),
    }
}

pub fn props_to_cbor(props: &Props) -> Result<Vec<u8>> {
    encode(&WireValue::Map(
        props
            .iter()
            .map(|(key, value)| (WireValue::Text(key.clone()), graph_value_to_cbor(value)))
            .collect(),
    ))
}

fn graph_edge_value(edge: &Edge) -> WireValue {
    WireValue::Array(vec![
        WireValue::Text(edge.src.clone()),
        WireValue::Text(edge.dst.clone()),
        WireValue::Text(edge.label.clone()),
        WireValue::Map(
            edge.props
                .iter()
                .map(|(key, value)| (WireValue::Text(key.clone()), graph_value_to_cbor(value)))
                .collect(),
        ),
    ])
}

pub fn graph_edge_cbor(edge: &Edge) -> Result<Vec<u8>> {
    encode(&graph_edge_value(edge))
}

pub fn graph_edges_cbor(edges: Vec<(String, Edge)>) -> Result<Vec<u8>> {
    encode(&WireValue::Array(
        edges
            .into_iter()
            .map(|(id, edge)| WireValue::Array(vec![WireValue::Text(id), graph_edge_value(&edge)]))
            .collect(),
    ))
}

pub fn graph_strings_cbor(values: Vec<String>) -> Result<Vec<u8>> {
    encode(&WireValue::Array(
        values.into_iter().map(WireValue::Text).collect(),
    ))
}

fn cmp_op(tag: u64) -> Result<CmpOp> {
    match tag {
        0 => Ok(CmpOp::Eq),
        1 => Ok(CmpOp::Ne),
        2 => Ok(CmpOp::Lt),
        3 => Ok(CmpOp::Le),
        4 => Ok(CmpOp::Gt),
        5 => Ok(CmpOp::Ge),
        other => Err(LoomError::invalid(format!(
            "unknown columnar comparison operator {other}"
        ))),
    }
}

fn columnar_aggregate_op(tag: u64) -> Result<ColumnarAggregateOp> {
    match tag {
        0 => Ok(ColumnarAggregateOp::Count),
        1 => Ok(ColumnarAggregateOp::CountNonNull),
        2 => Ok(ColumnarAggregateOp::Min),
        3 => Ok(ColumnarAggregateOp::Max),
        4 => Ok(ColumnarAggregateOp::Sum),
        other => Err(LoomError::invalid(format!(
            "unknown columnar aggregate operator {other}"
        ))),
    }
}

pub fn columnar_columns_from_cbor(bytes: &[u8]) -> Result<Vec<(String, ColumnType)>> {
    let WireValue::Array(cols) = decode(bytes)? else {
        return Err(LoomError::invalid("columnar columns must be an array"));
    };
    cols.into_iter()
        .map(|col| {
            let WireValue::Array(items) = col else {
                return Err(LoomError::invalid(
                    "columnar column must be [name, type_tag]",
                ));
            };
            let mut iter = items.into_iter();
            let name = match iter.next() {
                Some(WireValue::Text(name)) => name,
                _ => return Err(LoomError::invalid("columnar column name must be text")),
            };
            let ty = match iter.next() {
                Some(WireValue::Uint(tag)) => ColumnType::from_tag(
                    u8::try_from(tag)
                        .map_err(|_| LoomError::invalid("column type tag out of range"))?,
                )?,
                _ => return Err(LoomError::invalid("columnar column type tag must be uint")),
            };
            Ok((name, ty))
        })
        .collect()
}

pub fn columnar_columns_cbor(columns: Vec<(String, ColumnType)>) -> Result<Vec<u8>> {
    encode(&WireValue::Array(
        columns
            .into_iter()
            .map(|(name, ty)| {
                WireValue::Array(vec![
                    WireValue::Text(name),
                    WireValue::Uint(u64::from(ty.tag())),
                ])
            })
            .collect(),
    ))
}

pub fn columnar_row_from_cbor(bytes: &[u8]) -> Result<Vec<Value>> {
    let WireValue::Array(cells) = decode(bytes)? else {
        return Err(LoomError::invalid("columnar row must be an array"));
    };
    cells.into_iter().map(cell_from).collect()
}

pub fn columnar_rows_cbor(rows: Vec<Vec<Value>>) -> Result<Vec<u8>> {
    encode(&WireValue::Array(
        rows.into_iter()
            .map(|row| WireValue::Array(row.iter().map(cell_value).collect()))
            .collect(),
    ))
}

pub fn columnar_values_cbor(values: Vec<Value>) -> Result<Vec<u8>> {
    encode(&WireValue::Array(
        values.iter().map(cell_value).collect::<Vec<_>>(),
    ))
}

pub fn columnar_inspect_cbor(inspect: ColumnarInspect) -> Result<Vec<u8>> {
    encode(&WireValue::Array(vec![
        WireValue::Array(
            inspect
                .columns
                .into_iter()
                .map(|(name, ty)| {
                    WireValue::Array(vec![
                        WireValue::Text(name),
                        WireValue::Uint(u64::from(ty.tag())),
                    ])
                })
                .collect(),
        ),
        WireValue::Uint(inspect.rows as u64),
        WireValue::Uint(inspect.segment_count as u64),
        WireValue::Uint(inspect.target_segment_rows as u64),
        WireValue::Text(inspect.source_digest.to_string()),
    ]))
}

pub fn columnar_aggregates_from_cbor(bytes: &[u8]) -> Result<Vec<ColumnarAggregate>> {
    let WireValue::Array(items) = decode(bytes)? else {
        return Err(LoomError::invalid("columnar aggregates must be an array"));
    };
    items
        .into_iter()
        .map(|item| {
            let WireValue::Array(fields) = item else {
                return Err(LoomError::invalid(
                    "columnar aggregate must be [op, column?]",
                ));
            };
            let mut iter = fields.into_iter();
            let op = match iter.next() {
                Some(WireValue::Uint(tag)) => columnar_aggregate_op(tag)?,
                _ => return Err(LoomError::invalid("columnar aggregate op must be uint")),
            };
            let column = match iter.next() {
                Some(WireValue::Text(column)) => Some(column),
                Some(WireValue::Null) | None => None,
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

pub fn dataframe_batch_cbor(batch: DataframeBatch) -> Result<Vec<u8>> {
    encode(&WireValue::Array(vec![
        WireValue::Array(
            batch
                .columns
                .into_iter()
                .map(|column| {
                    WireValue::Array(vec![
                        WireValue::Text(column.name),
                        WireValue::Uint(u64::from(column.column_type.tag())),
                        WireValue::Bool(column.nullable),
                    ])
                })
                .collect(),
        ),
        WireValue::Array(
            batch
                .rows
                .into_iter()
                .map(|row| WireValue::Array(row.iter().map(cell_value).collect()))
                .collect(),
        ),
    ]))
}

pub fn digest_strings_cbor(digests: Vec<Digest>) -> Result<Vec<u8>> {
    encode(&WireValue::Array(
        digests
            .into_iter()
            .map(|digest| WireValue::Text(digest.to_string()))
            .collect(),
    ))
}

pub fn columnar_select_columns_from_cbor(bytes: &[u8]) -> Result<Vec<String>> {
    let WireValue::Array(cols) = decode(bytes)? else {
        return Err(LoomError::invalid(
            "columnar select columns must be an array",
        ));
    };
    cols.into_iter()
        .map(|col| match col {
            WireValue::Text(name) => Ok(name),
            _ => Err(LoomError::invalid("columnar select column must be text")),
        })
        .collect()
}

pub fn columnar_filter_from_cbor(bytes: &[u8]) -> Result<Option<(String, CmpOp, Value)>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let WireValue::Array(items) = decode(bytes)? else {
        return Err(LoomError::invalid("columnar filter must be an array"));
    };
    if items.is_empty() {
        return Ok(None);
    }
    let mut iter = items.into_iter();
    let column = match iter.next() {
        Some(WireValue::Text(column)) => column,
        _ => return Err(LoomError::invalid("columnar filter column must be text")),
    };
    let op = match iter.next() {
        Some(WireValue::Uint(tag)) => cmp_op(tag)?,
        _ => return Err(LoomError::invalid("columnar filter op must be uint")),
    };
    let value = iter
        .next()
        .ok_or_else(|| LoomError::invalid("columnar filter is missing value"))?;
    Ok(Some((column, op, cell_from(value)?)))
}

pub fn vector_metric(tag: i32) -> Result<Metric> {
    match tag {
        1 => Ok(Metric::Cosine),
        2 => Ok(Metric::L2),
        3 => Ok(Metric::Dot),
        other => Err(LoomError::invalid(format!("unknown vector metric {other}"))),
    }
}

pub fn vector_policy(tag: i32, threshold: usize) -> Result<AcceleratorPolicy> {
    match tag {
        0 => Ok(AcceleratorPolicy::ExactAlways),
        1 => Ok(AcceleratorPolicy::ApproximateAbove { threshold }),
        other => Err(LoomError::invalid(format!(
            "unknown vector accelerator policy {other}"
        ))),
    }
}

pub fn vector_from_bytes(bytes: &[u8]) -> Result<Vec<f32>> {
    if !bytes.len().is_multiple_of(4) {
        return Err(LoomError::invalid(
            "vector bytes length must be a multiple of 4",
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

fn vector_to_bytes(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

pub fn vector_metadata_from_cbor(
    bytes: &[u8],
) -> Result<std::collections::BTreeMap<String, Value>> {
    if bytes.is_empty() {
        return Ok(std::collections::BTreeMap::new());
    }
    let WireValue::Map(pairs) = decode(bytes)? else {
        return Err(LoomError::invalid("vector metadata must be a CBOR map"));
    };
    let mut out = std::collections::BTreeMap::new();
    for (key, value) in pairs {
        let WireValue::Text(key) = key else {
            return Err(LoomError::invalid("vector metadata key must be text"));
        };
        out.insert(key, cell_from(value)?);
    }
    Ok(out)
}

fn vector_metadata_value(metadata: &std::collections::BTreeMap<String, Value>) -> WireValue {
    WireValue::Map(
        metadata
            .iter()
            .map(|(key, value)| (WireValue::Text(key.clone()), cell_value(value)))
            .collect(),
    )
}

pub fn vector_entry_cbor(
    vector: Vec<f32>,
    metadata: std::collections::BTreeMap<String, Value>,
) -> Result<Vec<u8>> {
    encode(&WireValue::Array(vec![
        WireValue::Bytes(vector_to_bytes(&vector)),
        vector_metadata_value(&metadata),
    ]))
}

fn vector_filter_from_value(value: WireValue) -> Result<MetaFilter> {
    let WireValue::Array(items) = value else {
        return Err(LoomError::invalid("vector filter must be a CBOR array"));
    };
    let mut iter = items.into_iter();
    let tag = match iter.next() {
        Some(WireValue::Uint(tag)) => tag,
        _ => return Err(LoomError::invalid("vector filter tag must be uint")),
    };
    match tag {
        0 => Ok(MetaFilter::All),
        1 => {
            let key = match iter.next() {
                Some(WireValue::Text(key)) => key,
                _ => return Err(LoomError::invalid("vector filter Eq key must be text")),
            };
            let value = iter
                .next()
                .ok_or_else(|| LoomError::invalid("vector filter Eq is missing its value"))?;
            Ok(MetaFilter::Eq(key, cell_from(value)?))
        }
        2 => {
            let left = iter.next().ok_or_else(|| {
                LoomError::invalid("vector filter And is missing its left operand")
            })?;
            let right = iter.next().ok_or_else(|| {
                LoomError::invalid("vector filter And is missing its right operand")
            })?;
            Ok(MetaFilter::And(
                Box::new(vector_filter_from_value(left)?),
                Box::new(vector_filter_from_value(right)?),
            ))
        }
        other => Err(LoomError::invalid(format!(
            "unknown vector filter tag {other}"
        ))),
    }
}

pub fn vector_filter_from_cbor(bytes: &[u8]) -> Result<MetaFilter> {
    if bytes.is_empty() {
        return Ok(MetaFilter::All);
    }
    vector_filter_from_value(decode(bytes)?)
}

pub fn vector_strings_cbor(values: Vec<String>) -> Result<Vec<u8>> {
    encode(&WireValue::Array(
        values.into_iter().map(WireValue::Text).collect(),
    ))
}

pub fn vector_hits_cbor(hits: &[Hit]) -> Result<Vec<u8>> {
    encode(&WireValue::Array(
        hits.iter()
            .map(|hit| {
                WireValue::Array(vec![
                    WireValue::Text(hit.id.clone()),
                    cell_value(&Value::F32(hit.score)),
                ])
            })
            .collect(),
    ))
}

pub fn vector_embedding_model_cbor(model: &EmbeddingModel) -> Result<Vec<u8>> {
    encode(&WireValue::Array(vec![
        WireValue::Uint(1),
        WireValue::Text(model.model_id.clone()),
        WireValue::Uint(model.dimension as u64),
        WireValue::Text(model.weights_digest.clone().unwrap_or_default()),
    ]))
}

pub fn search_mapping_from_cbor(bytes: &[u8]) -> Result<Mapping> {
    loom_core::search_mapping_from_cbor(bytes)
}

pub fn search_document_from_cbor(bytes: &[u8]) -> Result<Document> {
    let WireValue::Map(pairs) = decode(bytes)? else {
        return Err(LoomError::invalid("search document must be a CBOR map"));
    };
    let mut doc = Document::new();
    for (key, value) in pairs {
        let WireValue::Text(field) = key else {
            return Err(LoomError::invalid(
                "search document field name must be text",
            ));
        };
        let value = match value {
            WireValue::Text(text) => FieldValue::Text(text),
            WireValue::Bytes(bytes) => FieldValue::Bytes(bytes),
            _ => {
                return Err(LoomError::invalid(
                    "search document value must be text or bytes",
                ));
            }
        };
        doc.insert(field, value);
    }
    Ok(doc)
}

pub fn search_document_cbor(doc: &Document) -> Result<Vec<u8>> {
    encode(&WireValue::Map(
        doc.iter()
            .map(|(field, value)| {
                let value = match value {
                    FieldValue::Text(text) => WireValue::Text(text.clone()),
                    FieldValue::Bytes(bytes) => WireValue::Bytes(bytes.clone()),
                };
                (WireValue::Text(field.clone()), value)
            })
            .collect(),
    ))
}

pub fn search_request_from_cbor(bytes: &[u8]) -> Result<QueryRequest> {
    loom_core::search_request_from_cbor(bytes)
}

pub fn search_response_cbor(response: &QueryResponse) -> Result<Vec<u8>> {
    Ok(loom_core::search_response_cbor(response))
}

pub fn search_ids_cbor(ids: Vec<Vec<u8>>) -> Result<Vec<u8>> {
    encode(&WireValue::Array(
        ids.into_iter().map(WireValue::Bytes).collect(),
    ))
}
