//! Canonical wire CBOR codecs for the graph facet, shared by the C ABI, the in-process client
//! service impl, and the server dispatch. Node/edge props cross as a CBOR `text -> value` map; a node
//! or edge list crosses as a CBOR array; an edge crosses as `[src, dst, label, props]`; an edge-list
//! entry is `[edge_id, edge]`.

use std::collections::BTreeMap;

use loom_codec::{Value as CborValue, decode, encode};
use loom_core::{
    Edge, GRAPH_GEOMETRY_TAG, Graph, GraphCrs, GraphEdgeDiff, GraphEdgeEndpointChange,
    GraphEdgeLabelChange, GraphGeometry, GraphIndexDiff, GraphIndexEntity, GraphIndexStatus,
    GraphMergeConflict, GraphMergeConflictEntity, GraphMergeConflictKind, GraphNodeDiff,
    GraphPropertyIndex, GraphPropertyIndexReport, GraphQueryEdge, GraphQueryExplain,
    GraphQueryIndexSelection, GraphQueryNode, GraphQueryResult, GraphQueryValue, GraphSemanticDiff,
    GraphSemanticMergeResult, GraphSpatialIndex, GraphSpatialIndexReport, GraphValue, Node, Props,
};
use loom_types::LoomError;

pub fn graph_value_to_cbor(value: &GraphValue) -> CborValue {
    match value {
        GraphValue::Null => CborValue::Null,
        GraphValue::Bool(value) => CborValue::Bool(*value),
        GraphValue::Int(value) => CborValue::int(*value),
        GraphValue::Float(value) => CborValue::Float(*value),
        GraphValue::Text(value) => CborValue::Text(value.clone()),
        GraphValue::Bytes(value) => CborValue::Bytes(value.clone()),
        GraphValue::List(values) => {
            CborValue::Array(values.iter().map(graph_value_to_cbor).collect())
        }
        GraphValue::Map(values) => CborValue::Map(
            values
                .iter()
                .map(|(key, value)| (CborValue::Text(key.clone()), graph_value_to_cbor(value)))
                .collect(),
        ),
        GraphValue::Geometry(value) => graph_geometry_to_cbor(value),
    }
}

pub fn graph_value_from_cbor(value: CborValue) -> Result<GraphValue, LoomError> {
    match value {
        CborValue::Null => Ok(GraphValue::Null),
        CborValue::Bool(value) => Ok(GraphValue::Bool(value)),
        CborValue::Uint(value) => i64::try_from(value)
            .map(GraphValue::Int)
            .map_err(|_| LoomError::invalid("graph property integer exceeds i64")),
        CborValue::Nint(value) => i64::try_from(value)
            .map(|value| GraphValue::Int(-1 - value))
            .map_err(|_| LoomError::invalid("graph property integer exceeds i64")),
        CborValue::Float(value) if value.is_finite() => Ok(GraphValue::Float(value)),
        CborValue::Float(_) => Err(LoomError::invalid("graph property float must be finite")),
        CborValue::Text(value) => Ok(GraphValue::Text(value)),
        CborValue::Bytes(value) => Ok(GraphValue::Bytes(value)),
        CborValue::Array(values) if cbor_array_has_geometry_tag(&values) => {
            graph_geometry_from_cbor(values).map(GraphValue::Geometry)
        }
        CborValue::Array(values) => values
            .into_iter()
            .map(graph_value_from_cbor)
            .collect::<Result<Vec<_>, _>>()
            .map(GraphValue::List),
        CborValue::Map(pairs) => {
            let mut values = BTreeMap::new();
            for (key, value) in pairs {
                let CborValue::Text(key) = key else {
                    return Err(LoomError::invalid("graph map key must be text"));
                };
                values.insert(key, graph_value_from_cbor(value)?);
            }
            Ok(GraphValue::Map(values))
        }
    }
}

fn graph_geometry_to_cbor(value: &GraphGeometry) -> CborValue {
    match value {
        GraphGeometry::Point(point) => CborValue::Array(vec![
            CborValue::Text(GRAPH_GEOMETRY_TAG.to_string()),
            CborValue::Text("point".to_string()),
            CborValue::Text(point.crs.as_str().to_string()),
            CborValue::Float(point.x),
            CborValue::Float(point.y),
            point.z.map(CborValue::Float).unwrap_or(CborValue::Null),
        ]),
    }
}

fn graph_geometry_from_cbor(values: Vec<CborValue>) -> Result<GraphGeometry, LoomError> {
    let [tag, kind, crs, x, y, z]: [CborValue; 6] = values
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
                CborValue::Null => None,
                other => Some(cbor_finite_float(other, "graph geometry z coordinate")?),
            };
            GraphGeometry::point(crs, x, y, z)
        }
        _ => Err(LoomError::invalid("unsupported graph geometry kind")),
    }
}

fn cbor_array_has_geometry_tag(values: &[CborValue]) -> bool {
    matches!(values.first(), Some(CborValue::Text(tag)) if tag == GRAPH_GEOMETRY_TAG)
}

fn cbor_text(value: CborValue) -> Result<String, LoomError> {
    match value {
        CborValue::Text(value) => Ok(value),
        _ => Err(LoomError::invalid("graph geometry field must be text")),
    }
}

fn cbor_finite_float(value: CborValue, name: &str) -> Result<f64, LoomError> {
    match value {
        CborValue::Float(value) if value.is_finite() => Ok(value),
        CborValue::Uint(value) => Ok(value as f64),
        CborValue::Nint(value) => Ok(-1.0 - value as f64),
        _ => Err(LoomError::invalid(format!("{name} must be finite"))),
    }
}

/// Encode a property bag as a CBOR `text -> value` map.
pub fn props_to_cbor(props: &Props) -> Vec<u8> {
    let pairs = props
        .iter()
        .map(|(k, v)| (CborValue::Text(k.clone()), graph_value_to_cbor(v)))
        .collect();
    encode(&CborValue::Map(pairs)).unwrap_or_default()
}

/// Decode a property bag from a CBOR `text -> value` map. Empty input is an empty bag.
pub fn props_from_cbor(bytes: &[u8]) -> Result<Props, LoomError> {
    if bytes.is_empty() {
        return Ok(Props::new());
    }
    let value = decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))?;
    let CborValue::Map(pairs) = value else {
        return Err(LoomError::invalid("graph props must be a CBOR map"));
    };
    let mut props = Props::new();
    for (k, v) in pairs {
        let CborValue::Text(key) = k else {
            return Err(LoomError::invalid("graph prop key must be text"));
        };
        props.insert(key, graph_value_from_cbor(v)?);
    }
    Ok(props)
}

fn edge_value(edge: &Edge) -> CborValue {
    let props = edge
        .props
        .iter()
        .map(|(k, v)| (CborValue::Text(k.clone()), graph_value_to_cbor(v)))
        .collect();
    CborValue::Array(vec![
        CborValue::Text(edge.src.clone()),
        CborValue::Text(edge.dst.clone()),
        CborValue::Text(edge.label.clone()),
        CborValue::Map(props),
    ])
}

/// Encode a single edge as `[src, dst, label, props]`.
pub fn edge_to_cbor(edge: &Edge) -> Vec<u8> {
    encode(&edge_value(edge)).unwrap_or_default()
}

/// Encode a list of node ids as a CBOR array of text.
pub fn strings_array_cbor(ids: Vec<String>) -> Vec<u8> {
    let items = ids.into_iter().map(CborValue::Text).collect();
    encode(&CborValue::Array(items)).unwrap_or_default()
}

/// Encode a list of `(edge_id, edge)` as a CBOR array of `[edge_id, edge]`.
pub fn edges_array_cbor(edges: Vec<(String, Edge)>) -> Vec<u8> {
    let items = edges
        .into_iter()
        .map(|(eid, e)| CborValue::Array(vec![CborValue::Text(eid), edge_value(&e)]))
        .collect();
    encode(&CborValue::Array(items)).unwrap_or_default()
}

fn node_value(node: &Node) -> CborValue {
    CborValue::Map(vec![
        (
            CborValue::Text("labels".to_string()),
            labels_value(&node.labels),
        ),
        (
            CborValue::Text("props".to_string()),
            props_value(&node.props),
        ),
    ])
}

fn graph_node_diff_value(diff: &GraphNodeDiff) -> CborValue {
    match diff {
        GraphNodeDiff::Added { id, node } => CborValue::Map(vec![
            (
                CborValue::Text("kind".to_string()),
                CborValue::Text("added".to_string()),
            ),
            (
                CborValue::Text("id".to_string()),
                CborValue::Text(id.clone()),
            ),
            (CborValue::Text("node".to_string()), node_value(node)),
        ]),
        GraphNodeDiff::Removed { id, node } => CborValue::Map(vec![
            (
                CborValue::Text("kind".to_string()),
                CborValue::Text("removed".to_string()),
            ),
            (
                CborValue::Text("id".to_string()),
                CborValue::Text(id.clone()),
            ),
            (CborValue::Text("node".to_string()), node_value(node)),
        ]),
        GraphNodeDiff::Updated {
            id,
            labels_added,
            labels_removed,
            props_set,
            props_removed,
        } => CborValue::Map(vec![
            (
                CborValue::Text("kind".to_string()),
                CborValue::Text("updated".to_string()),
            ),
            (
                CborValue::Text("id".to_string()),
                CborValue::Text(id.clone()),
            ),
            (
                CborValue::Text("labels_added".to_string()),
                labels_value(labels_added),
            ),
            (
                CborValue::Text("labels_removed".to_string()),
                labels_value(labels_removed),
            ),
            (
                CborValue::Text("props_set".to_string()),
                props_value(props_set),
            ),
            (
                CborValue::Text("props_removed".to_string()),
                CborValue::Array(props_removed.iter().cloned().map(CborValue::Text).collect()),
            ),
        ]),
    }
}

fn graph_edge_endpoint_change_value(change: &GraphEdgeEndpointChange) -> CborValue {
    CborValue::Map(vec![
        (
            CborValue::Text("old_src".to_string()),
            CborValue::Text(change.old_src.clone()),
        ),
        (
            CborValue::Text("old_dst".to_string()),
            CborValue::Text(change.old_dst.clone()),
        ),
        (
            CborValue::Text("new_src".to_string()),
            CborValue::Text(change.new_src.clone()),
        ),
        (
            CborValue::Text("new_dst".to_string()),
            CborValue::Text(change.new_dst.clone()),
        ),
    ])
}

fn graph_edge_label_change_value(change: &GraphEdgeLabelChange) -> CborValue {
    CborValue::Map(vec![
        (
            CborValue::Text("old_label".to_string()),
            CborValue::Text(change.old_label.clone()),
        ),
        (
            CborValue::Text("new_label".to_string()),
            CborValue::Text(change.new_label.clone()),
        ),
    ])
}

fn graph_edge_diff_value(diff: &GraphEdgeDiff) -> CborValue {
    match diff {
        GraphEdgeDiff::Added { id, edge } => CborValue::Map(vec![
            (
                CborValue::Text("kind".to_string()),
                CborValue::Text("added".to_string()),
            ),
            (
                CborValue::Text("id".to_string()),
                CborValue::Text(id.clone()),
            ),
            (CborValue::Text("edge".to_string()), edge_value(edge)),
        ]),
        GraphEdgeDiff::Removed { id, edge } => CborValue::Map(vec![
            (
                CborValue::Text("kind".to_string()),
                CborValue::Text("removed".to_string()),
            ),
            (
                CborValue::Text("id".to_string()),
                CborValue::Text(id.clone()),
            ),
            (CborValue::Text("edge".to_string()), edge_value(edge)),
        ]),
        GraphEdgeDiff::Updated {
            id,
            endpoints,
            label,
            props_set,
            props_removed,
        } => CborValue::Map(vec![
            (
                CborValue::Text("kind".to_string()),
                CborValue::Text("updated".to_string()),
            ),
            (
                CborValue::Text("id".to_string()),
                CborValue::Text(id.clone()),
            ),
            (
                CborValue::Text("endpoints".to_string()),
                endpoints
                    .as_ref()
                    .map(graph_edge_endpoint_change_value)
                    .unwrap_or(CborValue::Null),
            ),
            (
                CborValue::Text("label".to_string()),
                label
                    .as_ref()
                    .map(graph_edge_label_change_value)
                    .unwrap_or(CborValue::Null),
            ),
            (
                CborValue::Text("props_set".to_string()),
                props_value(props_set),
            ),
            (
                CborValue::Text("props_removed".to_string()),
                CborValue::Array(props_removed.iter().cloned().map(CborValue::Text).collect()),
            ),
        ]),
    }
}

fn graph_index_diff_value(diff: &GraphIndexDiff) -> CborValue {
    match diff {
        GraphIndexDiff::Added { name } => graph_index_diff_record("added", name),
        GraphIndexDiff::Removed { name } => graph_index_diff_record("removed", name),
        GraphIndexDiff::Updated { name } => graph_index_diff_record("updated", name),
    }
}

fn graph_index_diff_record(kind: &str, name: &str) -> CborValue {
    CborValue::Map(vec![
        (
            CborValue::Text("kind".to_string()),
            CborValue::Text(kind.to_string()),
        ),
        (
            CborValue::Text("name".to_string()),
            CborValue::Text(name.to_string()),
        ),
    ])
}

pub fn graph_semantic_diff_to_cbor(diff: &GraphSemanticDiff) -> Vec<u8> {
    encode(&CborValue::Map(vec![
        (
            CborValue::Text("nodes".to_string()),
            CborValue::Array(diff.nodes.iter().map(graph_node_diff_value).collect()),
        ),
        (
            CborValue::Text("edges".to_string()),
            CborValue::Array(diff.edges.iter().map(graph_edge_diff_value).collect()),
        ),
        (
            CborValue::Text("property_indexes".to_string()),
            CborValue::Array(
                diff.property_indexes
                    .iter()
                    .map(graph_index_diff_value)
                    .collect(),
            ),
        ),
        (
            CborValue::Text("spatial_indexes".to_string()),
            CborValue::Array(
                diff.spatial_indexes
                    .iter()
                    .map(graph_index_diff_value)
                    .collect(),
            ),
        ),
    ]))
    .unwrap_or_default()
}

fn labels_value(labels: &std::collections::BTreeSet<String>) -> CborValue {
    CborValue::Array(labels.iter().cloned().map(CborValue::Text).collect())
}

fn query_node_value(node: &GraphQueryNode) -> CborValue {
    CborValue::Array(vec![
        CborValue::Text("node".to_string()),
        CborValue::Text(node.id.clone()),
        labels_value(&node.labels),
        props_value(&node.props),
    ])
}

fn query_edge_value(edge: &GraphQueryEdge) -> CborValue {
    CborValue::Array(vec![
        CborValue::Text("edge".to_string()),
        CborValue::Text(edge.id.clone()),
        CborValue::Text(edge.src.clone()),
        CborValue::Text(edge.dst.clone()),
        CborValue::Text(edge.label.clone()),
        props_value(&edge.props),
    ])
}

fn props_value(props: &Props) -> CborValue {
    CborValue::Map(
        props
            .iter()
            .map(|(key, value)| (CborValue::Text(key.clone()), graph_value_to_cbor(value)))
            .collect(),
    )
}

fn graph_query_value_to_cbor(value: &GraphQueryValue) -> CborValue {
    match value {
        GraphQueryValue::Null => CborValue::Array(vec![CborValue::Text("null".to_string())]),
        GraphQueryValue::Scalar(value) => CborValue::Array(vec![
            CborValue::Text("scalar".to_string()),
            graph_value_to_cbor(value),
        ]),
        GraphQueryValue::Node(node) => query_node_value(node),
        GraphQueryValue::Edge(edge) => query_edge_value(edge),
        GraphQueryValue::Path(path) => CborValue::Array(vec![
            CborValue::Text("path".to_string()),
            CborValue::Array(path.nodes.iter().map(query_node_value).collect()),
            CborValue::Array(path.edges.iter().map(query_edge_value).collect()),
        ]),
        GraphQueryValue::List(values) => CborValue::Array(vec![
            CborValue::Text("list".to_string()),
            CborValue::Array(values.iter().map(graph_query_value_to_cbor).collect()),
        ]),
        GraphQueryValue::Map(values) => CborValue::Array(vec![
            CborValue::Text("map".to_string()),
            CborValue::Map(
                values
                    .iter()
                    .map(|(key, value)| {
                        (
                            CborValue::Text(key.clone()),
                            graph_query_value_to_cbor(value),
                        )
                    })
                    .collect(),
            ),
        ]),
    }
}

pub fn graph_query_result_to_cbor(result: &GraphQueryResult) -> Vec<u8> {
    let rows = result
        .rows
        .iter()
        .map(|row| {
            CborValue::Map(
                row.iter()
                    .map(|(key, value)| {
                        (
                            CborValue::Text(key.clone()),
                            graph_query_value_to_cbor(value),
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    encode(&CborValue::Array(rows)).unwrap_or_default()
}

fn graph_index_entity_name(entity: GraphIndexEntity) -> &'static str {
    match entity {
        GraphIndexEntity::Node => "node",
        GraphIndexEntity::Edge => "edge",
    }
}

fn graph_index_status_name(status: GraphIndexStatus) -> &'static str {
    match status {
        GraphIndexStatus::NotBuilt => "not_built",
        GraphIndexStatus::Stale => "stale",
        GraphIndexStatus::Ready => "ready",
    }
}

fn property_index_value(index: &GraphPropertyIndex) -> CborValue {
    CborValue::Map(vec![
        (
            CborValue::Text("name".to_string()),
            CborValue::Text(index.name.clone()),
        ),
        (
            CborValue::Text("entity".to_string()),
            CborValue::Text(graph_index_entity_name(index.entity).to_string()),
        ),
        (
            CborValue::Text("property".to_string()),
            CborValue::Text(index.property.clone()),
        ),
    ])
}

fn property_index_report_value(report: &GraphPropertyIndexReport) -> CborValue {
    CborValue::Map(vec![
        (
            CborValue::Text("index".to_string()),
            property_index_value(&report.index),
        ),
        (
            CborValue::Text("status".to_string()),
            CborValue::Text(graph_index_status_name(report.status).to_string()),
        ),
        (
            CborValue::Text("entries".to_string()),
            CborValue::Uint(u64::try_from(report.entries).unwrap_or(u64::MAX)),
        ),
        (
            CborValue::Text("distinct_values".to_string()),
            CborValue::Uint(u64::try_from(report.distinct_values).unwrap_or(u64::MAX)),
        ),
    ])
}

fn spatial_index_value(index: &GraphSpatialIndex) -> CborValue {
    CborValue::Map(vec![
        (
            CborValue::Text("name".to_string()),
            CborValue::Text(index.name.clone()),
        ),
        (
            CborValue::Text("entity".to_string()),
            CborValue::Text(graph_index_entity_name(index.entity).to_string()),
        ),
        (
            CborValue::Text("property".to_string()),
            CborValue::Text(index.property.clone()),
        ),
    ])
}

fn spatial_index_report_value(report: &GraphSpatialIndexReport) -> CborValue {
    CborValue::Map(vec![
        (
            CborValue::Text("index".to_string()),
            spatial_index_value(&report.index),
        ),
        (
            CborValue::Text("status".to_string()),
            CborValue::Text(graph_index_status_name(report.status).to_string()),
        ),
        (
            CborValue::Text("entries".to_string()),
            CborValue::Uint(u64::try_from(report.entries).unwrap_or(u64::MAX)),
        ),
    ])
}

fn query_index_selection_value(selection: &GraphQueryIndexSelection) -> CborValue {
    CborValue::Map(vec![
        (
            CborValue::Text("binding".to_string()),
            CborValue::Text(selection.binding.clone()),
        ),
        (
            CborValue::Text("entity".to_string()),
            CborValue::Text(graph_index_entity_name(selection.entity).to_string()),
        ),
        (
            CborValue::Text("property".to_string()),
            CborValue::Text(selection.property.clone()),
        ),
        (
            CborValue::Text("index".to_string()),
            selection
                .index
                .clone()
                .map(CborValue::Text)
                .unwrap_or(CborValue::Null),
        ),
        (
            CborValue::Text("status".to_string()),
            CborValue::Text(graph_index_status_name(selection.status).to_string()),
        ),
        (
            CborValue::Text("reason".to_string()),
            CborValue::Text(selection.reason.clone()),
        ),
    ])
}

pub fn graph_query_explain_to_cbor(explain: &GraphQueryExplain) -> Vec<u8> {
    encode(&CborValue::Map(vec![
        (
            CborValue::Text("indexes".to_string()),
            CborValue::Array(
                explain
                    .indexes
                    .iter()
                    .map(property_index_report_value)
                    .collect(),
            ),
        ),
        (
            CborValue::Text("spatial_indexes".to_string()),
            CborValue::Array(
                explain
                    .spatial_indexes
                    .iter()
                    .map(spatial_index_report_value)
                    .collect(),
            ),
        ),
        (
            CborValue::Text("selections".to_string()),
            CborValue::Array(
                explain
                    .selections
                    .iter()
                    .map(query_index_selection_value)
                    .collect(),
            ),
        ),
        (
            CborValue::Text("fallback_scan".to_string()),
            CborValue::Bool(explain.fallback_scan),
        ),
    ]))
    .unwrap_or_default()
}

fn graph_merge_conflict_entity_value(entity: &GraphMergeConflictEntity) -> CborValue {
    match entity {
        GraphMergeConflictEntity::Node(id) => graph_merge_conflict_entity_record("node", id),
        GraphMergeConflictEntity::Edge(id) => graph_merge_conflict_entity_record("edge", id),
        GraphMergeConflictEntity::PropertyIndex(name) => {
            graph_merge_conflict_entity_record("property_index", name)
        }
        GraphMergeConflictEntity::SpatialIndex(name) => {
            graph_merge_conflict_entity_record("spatial_index", name)
        }
    }
}

fn graph_merge_conflict_entity_record(kind: &str, id: &str) -> CborValue {
    CborValue::Map(vec![
        (
            CborValue::Text("kind".to_string()),
            CborValue::Text(kind.to_string()),
        ),
        (
            CborValue::Text("id".to_string()),
            CborValue::Text(id.to_string()),
        ),
    ])
}

fn graph_merge_conflict_kind_value(kind: &GraphMergeConflictKind) -> CborValue {
    match kind {
        GraphMergeConflictKind::SameNodeId => {
            graph_merge_conflict_kind_record("same_node_id", None)
        }
        GraphMergeConflictKind::SameEdgeId => {
            graph_merge_conflict_kind_record("same_edge_id", None)
        }
        GraphMergeConflictKind::EndpointDeleted => {
            graph_merge_conflict_kind_record("endpoint_deleted", None)
        }
        GraphMergeConflictKind::LabelConflict => {
            graph_merge_conflict_kind_record("label_conflict", None)
        }
        GraphMergeConflictKind::PropertyConflict(property) => {
            graph_merge_conflict_kind_record("property_conflict", Some(property))
        }
        GraphMergeConflictKind::AdjacencyChange => {
            graph_merge_conflict_kind_record("adjacency_change", None)
        }
        GraphMergeConflictKind::IndexDefinitionConflict => {
            graph_merge_conflict_kind_record("index_definition_conflict", None)
        }
    }
}

fn graph_merge_conflict_kind_record(kind: &str, property: Option<&str>) -> CborValue {
    CborValue::Map(vec![
        (
            CborValue::Text("kind".to_string()),
            CborValue::Text(kind.to_string()),
        ),
        (
            CborValue::Text("property".to_string()),
            property
                .map(|property| CborValue::Text(property.to_string()))
                .unwrap_or(CborValue::Null),
        ),
    ])
}

fn graph_merge_conflict_value(conflict: &GraphMergeConflict) -> CborValue {
    CborValue::Map(vec![
        (
            CborValue::Text("entity".to_string()),
            graph_merge_conflict_entity_value(&conflict.entity),
        ),
        (
            CborValue::Text("kind".to_string()),
            graph_merge_conflict_kind_value(&conflict.kind),
        ),
    ])
}

fn graph_merge_graph_value(graph: Option<&Graph>) -> CborValue {
    graph
        .map(|graph| CborValue::Bytes(graph.encode()))
        .unwrap_or(CborValue::Null)
}

pub fn graph_semantic_merge_result_to_cbor(result: &GraphSemanticMergeResult) -> Vec<u8> {
    encode(&CborValue::Map(vec![
        (
            CborValue::Text("graph".to_string()),
            graph_merge_graph_value(result.graph.as_ref()),
        ),
        (
            CborValue::Text("conflicts".to_string()),
            CborValue::Array(
                result
                    .conflicts
                    .iter()
                    .map(graph_merge_conflict_value)
                    .collect(),
            ),
        ),
    ]))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn props_round_trip() {
        let mut props = Props::new();
        props.insert("k".to_string(), GraphValue::Text("v".to_string()));
        props.insert("n".to_string(), GraphValue::Int(7));
        props.insert(
            "loc".to_string(),
            GraphValue::Geometry(
                GraphGeometry::point(GraphCrs::Crs84_2d, 12.5, 55.0, None).unwrap(),
            ),
        );
        let bytes = props_to_cbor(&props);
        assert_eq!(props_from_cbor(&bytes).unwrap(), props);
    }

    #[test]
    fn empty_props_bytes_decode_to_empty_bag() {
        assert_eq!(props_from_cbor(&[]).unwrap(), Props::new());
    }

    #[test]
    fn edge_encodes_as_four_element_array() {
        let edge = Edge {
            src: "a".into(),
            dst: "b".into(),
            label: "likes".into(),
            props: Props::new(),
        };
        let value = decode(&edge_to_cbor(&edge)).unwrap();
        let CborValue::Array(fields) = value else {
            panic!("expected array");
        };
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0], CborValue::Text("a".into()));
        assert_eq!(fields[1], CborValue::Text("b".into()));
        assert_eq!(fields[2], CborValue::Text("likes".into()));
    }

    #[test]
    fn graph_semantic_wire_vectors() {
        let mut base = Graph::new();
        base.upsert_node_with_labels(
            "ada",
            ["Person".to_string()].into_iter().collect(),
            BTreeMap::from([("name".to_string(), GraphValue::Text("Ada".to_string()))]),
        )
        .unwrap();
        base.upsert_node("org", Props::new()).unwrap();
        base.upsert_edge("works", "ada", "org", "WORKS_AT", Props::new())
            .unwrap();

        let mut head = base.clone();
        head.set_node_property("ada", "age", GraphValue::Int(41))
            .unwrap();
        head.set_edge_property("works", "since", GraphValue::Int(2026))
            .unwrap();
        let diff = Graph::semantic_diff(&base, &head);
        assert_eq!(
            hex(&graph_semantic_diff_to_cbor(&diff)),
            "a465656467657381a662696465776f726b73646b696e646775706461746564656c6162656cf669656e64706f696e7473f66970726f70735f736574a16573696e63651907ea6d70726f70735f72656d6f76656480656e6f64657381a662696463616461646b696e6467757064617465646970726f70735f736574a16361676518296c6c6162656c735f6164646564806d70726f70735f72656d6f766564806e6c6162656c735f72656d6f766564806f7370617469616c5f696e6465786573807070726f70657274795f696e646578657380"
        );

        let mut left = base.clone();
        left.set_node_property("ada", "age", GraphValue::Int(41))
            .unwrap();
        let mut right = base.clone();
        right
            .set_node_labels(
                "ada",
                ["Person".to_string(), "Researcher".to_string()]
                    .into_iter()
                    .collect(),
            )
            .unwrap();
        let clean = Graph::semantic_merge(&base, &left, &right).unwrap();
        assert_eq!(
            hex(&graph_semantic_merge_result_to_cbor(&clean)),
            "a2656772617068584b828283636164618266506572736f6e6a52657365617263686572a2636167651829646e616d656341646183636f726780a0818565776f726b7363616461636f726768574f524b535f4154a069636f6e666c6963747380"
        );

        let mut conflict_right = base.clone();
        conflict_right
            .set_node_property("ada", "name", GraphValue::Text("Augusta Ada".to_string()))
            .unwrap();
        let mut conflict_left = base.clone();
        conflict_left
            .set_node_property("ada", "name", GraphValue::Text("Ada Lovelace".to_string()))
            .unwrap();
        let conflict = Graph::semantic_merge(&base, &conflict_left, &conflict_right).unwrap();
        assert_eq!(
            hex(&graph_semantic_merge_result_to_cbor(&conflict)),
            "a2656772617068f669636f6e666c6963747382a2646b696e64a2646b696e647170726f70657274795f636f6e666c6963746870726f7065727479646e616d6566656e74697479a262696463616461646b696e64646e6f6465a2646b696e64a2646b696e6470656e64706f696e745f64656c657465646870726f7065727479f666656e74697479a262696465776f726b73646b696e646465646765"
        );
    }
}
