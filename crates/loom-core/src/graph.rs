//! The graph facet - a versioned property graph of nodes and edges. Pure-Rust, `wasm32`-clean,
//! deterministic. Graph storage uses a root Tree with component prolly maps, so commits, branches,
//! sync, and GC see graph structure through the object graph.
//!
//! - **Node identity:** caller-supplied stable ids are the identity; `upsert_node` merges by id
//!   (same id == same node).
//! - **Node removal:** `remove_node(id, cascade=false)` rejects with `Conflict` when incident edges
//!   exist; `cascade=true` removes the node and all incident edges. No dangling edges are ever produced.
//! - **Query surface:** the portable, deterministic traversal core is `neighbors` / `out_edges` /
//!   `in_edges`. An engine-specific `query` is layered on separately and is not part of this core.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest as _, Sha256};

use crate::AclRight;
use crate::cbor::{self, Value};
use crate::error::{Code, LoomError, Result};
use crate::object::{EntryKind, Object, TreeEntry};
use crate::provider::ObjectStore;
use crate::search::{QueryRequest, SearchEngine, search_query_auto};
use crate::vcs::{Loom, StagedEntry, normalize_path};
use crate::workspace::{FacetKind, WorkspaceId, facet_path, facet_root};

/// Property bag: ordered key -> typed value (encoded sorted by key, so it is deterministic).
pub type Props = BTreeMap<String, GraphValue>;

pub const GRAPH_GEOMETRY_TAG: &str = "loom.graph.geometry.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GraphCrs {
    Crs84_2d,
    Crs84_3d,
    Cartesian2d,
    Cartesian3d,
}

impl GraphCrs {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Crs84_2d => "crs84_2d",
            Self::Crs84_3d => "crs84_3d",
            Self::Cartesian2d => "cartesian_2d",
            Self::Cartesian3d => "cartesian_3d",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "crs84_2d" => Ok(Self::Crs84_2d),
            "crs84_3d" => Ok(Self::Crs84_3d),
            "cartesian_2d" => Ok(Self::Cartesian2d),
            "cartesian_3d" => Ok(Self::Cartesian3d),
            _ => Err(LoomError::invalid("unsupported graph geometry CRS")),
        }
    }

    fn is_geographic(self) -> bool {
        matches!(self, Self::Crs84_2d | Self::Crs84_3d)
    }

    fn is_3d(self) -> bool {
        matches!(self, Self::Crs84_3d | Self::Cartesian3d)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphPoint {
    pub crs: GraphCrs,
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
}

impl Eq for GraphPoint {}

impl GraphPoint {
    pub fn new(crs: GraphCrs, x: f64, y: f64, z: Option<f64>) -> Result<Self> {
        let point = Self { crs, x, y, z };
        point.validate()?;
        Ok(point)
    }

    fn validate(&self) -> Result<()> {
        if !self.x.is_finite() || !self.y.is_finite() || self.z.is_some_and(|z| !z.is_finite()) {
            return Err(LoomError::invalid(
                "graph geometry coordinates must be finite",
            ));
        }
        if self.crs.is_3d() != self.z.is_some() {
            return Err(LoomError::invalid(
                "graph geometry coordinate dimensionality must match CRS",
            ));
        }
        if self.crs.is_geographic()
            && !((-180.0..=180.0).contains(&self.x) && (-90.0..=90.0).contains(&self.y))
        {
            return Err(LoomError::invalid(
                "graph geographic coordinates are outside CRS84 bounds",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GraphGeometry {
    Point(GraphPoint),
}

impl Eq for GraphGeometry {}

impl GraphGeometry {
    pub fn point(crs: GraphCrs, x: f64, y: f64, z: Option<f64>) -> Result<Self> {
        GraphPoint::new(crs, x, y, z).map(Self::Point)
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Point(point) => point.validate(),
        }
    }

    fn to_cbor(&self) -> Value {
        match self {
            Self::Point(point) => Value::Array(vec![
                Value::Text(GRAPH_GEOMETRY_TAG.to_string()),
                Value::Text("point".to_string()),
                Value::Text(point.crs.as_str().to_string()),
                Value::Float(point.x),
                Value::Float(point.y),
                point.z.map(Value::Float).unwrap_or(Value::Null),
            ]),
        }
    }

    fn from_cbor_array(values: Vec<Value>) -> Result<Self> {
        let [tag, kind, crs, x, y, z]: [Value; 6] = values
            .try_into()
            .map_err(|_| LoomError::corrupt("malformed graph geometry value"))?;
        if cbor::as_text(tag)? != GRAPH_GEOMETRY_TAG {
            return Err(LoomError::corrupt("malformed graph geometry tag"));
        }
        match cbor::as_text(kind)?.as_str() {
            "point" => {
                let crs = GraphCrs::parse(&cbor::as_text(crs)?)
                    .map_err(|_| LoomError::corrupt("unsupported graph geometry CRS"))?;
                let x = cbor_float(x, "graph geometry x coordinate")?;
                let y = cbor_float(y, "graph geometry y coordinate")?;
                let z = match z {
                    Value::Null => None,
                    other => Some(cbor_float(other, "graph geometry z coordinate")?),
                };
                Self::point(crs, x, y, z)
                    .map_err(|_| LoomError::corrupt("invalid graph geometry point"))
            }
            _ => Err(LoomError::corrupt("unsupported graph geometry kind")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GraphValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    List(Vec<GraphValue>),
    Map(BTreeMap<String, GraphValue>),
    Geometry(GraphGeometry),
}

impl Eq for GraphValue {}

impl GraphValue {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Float(value) if !value.is_finite() => {
                Err(LoomError::invalid("graph property float must be finite"))
            }
            Self::List(values) => {
                if graph_value_list_uses_reserved_geometry_tag(values) {
                    return Err(LoomError::invalid(
                        "graph property list uses reserved geometry tag",
                    ));
                }
                for value in values {
                    value.validate()?;
                }
                Ok(())
            }
            Self::Map(values) => {
                for (key, value) in values {
                    validate_property_name(key)?;
                    value.validate()?;
                }
                Ok(())
            }
            Self::Geometry(value) => value.validate(),
            _ => Ok(()),
        }
    }

    fn to_cbor(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(*value),
            Self::Int(value) => Value::int(*value),
            Self::Float(value) => Value::Float(*value),
            Self::Text(value) => Value::Text(value.clone()),
            Self::Bytes(value) => Value::Bytes(value.clone()),
            Self::List(values) => Value::Array(values.iter().map(GraphValue::to_cbor).collect()),
            Self::Map(values) => Value::Map(
                values
                    .iter()
                    .map(|(key, value)| (Value::Text(key.clone()), value.to_cbor()))
                    .collect(),
            ),
            Self::Geometry(value) => value.to_cbor(),
        }
    }

    fn from_cbor(value: Value) -> Result<Self> {
        match value {
            Value::Null => Ok(Self::Null),
            Value::Bool(value) => Ok(Self::Bool(value)),
            Value::Uint(value) => i64::try_from(value)
                .map(Self::Int)
                .map_err(|_| LoomError::corrupt("graph property integer exceeds i64")),
            Value::Nint(value) => i64::try_from(value)
                .map(|value| Self::Int(-1 - value))
                .map_err(|_| LoomError::corrupt("graph property integer exceeds i64")),
            Value::Float(value) if value.is_finite() => Ok(Self::Float(value)),
            Value::Float(_) => Err(LoomError::corrupt("graph property float must be finite")),
            Value::Text(value) => Ok(Self::Text(value)),
            Value::Bytes(value) => Ok(Self::Bytes(value)),
            Value::Array(values) if cbor_array_has_geometry_tag(&values) => {
                GraphGeometry::from_cbor_array(values).map(Self::Geometry)
            }
            Value::Array(values) => values
                .into_iter()
                .map(GraphValue::from_cbor)
                .collect::<Result<Vec<_>>>()
                .map(Self::List),
            Value::Map(pairs) => {
                let mut values = BTreeMap::new();
                for (key, value) in pairs {
                    let key = cbor::as_text(key)?;
                    validate_property_name(&key)?;
                    values.insert(key, GraphValue::from_cbor(value)?);
                }
                Ok(Self::Map(values))
            }
        }
    }
}

fn cbor_float(value: Value, name: &str) -> Result<f64> {
    match value {
        Value::Float(value) if value.is_finite() => Ok(value),
        Value::Uint(value) => Ok(value as f64),
        Value::Nint(value) => Ok(-1.0 - value as f64),
        _ => Err(LoomError::corrupt(format!("{name} must be finite"))),
    }
}

fn cbor_array_has_geometry_tag(values: &[Value]) -> bool {
    matches!(values.first(), Some(Value::Text(tag)) if tag == GRAPH_GEOMETRY_TAG)
}

fn graph_value_list_uses_reserved_geometry_tag(values: &[GraphValue]) -> bool {
    matches!(values.first(), Some(GraphValue::Text(tag)) if tag == GRAPH_GEOMETRY_TAG)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Node {
    pub labels: BTreeSet<String>,
    pub props: Props,
}

impl Node {
    pub fn new(labels: BTreeSet<String>, props: Props) -> Result<Self> {
        validate_labels(&labels)?;
        validate_props(&props)?;
        Ok(Self { labels, props })
    }
}

/// A directed, labelled edge between two node ids.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Edge {
    pub src: String,
    pub dst: String,
    pub label: String,
    pub props: Props,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphQuery {
    pub patterns: Vec<GraphPattern>,
    pub predicate: GraphPredicate,
    pub returns: Vec<GraphReturn>,
    pub order_by: Vec<GraphOrder>,
    pub skip: Option<usize>,
    pub limit: Option<usize>,
}

impl GraphQuery {
    pub fn new(patterns: Vec<GraphPattern>, returns: Vec<GraphReturn>) -> Self {
        Self {
            patterns,
            predicate: GraphPredicate::All,
            returns,
            order_by: Vec::new(),
            skip: None,
            limit: None,
        }
    }

    pub fn parse_opencypher(input: &str) -> Result<Self> {
        let query = GraphQueryParser::new(input)?.parse()?;
        validate_query(&query)?;
        Ok(query)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPattern {
    pub path_variable: Option<String>,
    pub path_selection: GraphPathSelection,
    pub left: GraphNodePattern,
    pub edge: Option<GraphEdgePattern>,
    pub right: Option<GraphNodePattern>,
    pub segments: Vec<GraphPathSegment>,
}

impl GraphPattern {
    pub fn node(variable: &str) -> Self {
        Self::node_from(GraphNodePattern::new(variable))
    }

    pub fn node_from(left: GraphNodePattern) -> Self {
        Self {
            path_variable: None,
            path_selection: GraphPathSelection::AllSimple,
            left,
            edge: None,
            right: None,
            segments: Vec::new(),
        }
    }

    pub fn directed(
        left: GraphNodePattern,
        edge: GraphEdgePattern,
        right: GraphNodePattern,
    ) -> Self {
        let segment = GraphPathSegment {
            edge: edge.clone(),
            right: right.clone(),
        };
        Self {
            path_variable: None,
            path_selection: GraphPathSelection::AllSimple,
            left,
            edge: Some(edge),
            right: Some(right),
            segments: vec![segment],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphPathSelection {
    AllSimple,
    Shortest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPathSegment {
    pub edge: GraphEdgePattern,
    pub right: GraphNodePattern,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNodePattern {
    pub variable: String,
    pub labels: BTreeSet<String>,
    pub id: Option<String>,
    pub props: Props,
}

impl GraphNodePattern {
    pub fn new(variable: &str) -> Self {
        Self {
            variable: variable.to_string(),
            labels: BTreeSet::new(),
            id: None,
            props: Props::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdgePattern {
    pub variable: Option<String>,
    pub label: Option<String>,
    pub props: Props,
    pub min_hops: usize,
    pub max_hops: usize,
}

impl GraphEdgePattern {
    pub fn new(variable: &str) -> Self {
        Self {
            variable: Some(variable.to_string()),
            label: None,
            props: Props::new(),
            min_hops: 1,
            max_hops: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphPredicate {
    All,
    Eq {
        binding: String,
        property: String,
        value: GraphValue,
    },
    Ne {
        binding: String,
        property: String,
        value: GraphValue,
    },
    Gt {
        binding: String,
        property: String,
        value: GraphValue,
    },
    Gte {
        binding: String,
        property: String,
        value: GraphValue,
    },
    Lt {
        binding: String,
        property: String,
        value: GraphValue,
    },
    Lte {
        binding: String,
        property: String,
        value: GraphValue,
    },
    RegexMatch {
        binding: String,
        property: String,
        pattern: String,
    },
    HasLabel {
        binding: String,
        label: String,
    },
    FullTextMatch {
        binding: String,
        ids: BTreeSet<String>,
    },
    PointDistance {
        binding: String,
        property: String,
        point: GraphPoint,
        operator: GraphDistanceOperator,
        distance: GraphValue,
    },
    PointWithinBBox {
        binding: String,
        property: String,
        min_x: GraphValue,
        min_y: GraphValue,
        max_x: GraphValue,
        max_y: GraphValue,
    },
    And(Vec<GraphPredicate>),
    Or(Vec<GraphPredicate>),
    Not(Box<GraphPredicate>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphDistanceOperator {
    Lt,
    Lte,
    Gt,
    Gte,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphReturn {
    Binding(String),
    Property {
        binding: String,
        property: String,
    },
    Count {
        binding: Option<String>,
        alias: String,
    },
    PathLength {
        binding: String,
        alias: String,
    },
    Function {
        function: GraphFunction,
        alias: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphFunction {
    Id { binding: String },
    Type { binding: String },
    StartNode { binding: String },
    EndNode { binding: String },
    Labels { binding: String },
    Keys { binding: String },
    Properties { binding: String },
    Nodes { binding: String },
    Relationships { binding: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphOrder {
    pub item: GraphOrderItem,
    pub direction: GraphOrderDirection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphOrderItem {
    Binding(String),
    Property { binding: String, property: String },
    PathLength(String),
    Function(GraphFunction),
    ReturnKey(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphOrderDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphQueryResult {
    pub rows: Vec<BTreeMap<String, GraphQueryValue>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphMutationPlan {
    pub mutations: Vec<GraphMutation>,
}

impl GraphMutationPlan {
    pub fn new(mutations: Vec<GraphMutation>) -> Self {
        Self { mutations }
    }

    pub fn parse_opencypher(input: &str, identity: &GraphMutationIdentity) -> Result<Self> {
        GraphQueryParser::new(input)?.parse_mutation(identity)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphMutationIdentity {
    pub nodes: BTreeMap<String, String>,
    pub edges: BTreeMap<String, String>,
}

impl GraphMutationIdentity {
    pub fn new(nodes: BTreeMap<String, String>, edges: BTreeMap<String, String>) -> Self {
        Self { nodes, edges }
    }

    pub fn deterministic_opencypher(input: &str) -> Result<Self> {
        GraphQueryParser::new(input)?.parse_deterministic_mutation_identity()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphMutation {
    CreateNode {
        id: String,
        labels: BTreeSet<String>,
        props: Props,
    },
    CreateEdge {
        id: String,
        src: String,
        dst: String,
        label: String,
        props: Props,
    },
    MergeNode {
        id: String,
        labels: BTreeSet<String>,
        props: Props,
    },
    MergeEdge {
        id: String,
        src: String,
        dst: String,
        label: String,
        props: Props,
    },
    SetNodeProperty {
        id: String,
        property: String,
        value: GraphValue,
    },
    SetEdgeProperty {
        id: String,
        property: String,
        value: GraphValue,
    },
    RemoveNodeProperty {
        id: String,
        property: String,
    },
    RemoveEdgeProperty {
        id: String,
        property: String,
    },
    DeleteNode {
        id: String,
        detach: bool,
    },
    DeleteEdge {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphMutationResult {
    pub applied: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphQueryValue {
    Null,
    Scalar(GraphValue),
    Node(GraphQueryNode),
    Edge(GraphQueryEdge),
    Path(GraphPath),
    List(Vec<GraphQueryValue>),
    Map(BTreeMap<String, GraphQueryValue>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GraphIndexEntity {
    Node,
    Edge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphIndexStatus {
    NotBuilt,
    Stale,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPropertyIndex {
    pub name: String,
    pub entity: GraphIndexEntity,
    pub property: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphSpatialIndex {
    pub name: String,
    pub entity: GraphIndexEntity,
    pub property: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPropertyIndexReport {
    pub index: GraphPropertyIndex,
    pub status: GraphIndexStatus,
    pub entries: usize,
    pub distinct_values: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphSpatialIndexReport {
    pub index: GraphSpatialIndex,
    pub status: GraphIndexStatus,
    pub entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphSemanticDiff {
    pub nodes: Vec<GraphNodeDiff>,
    pub edges: Vec<GraphEdgeDiff>,
    pub property_indexes: Vec<GraphIndexDiff>,
    pub spatial_indexes: Vec<GraphIndexDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphNodeDiff {
    Added {
        id: String,
        node: Node,
    },
    Removed {
        id: String,
        node: Node,
    },
    Updated {
        id: String,
        labels_added: BTreeSet<String>,
        labels_removed: BTreeSet<String>,
        props_set: Props,
        props_removed: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphEdgeDiff {
    Added {
        id: String,
        edge: Edge,
    },
    Removed {
        id: String,
        edge: Edge,
    },
    Updated {
        id: String,
        endpoints: Option<GraphEdgeEndpointChange>,
        label: Option<GraphEdgeLabelChange>,
        props_set: Props,
        props_removed: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdgeEndpointChange {
    pub old_src: String,
    pub old_dst: String,
    pub new_src: String,
    pub new_dst: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdgeLabelChange {
    pub old_label: String,
    pub new_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphIndexDiff {
    Added { name: String },
    Removed { name: String },
    Updated { name: String },
}

#[derive(Debug, Clone)]
pub struct GraphSemanticMergeResult {
    pub graph: Option<Graph>,
    pub conflicts: Vec<GraphMergeConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphMergeConflict {
    pub entity: GraphMergeConflictEntity,
    pub kind: GraphMergeConflictKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphMergeConflictEntity {
    Node(String),
    Edge(String),
    PropertyIndex(String),
    SpatialIndex(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphMergeConflictKind {
    SameNodeId,
    SameEdgeId,
    EndpointDeleted,
    LabelConflict,
    PropertyConflict(String),
    AdjacencyChange,
    IndexDefinitionConflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphQueryIndexSelection {
    pub binding: String,
    pub entity: GraphIndexEntity,
    pub property: String,
    pub index: Option<String>,
    pub status: GraphIndexStatus,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphQueryExplain {
    pub indexes: Vec<GraphPropertyIndexReport>,
    pub spatial_indexes: Vec<GraphSpatialIndexReport>,
    pub selections: Vec<GraphQueryIndexSelection>,
    pub fallback_scan: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GraphPropertyIndexMaterialization {
    source_key: Vec<u8>,
    values: BTreeMap<Vec<u8>, BTreeSet<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct GraphBoundingBox {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Eq for GraphBoundingBox {}

impl GraphBoundingBox {
    fn point(point: &GraphPoint) -> Self {
        Self {
            min_x: point.x,
            min_y: point.y,
            max_x: point.x,
            max_y: point.y,
        }
    }

    fn intersects(&self, other: &Self) -> bool {
        self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_y <= other.max_y
            && self.max_y >= other.min_y
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GraphSpatialIndexMaterialization {
    source_key: Vec<u8>,
    boxes: BTreeMap<String, GraphBoundingBox>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GraphIndexCandidate {
    ids: Option<Vec<String>>,
    selection: GraphQueryIndexSelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphQueryNode {
    pub id: String,
    pub labels: BTreeSet<String>,
    pub props: Props,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphQueryEdge {
    pub id: String,
    pub src: String,
    pub dst: String,
    pub label: String,
    pub props: Props,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphPath {
    pub nodes: Vec<GraphQueryNode>,
    pub edges: Vec<GraphQueryEdge>,
}

#[derive(Debug, Clone)]
enum GraphBinding<'a> {
    Node(&'a str, &'a Node),
    Edge(&'a str, &'a Edge),
    Path(GraphPath),
}

#[derive(Debug, Clone, PartialEq)]
enum GraphQueryToken {
    Ident(String),
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Colon,
    Dot,
    Comma,
    Dash,
    Arrow,
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    RegexMatch,
    Star,
}

struct GraphQueryParser {
    tokens: Vec<GraphQueryToken>,
    pos: usize,
}

impl GraphQueryParser {
    fn new(input: &str) -> Result<Self> {
        Ok(Self {
            tokens: tokenize_graph_query(input)?,
            pos: 0,
        })
    }

    fn parse(&mut self) -> Result<GraphQuery> {
        self.expect_keyword("MATCH")?;
        let mut patterns = vec![self.parse_pattern()?];
        while self.consume(&GraphQueryToken::Comma) {
            patterns.push(self.parse_pattern()?);
        }
        let mut predicate = GraphPredicate::All;
        if self.consume_keyword("WHERE") {
            predicate = self.parse_predicate()?;
        }
        self.expect_keyword("RETURN")?;
        let mut returns = vec![self.parse_return()?];
        while self.consume(&GraphQueryToken::Comma) {
            returns.push(self.parse_return()?);
        }
        let mut order_by = Vec::new();
        if self.consume_keyword("ORDER") {
            self.expect_keyword("BY")?;
            order_by.push(self.parse_order()?);
            while self.consume(&GraphQueryToken::Comma) {
                order_by.push(self.parse_order()?);
            }
        }
        let skip = if self.consume_keyword("SKIP") {
            Some(self.parse_usize("SKIP", true)?)
        } else {
            None
        };
        let limit = if self.consume_keyword("LIMIT") {
            Some(self.parse_usize("LIMIT", false)?)
        } else {
            None
        };
        self.end()?;
        Ok(GraphQuery {
            patterns,
            predicate,
            returns,
            order_by,
            skip,
            limit,
        })
    }

    fn parse_mutation(&mut self, identity: &GraphMutationIdentity) -> Result<GraphMutationPlan> {
        validate_mutation_identity(identity)?;
        let plan = if self.consume_keyword("CREATE") {
            self.parse_create_mutation(identity)?
        } else if self.consume_keyword("MERGE") {
            self.parse_merge_mutation(identity)?
        } else if self.consume_keyword("SET") {
            self.parse_set_mutation(identity)?
        } else if self.consume_keyword("REMOVE") {
            self.parse_remove_mutation(identity)?
        } else if self.consume_keyword("DETACH") {
            self.expect_keyword("DELETE")?;
            self.parse_delete_mutation(identity, true)?
        } else if self.consume_keyword("DELETE") {
            self.parse_delete_mutation(identity, false)?
        } else {
            return Err(LoomError::invalid(
                "expected graph mutation keyword CREATE, MERGE, SET, REMOVE, DELETE, or DETACH DELETE",
            ));
        };
        self.end()?;
        validate_mutation_plan(&plan)?;
        Ok(plan)
    }

    fn parse_deterministic_mutation_identity(&mut self) -> Result<GraphMutationIdentity> {
        let identity = if self.consume_keyword("CREATE") || self.consume_keyword("MERGE") {
            self.parse_deterministic_identity_patterns()?
        } else {
            return Err(LoomError::unsupported(
                "deterministic graph mutation identity is supported for CREATE and MERGE patterns",
            ));
        };
        self.end()?;
        validate_mutation_identity(&identity)?;
        Ok(identity)
    }

    fn parse_deterministic_identity_patterns(&mut self) -> Result<GraphMutationIdentity> {
        let mut identity = GraphMutationIdentity::default();
        loop {
            let pattern = self.parse_pattern()?;
            append_deterministic_identity_pattern(&mut identity, &pattern)?;
            if !self.consume(&GraphQueryToken::Comma) {
                break;
            }
        }
        Ok(identity)
    }

    fn parse_create_mutation(
        &mut self,
        identity: &GraphMutationIdentity,
    ) -> Result<GraphMutationPlan> {
        let mut mutations = Vec::new();
        let mut created_nodes = BTreeSet::new();
        let mut created_edges = BTreeSet::new();
        loop {
            let pattern = self.parse_pattern()?;
            self.append_create_pattern(
                identity,
                &pattern,
                &mut created_nodes,
                &mut created_edges,
                &mut mutations,
            )?;
            if !self.consume(&GraphQueryToken::Comma) {
                break;
            }
        }
        Ok(GraphMutationPlan::new(mutations))
    }

    fn parse_merge_mutation(
        &mut self,
        identity: &GraphMutationIdentity,
    ) -> Result<GraphMutationPlan> {
        let mut mutations = Vec::new();
        let mut merged_nodes = BTreeSet::new();
        let mut merged_edges = BTreeSet::new();
        loop {
            let pattern = self.parse_pattern()?;
            self.append_merge_pattern(
                identity,
                &pattern,
                &mut merged_nodes,
                &mut merged_edges,
                &mut mutations,
            )?;
            if !self.consume(&GraphQueryToken::Comma) {
                break;
            }
        }
        Ok(GraphMutationPlan::new(mutations))
    }

    fn append_create_pattern(
        &self,
        identity: &GraphMutationIdentity,
        pattern: &GraphPattern,
        created_nodes: &mut BTreeSet<String>,
        created_edges: &mut BTreeSet<String>,
        mutations: &mut Vec<GraphMutation>,
    ) -> Result<()> {
        self.append_create_node(identity, &pattern.left, created_nodes, mutations)?;
        for segment in &pattern.segments {
            let edge = &segment.edge;
            let right = &segment.right;
            self.append_create_node(identity, right, created_nodes, mutations)?;
            let edge_variable = edge.variable.as_deref().ok_or_else(|| {
                LoomError::invalid("graph CREATE edge requires an explicit binding")
            })?;
            if edge.min_hops != 1 || edge.max_hops != 1 {
                return Err(LoomError::invalid(
                    "graph CREATE edge requires a single-hop pattern",
                ));
            }
            let edge_id = mutation_edge_id(identity, edge_variable)?;
            if created_edges.insert(edge_variable.to_string()) {
                let label = edge.label.clone().ok_or_else(|| {
                    LoomError::invalid("graph CREATE edge requires an explicit label")
                })?;
                mutations.push(GraphMutation::CreateEdge {
                    id: edge_id,
                    src: mutation_node_id(identity, &pattern.left.variable)?,
                    dst: mutation_node_id(identity, &right.variable)?,
                    label,
                    props: edge.props.clone(),
                });
            }
        }
        Ok(())
    }

    fn append_create_node(
        &self,
        identity: &GraphMutationIdentity,
        node: &GraphNodePattern,
        created_nodes: &mut BTreeSet<String>,
        mutations: &mut Vec<GraphMutation>,
    ) -> Result<()> {
        let id = mutation_node_id(identity, &node.variable)?;
        if created_nodes.insert(node.variable.clone()) {
            mutations.push(GraphMutation::CreateNode {
                id,
                labels: node.labels.clone(),
                props: node.props.clone(),
            });
        }
        Ok(())
    }

    fn append_merge_pattern(
        &self,
        identity: &GraphMutationIdentity,
        pattern: &GraphPattern,
        merged_nodes: &mut BTreeSet<String>,
        merged_edges: &mut BTreeSet<String>,
        mutations: &mut Vec<GraphMutation>,
    ) -> Result<()> {
        self.append_merge_node(identity, &pattern.left, merged_nodes, mutations)?;
        for segment in &pattern.segments {
            let edge = &segment.edge;
            let right = &segment.right;
            self.append_merge_node(identity, right, merged_nodes, mutations)?;
            let edge_variable = edge.variable.as_deref().ok_or_else(|| {
                LoomError::invalid("graph MERGE edge requires an explicit binding")
            })?;
            if edge.min_hops != 1 || edge.max_hops != 1 {
                return Err(LoomError::invalid(
                    "graph MERGE edge requires a single-hop pattern",
                ));
            }
            let edge_id = mutation_edge_id(identity, edge_variable)?;
            if merged_edges.insert(edge_variable.to_string()) {
                let label = edge.label.clone().ok_or_else(|| {
                    LoomError::invalid("graph MERGE edge requires an explicit label")
                })?;
                mutations.push(GraphMutation::MergeEdge {
                    id: edge_id,
                    src: mutation_node_id(identity, &pattern.left.variable)?,
                    dst: mutation_node_id(identity, &right.variable)?,
                    label,
                    props: edge.props.clone(),
                });
            }
        }
        Ok(())
    }

    fn append_merge_node(
        &self,
        identity: &GraphMutationIdentity,
        node: &GraphNodePattern,
        merged_nodes: &mut BTreeSet<String>,
        mutations: &mut Vec<GraphMutation>,
    ) -> Result<()> {
        let id = mutation_node_id(identity, &node.variable)?;
        if merged_nodes.insert(node.variable.clone()) {
            mutations.push(GraphMutation::MergeNode {
                id,
                labels: node.labels.clone(),
                props: node.props.clone(),
            });
        }
        Ok(())
    }

    fn parse_set_mutation(
        &mut self,
        identity: &GraphMutationIdentity,
    ) -> Result<GraphMutationPlan> {
        let mut mutations = vec![self.parse_set_item(identity)?];
        while self.consume(&GraphQueryToken::Comma) {
            mutations.push(self.parse_set_item(identity)?);
        }
        Ok(GraphMutationPlan::new(mutations))
    }

    fn parse_set_item(&mut self, identity: &GraphMutationIdentity) -> Result<GraphMutation> {
        let (binding, property) = self.parse_property_ref()?;
        self.expect(&GraphQueryToken::Eq)?;
        let value = self.parse_value()?;
        match mutation_binding_id(identity, &binding)? {
            MutationBindingIdentity::Node(id) => Ok(GraphMutation::SetNodeProperty {
                id,
                property,
                value,
            }),
            MutationBindingIdentity::Edge(id) => Ok(GraphMutation::SetEdgeProperty {
                id,
                property,
                value,
            }),
        }
    }

    fn parse_remove_mutation(
        &mut self,
        identity: &GraphMutationIdentity,
    ) -> Result<GraphMutationPlan> {
        let mut mutations = vec![self.parse_remove_item(identity)?];
        while self.consume(&GraphQueryToken::Comma) {
            mutations.push(self.parse_remove_item(identity)?);
        }
        Ok(GraphMutationPlan::new(mutations))
    }

    fn parse_remove_item(&mut self, identity: &GraphMutationIdentity) -> Result<GraphMutation> {
        let (binding, property) = self.parse_property_ref()?;
        match mutation_binding_id(identity, &binding)? {
            MutationBindingIdentity::Node(id) => {
                Ok(GraphMutation::RemoveNodeProperty { id, property })
            }
            MutationBindingIdentity::Edge(id) => {
                Ok(GraphMutation::RemoveEdgeProperty { id, property })
            }
        }
    }

    fn parse_delete_mutation(
        &mut self,
        identity: &GraphMutationIdentity,
        detach: bool,
    ) -> Result<GraphMutationPlan> {
        let mut mutations = vec![self.parse_delete_item(identity, detach)?];
        while self.consume(&GraphQueryToken::Comma) {
            mutations.push(self.parse_delete_item(identity, detach)?);
        }
        Ok(GraphMutationPlan::new(mutations))
    }

    fn parse_delete_item(
        &mut self,
        identity: &GraphMutationIdentity,
        detach: bool,
    ) -> Result<GraphMutation> {
        let binding = self.parse_ident()?;
        match mutation_binding_id(identity, &binding)? {
            MutationBindingIdentity::Node(id) => Ok(GraphMutation::DeleteNode { id, detach }),
            MutationBindingIdentity::Edge(id) => Ok(GraphMutation::DeleteEdge { id }),
        }
    }

    fn parse_property_ref(&mut self) -> Result<(String, String)> {
        let binding = self.parse_ident()?;
        self.expect(&GraphQueryToken::Dot)?;
        let property = self.parse_ident()?;
        Ok((binding, property))
    }

    fn parse_pattern(&mut self) -> Result<GraphPattern> {
        let path_variable =
            if let (Some(GraphQueryToken::Ident(binding)), Some(GraphQueryToken::Eq)) =
                (self.tokens.get(self.pos), self.tokens.get(self.pos + 1))
            {
                let binding = binding.clone();
                self.pos += 2;
                Some(binding)
            } else {
                None
            };
        let path_selection = if self.consume_keyword("shortestPath") {
            self.expect(&GraphQueryToken::LParen)?;
            GraphPathSelection::Shortest
        } else {
            GraphPathSelection::AllSimple
        };
        let left = self.parse_node_pattern()?;
        if !self.consume(&GraphQueryToken::Dash) {
            let mut pattern = GraphPattern::node_from(left);
            pattern.path_variable = path_variable;
            pattern.path_selection = path_selection;
            if path_selection == GraphPathSelection::Shortest {
                return Err(LoomError::invalid(
                    "graph shortestPath requires an edge pattern",
                ));
            }
            return Ok(pattern);
        }
        let mut segments = Vec::new();
        let edge = self.parse_edge_pattern()?;
        self.expect(&GraphQueryToken::Arrow)?;
        let right = self.parse_node_pattern()?;
        segments.push(GraphPathSegment {
            edge: edge.clone(),
            right: right.clone(),
        });
        while self.consume(&GraphQueryToken::Dash) {
            let edge = self.parse_edge_pattern()?;
            self.expect(&GraphQueryToken::Arrow)?;
            let right = self.parse_node_pattern()?;
            segments.push(GraphPathSegment { edge, right });
        }
        if path_selection == GraphPathSelection::Shortest {
            self.expect(&GraphQueryToken::RParen)?;
        }
        Ok(GraphPattern {
            path_variable,
            path_selection,
            left,
            edge: Some(edge),
            right: Some(right),
            segments,
        })
    }

    fn parse_node_pattern(&mut self) -> Result<GraphNodePattern> {
        self.expect(&GraphQueryToken::LParen)?;
        let variable = self.parse_ident()?;
        let mut pattern = GraphNodePattern::new(&variable);
        while self.consume(&GraphQueryToken::Colon) {
            pattern.labels.insert(self.parse_ident()?);
        }
        if self.consume(&GraphQueryToken::LBrace) {
            pattern.props = self.parse_props()?;
        }
        self.expect(&GraphQueryToken::RParen)?;
        Ok(pattern)
    }

    fn parse_edge_pattern(&mut self) -> Result<GraphEdgePattern> {
        self.expect(&GraphQueryToken::LBracket)?;
        let variable = match self.peek() {
            Some(GraphQueryToken::Ident(_)) => Some(self.parse_ident()?),
            _ => None,
        };
        let mut pattern = GraphEdgePattern {
            variable,
            label: None,
            props: Props::new(),
            min_hops: 1,
            max_hops: 1,
        };
        if self.consume(&GraphQueryToken::Colon) {
            pattern.label = Some(self.parse_ident()?);
        }
        if self.consume(&GraphQueryToken::LBrace) {
            pattern.props = self.parse_props()?;
        }
        if self.consume(&GraphQueryToken::Star) {
            let (min_hops, max_hops) = self.parse_path_hops()?;
            pattern.min_hops = min_hops;
            pattern.max_hops = max_hops;
        }
        self.expect(&GraphQueryToken::RBracket)?;
        Ok(pattern)
    }

    fn parse_path_hops(&mut self) -> Result<(usize, usize)> {
        let min_hops = match self.peek() {
            Some(GraphQueryToken::Int(_) | GraphQueryToken::Float(_)) => {
                self.parse_hop_count("path minimum hop count", true)?
            }
            Some(GraphQueryToken::Dot) => 1,
            _ => {
                return Err(LoomError::invalid(
                    "graph variable path requires an explicit bounded hop range",
                ));
            }
        };
        if self.consume(&GraphQueryToken::Dot) {
            let _ = self.consume(&GraphQueryToken::Dot);
            let max_hops = self.parse_hop_count("path maximum hop count", false)?;
            Ok((min_hops, max_hops))
        } else {
            Ok((min_hops, min_hops))
        }
    }

    fn parse_props(&mut self) -> Result<Props> {
        let mut props = Props::new();
        if self.consume(&GraphQueryToken::RBrace) {
            return Ok(props);
        }
        loop {
            let key = self.parse_ident()?;
            self.expect(&GraphQueryToken::Colon)?;
            props.insert(key, self.parse_value()?);
            if self.consume(&GraphQueryToken::RBrace) {
                break;
            }
            self.expect(&GraphQueryToken::Comma)?;
        }
        Ok(props)
    }

    fn parse_predicate(&mut self) -> Result<GraphPredicate> {
        self.parse_or_predicate()
    }

    fn parse_or_predicate(&mut self) -> Result<GraphPredicate> {
        let mut predicates = vec![self.parse_and_predicate()?];
        while self.consume_keyword("OR") {
            predicates.push(self.parse_and_predicate()?);
        }
        Ok(match predicates.as_slice() {
            [single] => single.clone(),
            _ => GraphPredicate::Or(predicates),
        })
    }

    fn parse_and_predicate(&mut self) -> Result<GraphPredicate> {
        let mut predicates = vec![self.parse_not_predicate()?];
        while self.consume_keyword("AND") {
            predicates.push(self.parse_not_predicate()?);
        }
        Ok(match predicates.as_slice() {
            [single] => single.clone(),
            _ => GraphPredicate::And(predicates),
        })
    }

    fn parse_not_predicate(&mut self) -> Result<GraphPredicate> {
        if self.consume_keyword("NOT") {
            Ok(GraphPredicate::Not(Box::new(self.parse_not_predicate()?)))
        } else {
            self.parse_comparison_predicate()
        }
    }

    fn parse_comparison_predicate(&mut self) -> Result<GraphPredicate> {
        let binding = self.parse_ident()?;
        if binding.eq_ignore_ascii_case("distance") && self.peek() == Some(&GraphQueryToken::LParen)
        {
            return self.parse_distance_predicate();
        }
        if binding.eq_ignore_ascii_case("within_bbox")
            && self.peek() == Some(&GraphQueryToken::LParen)
        {
            return self.parse_bbox_predicate();
        }
        self.expect(&GraphQueryToken::Dot)?;
        let property = self.parse_ident()?;
        let operator = self.next();
        let value = self.parse_value()?;
        match operator {
            Some(GraphQueryToken::Eq) => Ok(GraphPredicate::Eq {
                binding,
                property,
                value,
            }),
            Some(GraphQueryToken::Ne) => Ok(GraphPredicate::Ne {
                binding,
                property,
                value,
            }),
            Some(GraphQueryToken::Gt) => Ok(GraphPredicate::Gt {
                binding,
                property,
                value,
            }),
            Some(GraphQueryToken::Gte) => Ok(GraphPredicate::Gte {
                binding,
                property,
                value,
            }),
            Some(GraphQueryToken::Lt) => Ok(GraphPredicate::Lt {
                binding,
                property,
                value,
            }),
            Some(GraphQueryToken::Lte) => Ok(GraphPredicate::Lte {
                binding,
                property,
                value,
            }),
            Some(GraphQueryToken::RegexMatch) => match value {
                GraphValue::Text(pattern) => Ok(GraphPredicate::RegexMatch {
                    binding,
                    property,
                    pattern,
                }),
                _ => Err(LoomError::invalid(
                    "graph query regex predicate requires a string pattern",
                )),
            },
            _ => Err(LoomError::invalid(
                "expected graph query predicate operator",
            )),
        }
    }

    fn parse_distance_predicate(&mut self) -> Result<GraphPredicate> {
        self.expect(&GraphQueryToken::LParen)?;
        let (binding, property) = self.parse_property_ref()?;
        self.expect(&GraphQueryToken::Comma)?;
        let point = match self.parse_value()? {
            GraphValue::Geometry(GraphGeometry::Point(point)) => point,
            _ => {
                return Err(LoomError::invalid(
                    "graph distance predicate requires a point target",
                ));
            }
        };
        self.expect(&GraphQueryToken::RParen)?;
        let operator = match self.next() {
            Some(GraphQueryToken::Lt) => GraphDistanceOperator::Lt,
            Some(GraphQueryToken::Lte) => GraphDistanceOperator::Lte,
            Some(GraphQueryToken::Gt) => GraphDistanceOperator::Gt,
            Some(GraphQueryToken::Gte) => GraphDistanceOperator::Gte,
            _ => {
                return Err(LoomError::invalid(
                    "graph distance predicate requires <, <=, >, or >=",
                ));
            }
        };
        let distance = self.parse_value()?;
        Ok(GraphPredicate::PointDistance {
            binding,
            property,
            point,
            operator,
            distance,
        })
    }

    fn parse_bbox_predicate(&mut self) -> Result<GraphPredicate> {
        self.expect(&GraphQueryToken::LParen)?;
        let (binding, property) = self.parse_property_ref()?;
        self.expect(&GraphQueryToken::Comma)?;
        let min_x = self.parse_value()?;
        self.expect(&GraphQueryToken::Comma)?;
        let min_y = self.parse_value()?;
        self.expect(&GraphQueryToken::Comma)?;
        let max_x = self.parse_value()?;
        self.expect(&GraphQueryToken::Comma)?;
        let max_y = self.parse_value()?;
        self.expect(&GraphQueryToken::RParen)?;
        Ok(GraphPredicate::PointWithinBBox {
            binding,
            property,
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }

    fn parse_return(&mut self) -> Result<GraphReturn> {
        let binding = self.parse_ident()?;
        if binding.eq_ignore_ascii_case("count") {
            self.expect(&GraphQueryToken::LParen)?;
            let count_binding = if self.consume(&GraphQueryToken::Star) {
                None
            } else {
                Some(self.parse_ident()?)
            };
            self.expect(&GraphQueryToken::RParen)?;
            let default_alias = count_default_alias(count_binding.as_deref());
            let alias = if self.consume_keyword("AS") {
                self.parse_ident()?
            } else {
                default_alias
            };
            return Ok(GraphReturn::Count {
                binding: count_binding,
                alias,
            });
        }
        if binding.eq_ignore_ascii_case("length") {
            self.expect(&GraphQueryToken::LParen)?;
            let path_binding = self.parse_ident()?;
            self.expect(&GraphQueryToken::RParen)?;
            let default_alias = format!("length({path_binding})");
            let alias = if self.consume_keyword("AS") {
                self.parse_ident()?
            } else {
                default_alias
            };
            return Ok(GraphReturn::PathLength {
                binding: path_binding,
                alias,
            });
        }
        if let Some(function) = self.parse_graph_function_after_name(&binding)? {
            let default_alias = graph_function_alias(&function);
            let alias = if self.consume_keyword("AS") {
                self.parse_ident()?
            } else {
                default_alias
            };
            return Ok(GraphReturn::Function { function, alias });
        }
        if self.consume(&GraphQueryToken::Dot) {
            Ok(GraphReturn::Property {
                binding,
                property: self.parse_ident()?,
            })
        } else {
            Ok(GraphReturn::Binding(binding))
        }
    }

    fn parse_order(&mut self) -> Result<GraphOrder> {
        let binding = self.parse_ident()?;
        let item = if self.consume(&GraphQueryToken::Dot) {
            GraphOrderItem::Property {
                binding,
                property: self.parse_ident()?,
            }
        } else if self.peek() == Some(&GraphQueryToken::LParen) {
            if binding.eq_ignore_ascii_case("length") {
                self.expect(&GraphQueryToken::LParen)?;
                let path_binding = self.parse_ident()?;
                self.expect(&GraphQueryToken::RParen)?;
                GraphOrderItem::PathLength(path_binding)
            } else if let Some(function) = self.parse_graph_function_after_name(&binding)? {
                GraphOrderItem::Function(function)
            } else if binding.eq_ignore_ascii_case("count") {
                self.expect(&GraphQueryToken::LParen)?;
                let key = if self.consume(&GraphQueryToken::Star) {
                    "count(*)".to_string()
                } else {
                    format!("count({})", self.parse_ident()?)
                };
                self.expect(&GraphQueryToken::RParen)?;
                GraphOrderItem::ReturnKey(key)
            } else {
                return Err(LoomError::invalid("unsupported graph query order function"));
            }
        } else {
            GraphOrderItem::Binding(binding)
        };
        let direction = if self.consume_keyword("DESC") {
            GraphOrderDirection::Desc
        } else {
            let _ = self.consume_keyword("ASC");
            GraphOrderDirection::Asc
        };
        Ok(GraphOrder { item, direction })
    }

    fn parse_graph_function_after_name(&mut self, name: &str) -> Result<Option<GraphFunction>> {
        if self.peek() != Some(&GraphQueryToken::LParen) {
            return Ok(None);
        }
        let normalized = name.to_ascii_lowercase();
        match normalized.as_str() {
            "id" | "type" | "startnode" | "endnode" | "labels" | "keys" | "properties"
            | "nodes" | "relationships" => {
                self.expect(&GraphQueryToken::LParen)?;
                let binding = self.parse_ident()?;
                self.expect(&GraphQueryToken::RParen)?;
                Ok(Some(match normalized.as_str() {
                    "id" => GraphFunction::Id { binding },
                    "type" => GraphFunction::Type { binding },
                    "startnode" => GraphFunction::StartNode { binding },
                    "endnode" => GraphFunction::EndNode { binding },
                    "labels" => GraphFunction::Labels { binding },
                    "keys" => GraphFunction::Keys { binding },
                    "properties" => GraphFunction::Properties { binding },
                    "nodes" => GraphFunction::Nodes { binding },
                    "relationships" => GraphFunction::Relationships { binding },
                    _ => unreachable!(),
                }))
            }
            _ => Ok(None),
        }
    }

    fn parse_value(&mut self) -> Result<GraphValue> {
        match self.next() {
            Some(GraphQueryToken::String(value)) => Ok(GraphValue::Text(value)),
            Some(GraphQueryToken::Int(value)) => Ok(GraphValue::Int(value)),
            Some(GraphQueryToken::Float(value)) if value.is_finite() => {
                Ok(GraphValue::Float(value))
            }
            Some(GraphQueryToken::Float(_)) => {
                Err(LoomError::invalid("graph query float must be finite"))
            }
            Some(GraphQueryToken::Bool(value)) => Ok(GraphValue::Bool(value)),
            Some(GraphQueryToken::Null) => Ok(GraphValue::Null),
            Some(GraphQueryToken::LBracket) => self.parse_list_value(),
            Some(GraphQueryToken::LBrace) => self.parse_map_value(),
            Some(GraphQueryToken::Ident(value)) if value.eq_ignore_ascii_case("point") => {
                self.parse_point_value()
            }
            _ => Err(LoomError::invalid("expected graph query literal")),
        }
    }

    fn parse_point_value(&mut self) -> Result<GraphValue> {
        self.expect(&GraphQueryToken::LParen)?;
        let crs = GraphCrs::parse(&self.parse_crs_literal()?)?;
        self.expect(&GraphQueryToken::Comma)?;
        let x = self.parse_float_literal("point x coordinate")?;
        self.expect(&GraphQueryToken::Comma)?;
        let y = self.parse_float_literal("point y coordinate")?;
        let z = if self.consume(&GraphQueryToken::Comma) {
            Some(self.parse_float_literal("point z coordinate")?)
        } else {
            None
        };
        self.expect(&GraphQueryToken::RParen)?;
        GraphGeometry::point(crs, x, y, z).map(GraphValue::Geometry)
    }

    fn parse_list_value(&mut self) -> Result<GraphValue> {
        let mut values = Vec::new();
        if self.consume(&GraphQueryToken::RBracket) {
            return Ok(GraphValue::List(values));
        }
        loop {
            values.push(self.parse_value()?);
            if self.consume(&GraphQueryToken::RBracket) {
                break;
            }
            self.expect(&GraphQueryToken::Comma)?;
        }
        Ok(GraphValue::List(values))
    }

    fn parse_map_value(&mut self) -> Result<GraphValue> {
        let mut values = BTreeMap::new();
        if self.consume(&GraphQueryToken::RBrace) {
            return Ok(GraphValue::Map(values));
        }
        loop {
            let key = self.parse_map_key()?;
            self.expect(&GraphQueryToken::Colon)?;
            values.insert(key, self.parse_value()?);
            if self.consume(&GraphQueryToken::RBrace) {
                break;
            }
            self.expect(&GraphQueryToken::Comma)?;
        }
        Ok(GraphValue::Map(values))
    }

    fn parse_map_key(&mut self) -> Result<String> {
        match self.next() {
            Some(GraphQueryToken::Ident(value) | GraphQueryToken::String(value)) => {
                validate_property_name(&value)?;
                Ok(value)
            }
            _ => Err(LoomError::invalid("expected graph map key")),
        }
    }

    fn parse_crs_literal(&mut self) -> Result<String> {
        match self.next() {
            Some(GraphQueryToken::Ident(value) | GraphQueryToken::String(value)) => Ok(value),
            _ => Err(LoomError::invalid("expected graph point CRS literal")),
        }
    }

    fn parse_float_literal(&mut self, name: &str) -> Result<f64> {
        match self.next() {
            Some(GraphQueryToken::Int(value)) => Ok(value as f64),
            Some(GraphQueryToken::Float(value)) if value.is_finite() => Ok(value),
            _ => Err(LoomError::invalid(format!("expected graph query {name}"))),
        }
    }

    fn parse_usize(&mut self, name: &str, allow_zero: bool) -> Result<usize> {
        match self.next() {
            Some(GraphQueryToken::Int(value)) => usize::try_from(value)
                .ok()
                .filter(|value| allow_zero || *value > 0)
                .ok_or_else(|| LoomError::invalid(format!("graph query {name} must be positive"))),
            _ => Err(LoomError::invalid(format!(
                "expected graph query {name} integer"
            ))),
        }
    }

    fn parse_hop_count(&mut self, name: &str, allow_zero: bool) -> Result<usize> {
        match self.next() {
            Some(GraphQueryToken::Int(value)) => usize::try_from(value)
                .ok()
                .filter(|value| allow_zero || *value > 0)
                .ok_or_else(|| LoomError::invalid(format!("graph query {name} must be positive"))),
            Some(GraphQueryToken::Float(value)) if value.is_finite() && value.fract() == 0.0 => {
                let value = value as i64;
                usize::try_from(value)
                    .ok()
                    .filter(|value| allow_zero || *value > 0)
                    .ok_or_else(|| {
                        LoomError::invalid(format!("graph query {name} must be positive"))
                    })
            }
            _ => Err(LoomError::invalid(format!(
                "expected graph query {name} integer"
            ))),
        }
    }

    fn parse_ident(&mut self) -> Result<String> {
        match self.next() {
            Some(GraphQueryToken::Ident(value)) => Ok(value),
            _ => Err(LoomError::invalid("expected graph query identifier")),
        }
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<()> {
        if self.consume_keyword(keyword) {
            Ok(())
        } else {
            Err(LoomError::invalid(format!(
                "expected graph query keyword {keyword}"
            )))
        }
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        match self.peek() {
            Some(GraphQueryToken::Ident(value)) if value.eq_ignore_ascii_case(keyword) => {
                self.pos += 1;
                true
            }
            _ => false,
        }
    }

    fn expect(&mut self, token: &GraphQueryToken) -> Result<()> {
        if self.consume(token) {
            Ok(())
        } else {
            Err(LoomError::invalid("unexpected graph query token"))
        }
    }

    fn consume(&mut self, token: &GraphQueryToken) -> bool {
        if self.peek() == Some(token) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn next(&mut self) -> Option<GraphQueryToken> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn peek(&self) -> Option<&GraphQueryToken> {
        self.tokens.get(self.pos)
    }

    fn end(&self) -> Result<()> {
        if self.pos == self.tokens.len() {
            Ok(())
        } else {
            Err(LoomError::invalid("unexpected trailing graph query tokens"))
        }
    }
}

fn count_default_alias(binding: Option<&str>) -> String {
    match binding {
        Some(binding) => format!("count({binding})"),
        None => "count(*)".to_string(),
    }
}

enum MutationBindingIdentity {
    Node(String),
    Edge(String),
}

fn validate_mutation_identity(identity: &GraphMutationIdentity) -> Result<()> {
    for (binding, id) in &identity.nodes {
        validate_binding(binding)?;
        validate_graph_id(id)?;
        if identity.edges.contains_key(binding) {
            return Err(LoomError::invalid(
                "graph mutation identity binding cannot be both node and edge",
            ));
        }
    }
    for (binding, id) in &identity.edges {
        validate_binding(binding)?;
        validate_graph_id(id)?;
    }
    Ok(())
}

fn append_deterministic_identity_pattern(
    identity: &mut GraphMutationIdentity,
    pattern: &GraphPattern,
) -> Result<()> {
    let left_id = deterministic_node_identity(&pattern.left)?;
    insert_deterministic_node_identity(identity, &pattern.left.variable, left_id.clone())?;
    for segment in &pattern.segments {
        let right_id = deterministic_node_identity(&segment.right)?;
        insert_deterministic_node_identity(identity, &segment.right.variable, right_id.clone())?;
        if segment.edge.min_hops != 1 || segment.edge.max_hops != 1 {
            return Err(LoomError::invalid(
                "deterministic graph mutation identity requires single-hop edge patterns",
            ));
        }
        let edge_variable = segment.edge.variable.as_deref().ok_or_else(|| {
            LoomError::invalid("deterministic graph mutation identity requires edge bindings")
        })?;
        let edge_id = deterministic_edge_identity(&segment.edge, &left_id, &right_id)?;
        insert_deterministic_edge_identity(identity, edge_variable, edge_id)?;
    }
    Ok(())
}

fn insert_deterministic_node_identity(
    identity: &mut GraphMutationIdentity,
    binding: &str,
    id: String,
) -> Result<()> {
    match identity.nodes.get(binding) {
        Some(existing) if existing == &id => Ok(()),
        Some(_) => Err(LoomError::invalid(
            "deterministic graph mutation node identity is conflicting",
        )),
        None => {
            identity.nodes.insert(binding.to_string(), id);
            Ok(())
        }
    }
}

fn insert_deterministic_edge_identity(
    identity: &mut GraphMutationIdentity,
    binding: &str,
    id: String,
) -> Result<()> {
    match identity.edges.get(binding) {
        Some(existing) if existing == &id => Ok(()),
        Some(_) => Err(LoomError::invalid(
            "deterministic graph mutation edge identity is conflicting",
        )),
        None => {
            identity.edges.insert(binding.to_string(), id);
            Ok(())
        }
    }
}

fn deterministic_node_identity(node: &GraphNodePattern) -> Result<String> {
    if node.labels.is_empty() || node.props.is_empty() {
        return Err(LoomError::invalid(
            "deterministic graph mutation node identity requires at least one label and one property",
        ));
    }
    validate_labels(&node.labels)?;
    validate_props(&node.props)?;
    let labels = node
        .labels
        .iter()
        .cloned()
        .map(Value::Text)
        .collect::<Vec<_>>();
    let props = Value::Map(
        node.props
            .iter()
            .map(|(key, value)| (Value::Text(key.clone()), value.to_cbor()))
            .collect(),
    );
    Ok(deterministic_identity_digest(
        "neo4j/node",
        Value::Array(vec![
            Value::Text("loom.graph.neo4j.node-identity.v1".to_string()),
            Value::Array(labels),
            props,
        ]),
    ))
}

fn deterministic_edge_identity(edge: &GraphEdgePattern, src: &str, dst: &str) -> Result<String> {
    let label = edge.label.as_deref().ok_or_else(|| {
        LoomError::invalid("deterministic graph mutation edge identity requires a label")
    })?;
    validate_label(label)?;
    validate_props(&edge.props)?;
    let props = Value::Map(
        edge.props
            .iter()
            .map(|(key, value)| (Value::Text(key.clone()), value.to_cbor()))
            .collect(),
    );
    Ok(deterministic_identity_digest(
        "neo4j/edge",
        Value::Array(vec![
            Value::Text("loom.graph.neo4j.edge-identity.v1".to_string()),
            Value::Text(src.to_string()),
            Value::Text(dst.to_string()),
            Value::Text(label.to_string()),
            props,
        ]),
    ))
}

fn deterministic_identity_digest(prefix: &str, value: Value) -> String {
    let digest = Sha256::digest(cbor::encode(&value));
    format!("{prefix}/{}", hex::encode(digest))
}

fn mutation_node_id(identity: &GraphMutationIdentity, binding: &str) -> Result<String> {
    identity
        .nodes
        .get(binding)
        .cloned()
        .ok_or_else(|| LoomError::invalid("graph mutation node binding is missing identity"))
}

fn mutation_edge_id(identity: &GraphMutationIdentity, binding: &str) -> Result<String> {
    identity
        .edges
        .get(binding)
        .cloned()
        .ok_or_else(|| LoomError::invalid("graph mutation edge binding is missing identity"))
}

fn mutation_binding_id(
    identity: &GraphMutationIdentity,
    binding: &str,
) -> Result<MutationBindingIdentity> {
    match (identity.nodes.get(binding), identity.edges.get(binding)) {
        (Some(id), None) => Ok(MutationBindingIdentity::Node(id.clone())),
        (None, Some(id)) => Ok(MutationBindingIdentity::Edge(id.clone())),
        (Some(_), Some(_)) => Err(LoomError::invalid(
            "graph mutation identity binding cannot be both node and edge",
        )),
        (None, None) => Err(LoomError::invalid(
            "graph mutation binding is missing identity",
        )),
    }
}

fn tokenize_graph_query(input: &str) -> Result<Vec<GraphQueryToken>> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((_, ch)) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        match ch {
            '(' => {
                chars.next();
                tokens.push(GraphQueryToken::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(GraphQueryToken::RParen);
            }
            '[' => {
                chars.next();
                tokens.push(GraphQueryToken::LBracket);
            }
            ']' => {
                chars.next();
                tokens.push(GraphQueryToken::RBracket);
            }
            '{' => {
                chars.next();
                tokens.push(GraphQueryToken::LBrace);
            }
            '}' => {
                chars.next();
                tokens.push(GraphQueryToken::RBrace);
            }
            ':' => {
                chars.next();
                tokens.push(GraphQueryToken::Colon);
            }
            '.' => {
                chars.next();
                tokens.push(GraphQueryToken::Dot);
            }
            ',' => {
                chars.next();
                tokens.push(GraphQueryToken::Comma);
            }
            '=' => {
                chars.next();
                if chars.peek().is_some_and(|(_, next)| *next == '~') {
                    chars.next();
                    tokens.push(GraphQueryToken::RegexMatch);
                } else {
                    tokens.push(GraphQueryToken::Eq);
                }
            }
            '!' => {
                chars.next();
                if chars.peek().is_some_and(|(_, next)| *next == '=') {
                    chars.next();
                    tokens.push(GraphQueryToken::Ne);
                } else {
                    return Err(LoomError::invalid("invalid graph query operator"));
                }
            }
            '<' => {
                chars.next();
                if chars.peek().is_some_and(|(_, next)| *next == '=') {
                    chars.next();
                    tokens.push(GraphQueryToken::Lte);
                } else if chars.peek().is_some_and(|(_, next)| *next == '>') {
                    chars.next();
                    tokens.push(GraphQueryToken::Ne);
                } else {
                    tokens.push(GraphQueryToken::Lt);
                }
            }
            '>' => {
                chars.next();
                if chars.peek().is_some_and(|(_, next)| *next == '=') {
                    chars.next();
                    tokens.push(GraphQueryToken::Gte);
                } else {
                    tokens.push(GraphQueryToken::Gt);
                }
            }
            '*' => {
                chars.next();
                tokens.push(GraphQueryToken::Star);
            }
            '-' => {
                chars.next();
                if chars.peek().is_some_and(|(_, next)| *next == '>') {
                    chars.next();
                    tokens.push(GraphQueryToken::Arrow);
                } else if chars.peek().is_some_and(|(_, next)| next.is_ascii_digit()) {
                    tokens.push(read_number('-', &mut chars)?);
                } else {
                    tokens.push(GraphQueryToken::Dash);
                }
            }
            '"' | '\'' => tokens.push(GraphQueryToken::String(read_string(&mut chars)?)),
            value if value.is_ascii_digit() => tokens.push(read_number('\0', &mut chars)?),
            value if is_ident_start(value) => tokens.push(read_ident(&mut chars)),
            _ => return Err(LoomError::invalid("invalid graph query character")),
        }
    }
    Ok(tokens)
}

fn read_ident<I>(chars: &mut std::iter::Peekable<I>) -> GraphQueryToken
where
    I: Iterator<Item = (usize, char)>,
{
    let mut out = String::new();
    while let Some((_, ch)) = chars.peek().copied() {
        if is_ident_continue(ch) {
            out.push(ch);
            chars.next();
        } else {
            break;
        }
    }
    if out.eq_ignore_ascii_case("true") {
        GraphQueryToken::Bool(true)
    } else if out.eq_ignore_ascii_case("false") {
        GraphQueryToken::Bool(false)
    } else if out.eq_ignore_ascii_case("null") {
        GraphQueryToken::Null
    } else {
        GraphQueryToken::Ident(out)
    }
}

fn read_string<I>(chars: &mut std::iter::Peekable<I>) -> Result<String>
where
    I: Iterator<Item = (usize, char)>,
{
    let Some((_, quote)) = chars.next() else {
        return Err(LoomError::invalid("expected graph query string"));
    };
    let mut out = String::new();
    while let Some((_, ch)) = chars.next() {
        if ch == quote {
            return Ok(out);
        }
        if ch == '\\' {
            let Some((_, escaped)) = chars.next() else {
                return Err(LoomError::invalid("unterminated graph query escape"));
            };
            match escaped {
                '"' => out.push('"'),
                '\'' => out.push('\''),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                _ => return Err(LoomError::invalid("unsupported graph query escape")),
            }
        } else {
            out.push(ch);
        }
    }
    Err(LoomError::invalid("unterminated graph query string"))
}

fn read_number<I>(sign: char, chars: &mut std::iter::Peekable<I>) -> Result<GraphQueryToken>
where
    I: Iterator<Item = (usize, char)>,
{
    let mut out = String::new();
    if sign == '-' {
        out.push('-');
    }
    let mut has_dot = false;
    while let Some((_, ch)) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            out.push(ch);
            chars.next();
        } else if ch == '.' && !has_dot {
            has_dot = true;
            out.push(ch);
            chars.next();
        } else {
            break;
        }
    }
    if out == "-" || out == "." || out == "-." {
        return Err(LoomError::invalid("invalid graph query number"));
    }
    if has_dot {
        out.parse::<f64>()
            .map(GraphQueryToken::Float)
            .map_err(|_| LoomError::invalid("invalid graph query float"))
    } else {
        out.parse::<i64>()
            .map(GraphQueryToken::Int)
            .map_err(|_| LoomError::invalid("invalid graph query integer"))
    }
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

/// A versioned property graph: nodes keyed by caller-supplied id, edges keyed by caller-supplied id.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    nodes: BTreeMap<String, Node>,
    edges: BTreeMap<String, Edge>,
    property_indexes: BTreeMap<String, GraphPropertyIndex>,
    property_index_materializations: BTreeMap<String, GraphPropertyIndexMaterialization>,
    spatial_indexes: BTreeMap<String, GraphSpatialIndex>,
    spatial_index_materializations: BTreeMap<String, GraphSpatialIndexMaterialization>,
}

impl Graph {
    /// An empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn semantic_diff(base: &Self, head: &Self) -> GraphSemanticDiff {
        GraphSemanticDiff {
            nodes: diff_nodes(&base.nodes, &head.nodes),
            edges: diff_edges(&base.edges, &head.edges),
            property_indexes: diff_indexes(&base.property_indexes, &head.property_indexes),
            spatial_indexes: diff_indexes(&base.spatial_indexes, &head.spatial_indexes),
        }
    }

    pub fn semantic_merge(
        base: &Self,
        left: &Self,
        right: &Self,
    ) -> Result<GraphSemanticMergeResult> {
        let mut merged = Graph::new();
        let mut conflicts = Vec::new();

        let mut node_ids = BTreeSet::new();
        node_ids.extend(base.nodes.keys().cloned());
        node_ids.extend(left.nodes.keys().cloned());
        node_ids.extend(right.nodes.keys().cloned());
        for id in node_ids {
            if let Some(node) = merge_node(
                &id,
                base.nodes.get(&id),
                left.nodes.get(&id),
                right.nodes.get(&id),
                &mut conflicts,
            )? {
                merged.nodes.insert(id, node);
            }
        }

        let mut edge_ids = BTreeSet::new();
        edge_ids.extend(base.edges.keys().cloned());
        edge_ids.extend(left.edges.keys().cloned());
        edge_ids.extend(right.edges.keys().cloned());
        for id in edge_ids {
            if let Some(edge) = merge_edge(
                &id,
                base.edges.get(&id),
                left.edges.get(&id),
                right.edges.get(&id),
                &mut conflicts,
            )? {
                merged.edges.insert(id, edge);
            }
        }

        for (id, edge) in &base.edges {
            if !merged.edges.contains_key(id)
                && (left.edges.contains_key(id) || right.edges.contains_key(id))
                && (!merged.nodes.contains_key(&edge.src) || !merged.nodes.contains_key(&edge.dst))
            {
                conflicts.push(GraphMergeConflict {
                    entity: GraphMergeConflictEntity::Edge(id.clone()),
                    kind: GraphMergeConflictKind::EndpointDeleted,
                });
            }
        }

        merge_index_catalog(
            &base.property_indexes,
            &left.property_indexes,
            &right.property_indexes,
            |name| GraphMergeConflictEntity::PropertyIndex(name.to_string()),
            &mut merged.property_indexes,
            &mut conflicts,
        );
        merge_index_catalog(
            &base.spatial_indexes,
            &left.spatial_indexes,
            &right.spatial_indexes,
            |name| GraphMergeConflictEntity::SpatialIndex(name.to_string()),
            &mut merged.spatial_indexes,
            &mut conflicts,
        );

        for (id, edge) in &merged.edges {
            if !merged.nodes.contains_key(&edge.src) || !merged.nodes.contains_key(&edge.dst) {
                conflicts.push(GraphMergeConflict {
                    entity: GraphMergeConflictEntity::Edge(id.clone()),
                    kind: GraphMergeConflictKind::EndpointDeleted,
                });
            }
        }

        if conflicts.is_empty() {
            Ok(GraphSemanticMergeResult {
                graph: Some(merged),
                conflicts,
            })
        } else {
            Ok(GraphSemanticMergeResult {
                graph: None,
                conflicts,
            })
        }
    }

    pub fn declare_property_index(
        &mut self,
        name: &str,
        entity: GraphIndexEntity,
        property: &str,
    ) -> Result<()> {
        validate_graph_id(name)?;
        validate_property_name(property)?;
        let index = GraphPropertyIndex {
            name: name.to_string(),
            entity,
            property: property.to_string(),
        };
        match self.property_indexes.get(name) {
            Some(existing) if existing != &index => {
                return Err(LoomError::new(
                    Code::Conflict,
                    format!("graph property index {name:?} already has a different definition"),
                ));
            }
            Some(_) => {}
            None => {
                self.property_indexes.insert(name.to_string(), index);
            }
        }
        self.property_index_materializations.remove(name);
        Ok(())
    }

    pub fn drop_property_index(&mut self, name: &str) -> Result<bool> {
        validate_graph_id(name)?;
        self.property_index_materializations.remove(name);
        Ok(self.property_indexes.remove(name).is_some())
    }

    pub fn property_indexes(&self) -> Vec<GraphPropertyIndex> {
        self.property_indexes.values().cloned().collect()
    }

    pub fn rebuild_property_index(&mut self, name: &str) -> Result<GraphPropertyIndexReport> {
        let index = self
            .property_indexes
            .get(name)
            .cloned()
            .ok_or_else(|| LoomError::not_found(format!("graph property index {name:?}")))?;
        let materialization = self.materialize_property_index(&index)?;
        self.property_index_materializations
            .insert(name.to_string(), materialization);
        self.property_index_report(&index)
            .ok_or_else(|| LoomError::corrupt("rebuilt graph property index is missing"))
    }

    pub fn rebuild_property_indexes(&mut self) -> Result<Vec<GraphPropertyIndexReport>> {
        let names = self.property_indexes.keys().cloned().collect::<Vec<_>>();
        let mut reports = Vec::new();
        for name in names {
            reports.push(self.rebuild_property_index(&name)?);
        }
        Ok(reports)
    }

    pub fn property_index_reports(&self) -> Vec<GraphPropertyIndexReport> {
        self.property_indexes
            .values()
            .filter_map(|index| self.property_index_report(index))
            .collect()
    }

    pub fn declare_spatial_index(
        &mut self,
        name: &str,
        entity: GraphIndexEntity,
        property: &str,
    ) -> Result<()> {
        validate_graph_id(name)?;
        validate_property_name(property)?;
        let index = GraphSpatialIndex {
            name: name.to_string(),
            entity,
            property: property.to_string(),
        };
        match self.spatial_indexes.get(name) {
            Some(existing) if existing != &index => {
                return Err(LoomError::new(
                    Code::Conflict,
                    format!("graph spatial index {name:?} already has a different definition"),
                ));
            }
            Some(_) => {}
            None => {
                self.spatial_indexes.insert(name.to_string(), index);
            }
        }
        self.spatial_index_materializations.remove(name);
        Ok(())
    }

    pub fn drop_spatial_index(&mut self, name: &str) -> Result<bool> {
        validate_graph_id(name)?;
        self.spatial_index_materializations.remove(name);
        Ok(self.spatial_indexes.remove(name).is_some())
    }

    pub fn spatial_indexes(&self) -> Vec<GraphSpatialIndex> {
        self.spatial_indexes.values().cloned().collect()
    }

    pub fn rebuild_spatial_index(&mut self, name: &str) -> Result<GraphSpatialIndexReport> {
        let index = self
            .spatial_indexes
            .get(name)
            .cloned()
            .ok_or_else(|| LoomError::not_found(format!("graph spatial index {name:?}")))?;
        let materialization = self.materialize_spatial_index(&index)?;
        self.spatial_index_materializations
            .insert(name.to_string(), materialization);
        self.spatial_index_report(&index)
            .ok_or_else(|| LoomError::corrupt("rebuilt graph spatial index is missing"))
    }

    pub fn rebuild_spatial_indexes(&mut self) -> Result<Vec<GraphSpatialIndexReport>> {
        let names = self.spatial_indexes.keys().cloned().collect::<Vec<_>>();
        let mut reports = Vec::new();
        for name in names {
            reports.push(self.rebuild_spatial_index(&name)?);
        }
        Ok(reports)
    }

    pub fn spatial_index_reports(&self) -> Vec<GraphSpatialIndexReport> {
        self.spatial_indexes
            .values()
            .filter_map(|index| self.spatial_index_report(index))
            .collect()
    }

    /// Insert or replace the node `id` with `props` (upsert merges by id - same id is the same node).
    pub fn upsert_node(&mut self, id: &str, props: Props) -> Result<()> {
        let labels = self
            .nodes
            .get(id)
            .map(|node| node.labels.clone())
            .unwrap_or_default();
        self.upsert_node_with_labels(id, labels, props)
    }

    pub fn upsert_node_with_labels(
        &mut self,
        id: &str,
        labels: BTreeSet<String>,
        props: Props,
    ) -> Result<()> {
        validate_graph_id(id)?;
        self.nodes.insert(id.to_string(), Node::new(labels, props)?);
        Ok(())
    }

    pub fn create_node_with_labels(
        &mut self,
        id: &str,
        labels: BTreeSet<String>,
        props: Props,
    ) -> Result<()> {
        validate_graph_id(id)?;
        if self.nodes.contains_key(id) {
            return Err(LoomError::new(
                Code::Conflict,
                format!("node {id:?} already exists"),
            ));
        }
        self.nodes.insert(id.to_string(), Node::new(labels, props)?);
        Ok(())
    }

    pub fn merge_node_with_labels(
        &mut self,
        id: &str,
        labels: BTreeSet<String>,
        props: Props,
    ) -> Result<()> {
        validate_graph_id(id)?;
        validate_labels(&labels)?;
        validate_props(&props)?;
        if let Some(node) = self.nodes.get(id) {
            if !labels.iter().all(|label| node.labels.contains(label))
                || !props_match(&node.props, &props)
            {
                return Err(LoomError::new(
                    Code::Conflict,
                    format!("node {id:?} does not satisfy MERGE pattern"),
                ));
            }
            return Ok(());
        }
        self.nodes.insert(id.to_string(), Node::new(labels, props)?);
        Ok(())
    }

    /// The node `id`'s properties, or `None`.
    pub fn node(&self, id: &str) -> Option<&Props> {
        self.nodes.get(id).map(|node| &node.props)
    }

    pub fn graph_node(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn node_labels(&self, id: &str) -> Option<&BTreeSet<String>> {
        self.nodes.get(id).map(|node| &node.labels)
    }

    pub fn set_node_labels(&mut self, id: &str, labels: BTreeSet<String>) -> Result<()> {
        validate_labels(&labels)?;
        let node = self
            .nodes
            .get_mut(id)
            .ok_or_else(|| LoomError::not_found(format!("node {id:?}")))?;
        node.labels = labels;
        Ok(())
    }

    pub fn set_node_property(&mut self, id: &str, property: &str, value: GraphValue) -> Result<()> {
        validate_property_name(property)?;
        value.validate()?;
        let node = self
            .nodes
            .get_mut(id)
            .ok_or_else(|| LoomError::not_found(format!("node {id:?}")))?;
        node.props.insert(property.to_string(), value);
        Ok(())
    }

    pub fn remove_node_property(&mut self, id: &str, property: &str) -> Result<bool> {
        validate_property_name(property)?;
        let node = self
            .nodes
            .get_mut(id)
            .ok_or_else(|| LoomError::not_found(format!("node {id:?}")))?;
        Ok(node.props.remove(property).is_some())
    }

    /// Number of nodes / edges.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Remove node `id`. With `cascade=false`, rejects (`Conflict`) if any edge is incident to it
    /// (referential integrity); with `cascade=true`, removes the node and every incident edge.
    pub fn remove_node(&mut self, id: &str, cascade: bool) -> Result<()> {
        if !self.nodes.contains_key(id) {
            return Err(LoomError::not_found(format!("node {id:?}")));
        }
        let incident: Vec<String> = self
            .edges
            .iter()
            .filter(|(_, e)| e.src == id || e.dst == id)
            .map(|(eid, _)| eid.clone())
            .collect();
        if !incident.is_empty() && !cascade {
            return Err(LoomError::new(
                Code::Conflict,
                format!(
                    "node {id:?} has {} incident edge(s); pass cascade",
                    incident.len()
                ),
            ));
        }
        for eid in incident {
            self.edges.remove(&eid);
        }
        self.nodes.remove(id);
        Ok(())
    }

    /// Insert or replace edge `id` from `src` to `dst`. Both endpoints MUST already exist (a missing
    /// endpoint is `NOT_FOUND`), so the graph never holds an edge with a dangling endpoint.
    pub fn upsert_edge(
        &mut self,
        id: &str,
        src: &str,
        dst: &str,
        label: &str,
        props: Props,
    ) -> Result<()> {
        validate_props(&props)?;
        if !self.nodes.contains_key(src) {
            return Err(LoomError::not_found(format!("edge src node {src:?}")));
        }
        if !self.nodes.contains_key(dst) {
            return Err(LoomError::not_found(format!("edge dst node {dst:?}")));
        }
        self.edges.insert(
            id.to_string(),
            Edge {
                src: src.to_string(),
                dst: dst.to_string(),
                label: label.to_string(),
                props,
            },
        );
        Ok(())
    }

    pub fn create_edge(
        &mut self,
        id: &str,
        src: &str,
        dst: &str,
        label: &str,
        props: Props,
    ) -> Result<()> {
        validate_graph_id(id)?;
        if self.edges.contains_key(id) {
            return Err(LoomError::new(
                Code::Conflict,
                format!("edge {id:?} already exists"),
            ));
        }
        self.upsert_edge(id, src, dst, label, props)
    }

    pub fn merge_edge(
        &mut self,
        id: &str,
        src: &str,
        dst: &str,
        label: &str,
        props: Props,
    ) -> Result<()> {
        validate_graph_id(id)?;
        validate_graph_id(src)?;
        validate_graph_id(dst)?;
        validate_label(label)?;
        validate_props(&props)?;
        if let Some(edge) = self.edges.get(id) {
            if edge.src != src
                || edge.dst != dst
                || edge.label != label
                || !props_match(&edge.props, &props)
            {
                return Err(LoomError::new(
                    Code::Conflict,
                    format!("edge {id:?} does not satisfy MERGE pattern"),
                ));
            }
            return Ok(());
        }
        self.upsert_edge(id, src, dst, label, props)
    }

    pub fn set_edge_property(&mut self, id: &str, property: &str, value: GraphValue) -> Result<()> {
        validate_property_name(property)?;
        value.validate()?;
        let edge = self
            .edges
            .get_mut(id)
            .ok_or_else(|| LoomError::not_found(format!("edge {id:?}")))?;
        edge.props.insert(property.to_string(), value);
        Ok(())
    }

    pub fn remove_edge_property(&mut self, id: &str, property: &str) -> Result<bool> {
        validate_property_name(property)?;
        let edge = self
            .edges
            .get_mut(id)
            .ok_or_else(|| LoomError::not_found(format!("edge {id:?}")))?;
        Ok(edge.props.remove(property).is_some())
    }

    /// The edge `id`, or `None`.
    pub fn edge(&self, id: &str) -> Option<&Edge> {
        self.edges.get(id)
    }

    /// All edges as `(edge_id, &Edge)`, in edge-id order.
    pub fn edges(&self) -> Vec<(&str, &Edge)> {
        self.edges
            .iter()
            .map(|(eid, edge)| (eid.as_str(), edge))
            .collect()
    }

    /// Remove edge `id` (no-op if absent).
    pub fn remove_edge(&mut self, id: &str) {
        self.edges.remove(id);
    }

    /// Portable traversal: edges whose `src` is `id`, as `(edge_id, &Edge)`, in edge-id order.
    pub fn out_edges(&self, id: &str) -> Vec<(&str, &Edge)> {
        self.edges
            .iter()
            .filter(|(_, e)| e.src == id)
            .map(|(eid, e)| (eid.as_str(), e))
            .collect()
    }

    /// Portable traversal: edges whose `dst` is `id`, as `(edge_id, &Edge)`, in edge-id order.
    pub fn in_edges(&self, id: &str) -> Vec<(&str, &Edge)> {
        self.edges
            .iter()
            .filter(|(_, e)| e.dst == id)
            .map(|(eid, e)| (eid.as_str(), e))
            .collect()
    }

    /// Portable traversal: the distinct adjacent node ids (out- and in-neighbours), sorted.
    pub fn neighbors(&self, id: &str) -> Vec<String> {
        let mut out: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for e in self.edges.values() {
            if e.src == id {
                out.insert(e.dst.clone());
            }
            if e.dst == id {
                out.insert(e.src.clone());
            }
        }
        out.into_iter().collect()
    }

    /// Canonical bytes (nodes then edges, each in id order; props sorted by key). Deterministic.
    pub fn encode(&self) -> Vec<u8> {
        let nodes = self
            .nodes
            .iter()
            .map(|(id, node)| {
                Value::Array(vec![
                    Value::Text(id.clone()),
                    labels_value(&node.labels),
                    props_value(&node.props),
                ])
            })
            .collect();
        let edges = self
            .edges
            .iter()
            .map(|(id, e)| {
                Value::Array(vec![
                    Value::Text(id.clone()),
                    Value::Text(e.src.clone()),
                    Value::Text(e.dst.clone()),
                    Value::Text(e.label.clone()),
                    props_value(&e.props),
                ])
            })
            .collect();
        cbor::encode(&Value::Array(vec![
            Value::Array(nodes),
            Value::Array(edges),
        ]))
    }

    /// Parse a graph from [`Graph::encode`] output.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut top = cbor::Fields::new(cbor::decode_array(bytes)?);
        let nodes = top.array()?;
        let edges = top.array()?;
        top.end()?;
        let mut g = Graph::new();
        for item in nodes {
            let mut item = cbor::as_array(item)?;
            if item.len() == 2 {
                let props = props_from(cbor::as_map(item.pop().unwrap())?)?;
                let id = cbor::as_text(item.pop().unwrap())?;
                g.nodes.insert(id, Node::new(BTreeSet::new(), props)?);
            } else if item.len() == 3 {
                let props = props_from(cbor::as_map(item.pop().unwrap())?)?;
                let labels = labels_from(cbor::as_array(item.pop().unwrap())?)?;
                let id = cbor::as_text(item.pop().unwrap())?;
                g.nodes.insert(id, Node::new(labels, props)?);
            } else {
                return Err(LoomError::corrupt("invalid graph node encoding"));
            }
        }
        for item in edges {
            let mut f = cbor::Fields::new(cbor::as_array(item)?);
            let id = f.text()?;
            let src = f.text()?;
            let dst = f.text()?;
            let label = f.text()?;
            let props = props_from(f.map()?)?;
            f.end()?;
            g.edges.insert(
                id,
                Edge {
                    src,
                    dst,
                    label,
                    props,
                },
            );
        }
        Ok(g)
    }
}

fn diff_nodes(base: &BTreeMap<String, Node>, head: &BTreeMap<String, Node>) -> Vec<GraphNodeDiff> {
    let mut ids = BTreeSet::new();
    ids.extend(base.keys().cloned());
    ids.extend(head.keys().cloned());
    ids.into_iter()
        .filter_map(|id| match (base.get(&id), head.get(&id)) {
            (None, Some(node)) => Some(GraphNodeDiff::Added {
                id,
                node: node.clone(),
            }),
            (Some(node), None) => Some(GraphNodeDiff::Removed {
                id,
                node: node.clone(),
            }),
            (Some(before), Some(after)) if before != after => Some(GraphNodeDiff::Updated {
                id,
                labels_added: after
                    .labels
                    .difference(&before.labels)
                    .cloned()
                    .collect::<BTreeSet<_>>(),
                labels_removed: before
                    .labels
                    .difference(&after.labels)
                    .cloned()
                    .collect::<BTreeSet<_>>(),
                props_set: props_set_diff(&before.props, &after.props),
                props_removed: props_removed_diff(&before.props, &after.props),
            }),
            _ => None,
        })
        .collect()
}

fn diff_edges(base: &BTreeMap<String, Edge>, head: &BTreeMap<String, Edge>) -> Vec<GraphEdgeDiff> {
    let mut ids = BTreeSet::new();
    ids.extend(base.keys().cloned());
    ids.extend(head.keys().cloned());
    ids.into_iter()
        .filter_map(|id| match (base.get(&id), head.get(&id)) {
            (None, Some(edge)) => Some(GraphEdgeDiff::Added {
                id,
                edge: edge.clone(),
            }),
            (Some(edge), None) => Some(GraphEdgeDiff::Removed {
                id,
                edge: edge.clone(),
            }),
            (Some(before), Some(after)) if before != after => Some(GraphEdgeDiff::Updated {
                id,
                endpoints: (before.src != after.src || before.dst != after.dst).then(|| {
                    GraphEdgeEndpointChange {
                        old_src: before.src.clone(),
                        old_dst: before.dst.clone(),
                        new_src: after.src.clone(),
                        new_dst: after.dst.clone(),
                    }
                }),
                label: (before.label != after.label).then(|| GraphEdgeLabelChange {
                    old_label: before.label.clone(),
                    new_label: after.label.clone(),
                }),
                props_set: props_set_diff(&before.props, &after.props),
                props_removed: props_removed_diff(&before.props, &after.props),
            }),
            _ => None,
        })
        .collect()
}

fn diff_indexes<T: PartialEq>(
    base: &BTreeMap<String, T>,
    head: &BTreeMap<String, T>,
) -> Vec<GraphIndexDiff> {
    let mut names = BTreeSet::new();
    names.extend(base.keys().cloned());
    names.extend(head.keys().cloned());
    names
        .into_iter()
        .filter_map(|name| match (base.get(&name), head.get(&name)) {
            (None, Some(_)) => Some(GraphIndexDiff::Added { name }),
            (Some(_), None) => Some(GraphIndexDiff::Removed { name }),
            (Some(before), Some(after)) if before != after => {
                Some(GraphIndexDiff::Updated { name })
            }
            _ => None,
        })
        .collect()
}

fn props_set_diff(base: &Props, head: &Props) -> Props {
    head.iter()
        .filter(|(name, value)| base.get(*name) != Some(*value))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

fn props_removed_diff(base: &Props, head: &Props) -> Vec<String> {
    base.keys()
        .filter(|name| !head.contains_key(*name))
        .cloned()
        .collect()
}

fn merge_node(
    id: &str,
    base: Option<&Node>,
    left: Option<&Node>,
    right: Option<&Node>,
    conflicts: &mut Vec<GraphMergeConflict>,
) -> Result<Option<Node>> {
    if left == right {
        return Ok(left.cloned());
    }
    if left == base {
        return Ok(right.cloned());
    }
    if right == base {
        return Ok(left.cloned());
    }
    let (Some(base), Some(left), Some(right)) = (base, left, right) else {
        conflicts.push(GraphMergeConflict {
            entity: GraphMergeConflictEntity::Node(id.to_string()),
            kind: GraphMergeConflictKind::SameNodeId,
        });
        return Ok(None);
    };

    let labels = merge_labels(id, &base.labels, &left.labels, &right.labels, conflicts);
    let props = merge_props(
        &GraphMergeConflictEntity::Node(id.to_string()),
        &base.props,
        &left.props,
        &right.props,
        conflicts,
    );
    if conflicts.iter().any(|conflict| {
        matches!(&conflict.entity, GraphMergeConflictEntity::Node(conflict_id) if conflict_id == id)
    }) {
        return Ok(None);
    }
    Ok(Some(Node::new(labels, props)?))
}

fn merge_edge(
    id: &str,
    base: Option<&Edge>,
    left: Option<&Edge>,
    right: Option<&Edge>,
    conflicts: &mut Vec<GraphMergeConflict>,
) -> Result<Option<Edge>> {
    if left == right {
        return Ok(left.cloned());
    }
    if left == base {
        return Ok(right.cloned());
    }
    if right == base {
        return Ok(left.cloned());
    }
    let (Some(base), Some(left), Some(right)) = (base, left, right) else {
        conflicts.push(GraphMergeConflict {
            entity: GraphMergeConflictEntity::Edge(id.to_string()),
            kind: GraphMergeConflictKind::SameEdgeId,
        });
        return Ok(None);
    };

    let left_endpoints_changed = left.src != base.src || left.dst != base.dst;
    let right_endpoints_changed = right.src != base.src || right.dst != base.dst;
    if left_endpoints_changed
        && right_endpoints_changed
        && (&left.src, &left.dst) != (&right.src, &right.dst)
    {
        conflicts.push(GraphMergeConflict {
            entity: GraphMergeConflictEntity::Edge(id.to_string()),
            kind: GraphMergeConflictKind::AdjacencyChange,
        });
        return Ok(None);
    }
    let left_label_changed = left.label != base.label;
    let right_label_changed = right.label != base.label;
    if left_label_changed && right_label_changed && left.label != right.label {
        conflicts.push(GraphMergeConflict {
            entity: GraphMergeConflictEntity::Edge(id.to_string()),
            kind: GraphMergeConflictKind::LabelConflict,
        });
        return Ok(None);
    }

    let props = merge_props(
        &GraphMergeConflictEntity::Edge(id.to_string()),
        &base.props,
        &left.props,
        &right.props,
        conflicts,
    );
    if conflicts.iter().any(|conflict| {
        matches!(&conflict.entity, GraphMergeConflictEntity::Edge(conflict_id) if conflict_id == id)
    }) {
        return Ok(None);
    }

    let source = if left_endpoints_changed { left } else { right };
    let label_source = if left_label_changed { left } else { right };
    Ok(Some(Edge {
        src: source.src.clone(),
        dst: source.dst.clone(),
        label: label_source.label.clone(),
        props,
    }))
}

fn merge_labels(
    id: &str,
    base: &BTreeSet<String>,
    left: &BTreeSet<String>,
    right: &BTreeSet<String>,
    conflicts: &mut Vec<GraphMergeConflict>,
) -> BTreeSet<String> {
    let mut labels = base.clone();
    for label in base.difference(left) {
        labels.remove(label);
    }
    for label in base.difference(right) {
        labels.remove(label);
    }
    labels.extend(left.difference(base).cloned());
    labels.extend(right.difference(base).cloned());
    for label in left.symmetric_difference(right) {
        let left_changed = left.contains(label) != base.contains(label);
        let right_changed = right.contains(label) != base.contains(label);
        if left_changed && right_changed {
            conflicts.push(GraphMergeConflict {
                entity: GraphMergeConflictEntity::Node(id.to_string()),
                kind: GraphMergeConflictKind::LabelConflict,
            });
            break;
        }
    }
    labels
}

fn merge_props(
    entity: &GraphMergeConflictEntity,
    base: &Props,
    left: &Props,
    right: &Props,
    conflicts: &mut Vec<GraphMergeConflict>,
) -> Props {
    let mut names = BTreeSet::new();
    names.extend(base.keys().cloned());
    names.extend(left.keys().cloned());
    names.extend(right.keys().cloned());
    let mut props = Props::new();
    for name in names {
        let base_value = base.get(&name);
        let left_value = left.get(&name);
        let right_value = right.get(&name);
        let value = if left_value == right_value {
            left_value
        } else if left_value == base_value {
            right_value
        } else if right_value == base_value {
            left_value
        } else {
            conflicts.push(GraphMergeConflict {
                entity: entity.clone(),
                kind: GraphMergeConflictKind::PropertyConflict(name.clone()),
            });
            None
        };
        if let Some(value) = value {
            props.insert(name, value.clone());
        }
    }
    props
}

fn merge_index_catalog<T: Clone + PartialEq>(
    base: &BTreeMap<String, T>,
    left: &BTreeMap<String, T>,
    right: &BTreeMap<String, T>,
    entity: impl Fn(&str) -> GraphMergeConflictEntity,
    merged: &mut BTreeMap<String, T>,
    conflicts: &mut Vec<GraphMergeConflict>,
) {
    let mut names = BTreeSet::new();
    names.extend(base.keys().cloned());
    names.extend(left.keys().cloned());
    names.extend(right.keys().cloned());
    for name in names {
        let base_value = base.get(&name);
        let left_value = left.get(&name);
        let right_value = right.get(&name);
        let value = if left_value == right_value {
            left_value
        } else if left_value == base_value {
            right_value
        } else if right_value == base_value {
            left_value
        } else {
            conflicts.push(GraphMergeConflict {
                entity: entity(&name),
                kind: GraphMergeConflictKind::IndexDefinitionConflict,
            });
            None
        };
        if let Some(value) = value {
            merged.insert(name, value.clone());
        }
    }
}

impl Graph {
    /// Node ids reachable from `start` by following directed edges (only those labelled `via_label`
    /// when given), up to `max_depth` hops (`None` for unbounded). Deterministic: the reachable set is
    /// returned sorted and excludes `start`; empty when `start` is absent.
    pub fn reachable(
        &self,
        start: &str,
        max_depth: Option<usize>,
        via_label: Option<&str>,
    ) -> Vec<String> {
        use std::collections::{BTreeSet, VecDeque};
        if !self.nodes.contains_key(start) {
            return Vec::new();
        }
        let mut visited: BTreeSet<String> = BTreeSet::new();
        visited.insert(start.to_string());
        let mut queue: VecDeque<(String, usize)> = VecDeque::new();
        queue.push_back((start.to_string(), 0));
        while let Some((node, depth)) = queue.pop_front() {
            if max_depth.is_some_and(|m| depth >= m) {
                continue;
            }
            for (_, e) in self.out_edges(&node) {
                if via_label.is_some_and(|l| e.label != l) {
                    continue;
                }
                if visited.insert(e.dst.clone()) {
                    queue.push_back((e.dst.clone(), depth + 1));
                }
            }
        }
        visited.remove(start);
        visited.into_iter().collect()
    }

    /// A shortest directed path of node ids from `from` to `to` (inclusive), following edges restricted
    /// to `via_label` when given; `None` when no path exists or an endpoint is absent. Deterministic:
    /// neighbours are explored in ascending id order, so the path is stable across platforms.
    pub fn shortest_path(
        &self,
        from: &str,
        to: &str,
        via_label: Option<&str>,
    ) -> Option<Vec<String>> {
        use std::collections::{BTreeMap, BTreeSet, VecDeque};
        if !self.nodes.contains_key(from) || !self.nodes.contains_key(to) {
            return None;
        }
        if from == to {
            return Some(vec![from.to_string()]);
        }
        let mut prev: BTreeMap<String, String> = BTreeMap::new();
        let mut visited: BTreeSet<String> = BTreeSet::new();
        visited.insert(from.to_string());
        let mut queue: VecDeque<String> = VecDeque::new();
        queue.push_back(from.to_string());
        while let Some(node) = queue.pop_front() {
            let mut dsts: BTreeSet<String> = BTreeSet::new();
            for (_, e) in self.out_edges(&node) {
                if via_label.is_some_and(|l| e.label != l) {
                    continue;
                }
                dsts.insert(e.dst.clone());
            }
            for dst in dsts {
                if visited.insert(dst.clone()) {
                    prev.insert(dst.clone(), node.clone());
                    if dst == to {
                        let mut path = vec![to.to_string()];
                        let mut cur = to.to_string();
                        while let Some(p) = prev.get(&cur) {
                            path.push(p.clone());
                            cur = p.clone();
                        }
                        path.reverse();
                        return Some(path);
                    }
                    queue.push_back(dst);
                }
            }
        }
        None
    }

    pub fn query(&self, query: &GraphQuery) -> Result<GraphQueryResult> {
        validate_query(query)?;
        let mut matches = Vec::new();
        let mut bindings = BTreeMap::new();
        self.query_patterns(query, 0, &mut bindings, &mut matches)?;
        finish_query(query, &matches)
    }

    pub fn explain_query(&self, query: &GraphQuery) -> Result<GraphQueryExplain> {
        validate_query(query)?;
        let mut selections = Vec::new();
        let mut fallback_scan = false;
        for pattern in &query.patterns {
            let property_candidate = self.node_index_candidate(
                &pattern.left.variable,
                &pattern.left.props,
                &query.predicate,
            );
            let spatial_candidate =
                self.node_spatial_index_candidate(&pattern.left.variable, &query.predicate);
            let mut pattern_has_candidate = false;
            if let Some(candidate) = property_candidate {
                pattern_has_candidate = true;
                selections.push(candidate.selection);
                fallback_scan |= candidate.ids.is_none();
            }
            if let Some(candidate) = spatial_candidate {
                pattern_has_candidate = true;
                selections.push(candidate.selection);
                fallback_scan |= candidate.ids.is_none();
            }
            if !pattern_has_candidate {
                selections.push(GraphQueryIndexSelection {
                    binding: pattern.left.variable.clone(),
                    entity: GraphIndexEntity::Node,
                    property: String::new(),
                    index: None,
                    status: GraphIndexStatus::NotBuilt,
                    reason: "no indexable node equality predicate".to_string(),
                });
                fallback_scan = true;
            }
            for segment in &pattern.segments {
                if let Some(edge_variable) = &segment.edge.variable {
                    if let Some(candidate) =
                        self.edge_spatial_index_candidate(edge_variable, &query.predicate)
                    {
                        selections.push(candidate.selection);
                        fallback_scan |= candidate.ids.is_none();
                    } else {
                        selections.push(GraphQueryIndexSelection {
                            binding: edge_variable.clone(),
                            entity: GraphIndexEntity::Edge,
                            property: String::new(),
                            index: None,
                            status: GraphIndexStatus::NotBuilt,
                            reason: "no indexable edge spatial predicate".to_string(),
                        });
                    }
                } else {
                    selections.push(GraphQueryIndexSelection {
                        binding: "<anonymous>".to_string(),
                        entity: GraphIndexEntity::Edge,
                        property: String::new(),
                        index: None,
                        status: GraphIndexStatus::NotBuilt,
                        reason: "anonymous edge pattern has no indexable binding".to_string(),
                    });
                }
            }
        }
        Ok(GraphQueryExplain {
            indexes: self.property_index_reports(),
            spatial_indexes: self.spatial_index_reports(),
            selections,
            fallback_scan,
        })
    }

    pub fn apply_mutations(&mut self, plan: &GraphMutationPlan) -> Result<GraphMutationResult> {
        validate_mutation_plan(plan)?;
        let mut next = self.clone();
        for mutation in &plan.mutations {
            next.apply_mutation(mutation)?;
        }
        let applied = plan.mutations.len();
        *self = next;
        Ok(GraphMutationResult { applied })
    }

    fn apply_mutation(&mut self, mutation: &GraphMutation) -> Result<()> {
        match mutation {
            GraphMutation::CreateNode { id, labels, props } => {
                self.create_node_with_labels(id, labels.clone(), props.clone())
            }
            GraphMutation::CreateEdge {
                id,
                src,
                dst,
                label,
                props,
            } => self.create_edge(id, src, dst, label, props.clone()),
            GraphMutation::MergeNode { id, labels, props } => {
                self.merge_node_with_labels(id, labels.clone(), props.clone())
            }
            GraphMutation::MergeEdge {
                id,
                src,
                dst,
                label,
                props,
            } => self.merge_edge(id, src, dst, label, props.clone()),
            GraphMutation::SetNodeProperty {
                id,
                property,
                value,
            } => self.set_node_property(id, property, value.clone()),
            GraphMutation::SetEdgeProperty {
                id,
                property,
                value,
            } => self.set_edge_property(id, property, value.clone()),
            GraphMutation::RemoveNodeProperty { id, property } => {
                self.remove_node_property(id, property).map(|_| ())
            }
            GraphMutation::RemoveEdgeProperty { id, property } => {
                self.remove_edge_property(id, property).map(|_| ())
            }
            GraphMutation::DeleteNode { id, detach } => self.remove_node(id, *detach),
            GraphMutation::DeleteEdge { id } => {
                if self.edge(id).is_none() {
                    return Err(LoomError::not_found(format!("edge {id:?}")));
                }
                self.remove_edge(id);
                Ok(())
            }
        }
    }

    fn materialize_property_index(
        &self,
        index: &GraphPropertyIndex,
    ) -> Result<GraphPropertyIndexMaterialization> {
        let mut values: BTreeMap<Vec<u8>, BTreeSet<String>> = BTreeMap::new();
        match index.entity {
            GraphIndexEntity::Node => {
                for (id, node) in &self.nodes {
                    if let Some(value) = node.props.get(&index.property) {
                        values
                            .entry(graph_index_value_key(value)?)
                            .or_default()
                            .insert(id.clone());
                    }
                }
            }
            GraphIndexEntity::Edge => {
                for (id, edge) in &self.edges {
                    if let Some(value) = edge.props.get(&index.property) {
                        values
                            .entry(graph_index_value_key(value)?)
                            .or_default()
                            .insert(id.clone());
                    }
                }
            }
        }
        Ok(GraphPropertyIndexMaterialization {
            source_key: self.property_index_source_key(index)?,
            values,
        })
    }

    fn property_index_report(
        &self,
        index: &GraphPropertyIndex,
    ) -> Option<GraphPropertyIndexReport> {
        let materialization = self.property_index_materializations.get(&index.name);
        let status = self.property_index_status(index, materialization);
        let (entries, distinct_values) = materialization
            .map(|materialization| {
                (
                    materialization.values.values().map(BTreeSet::len).sum(),
                    materialization.values.len(),
                )
            })
            .unwrap_or((0, 0));
        Some(GraphPropertyIndexReport {
            index: index.clone(),
            status,
            entries,
            distinct_values,
        })
    }

    fn property_index_status(
        &self,
        index: &GraphPropertyIndex,
        materialization: Option<&GraphPropertyIndexMaterialization>,
    ) -> GraphIndexStatus {
        let Some(materialization) = materialization else {
            return GraphIndexStatus::NotBuilt;
        };
        match self.property_index_source_key(index) {
            Ok(source_key) if source_key == materialization.source_key => GraphIndexStatus::Ready,
            Ok(_) | Err(_) => GraphIndexStatus::Stale,
        }
    }

    fn property_index_source_key(&self, index: &GraphPropertyIndex) -> Result<Vec<u8>> {
        let values = match index.entity {
            GraphIndexEntity::Node => self
                .nodes
                .iter()
                .map(|(id, node)| {
                    Value::Array(vec![
                        Value::Text(id.clone()),
                        node.props
                            .get(&index.property)
                            .map(GraphValue::to_cbor)
                            .unwrap_or(Value::Null),
                    ])
                })
                .collect(),
            GraphIndexEntity::Edge => self
                .edges
                .iter()
                .map(|(id, edge)| {
                    Value::Array(vec![
                        Value::Text(id.clone()),
                        edge.props
                            .get(&index.property)
                            .map(GraphValue::to_cbor)
                            .unwrap_or(Value::Null),
                    ])
                })
                .collect(),
        };
        Ok(cbor::encode(&Value::Array(values)))
    }

    fn materialize_spatial_index(
        &self,
        index: &GraphSpatialIndex,
    ) -> Result<GraphSpatialIndexMaterialization> {
        let mut boxes = BTreeMap::new();
        match index.entity {
            GraphIndexEntity::Node => {
                for (id, node) in &self.nodes {
                    if let Some(GraphValue::Geometry(GraphGeometry::Point(point))) =
                        node.props.get(&index.property)
                    {
                        boxes.insert(id.clone(), GraphBoundingBox::point(point));
                    }
                }
            }
            GraphIndexEntity::Edge => {
                for (id, edge) in &self.edges {
                    if let Some(GraphValue::Geometry(GraphGeometry::Point(point))) =
                        edge.props.get(&index.property)
                    {
                        boxes.insert(id.clone(), GraphBoundingBox::point(point));
                    }
                }
            }
        }
        Ok(GraphSpatialIndexMaterialization {
            source_key: self.spatial_index_source_key(index)?,
            boxes,
        })
    }

    fn spatial_index_report(&self, index: &GraphSpatialIndex) -> Option<GraphSpatialIndexReport> {
        let materialization = self.spatial_index_materializations.get(&index.name);
        let status = self.spatial_index_status(index, materialization);
        let entries = materialization
            .map(|materialization| materialization.boxes.len())
            .unwrap_or(0);
        Some(GraphSpatialIndexReport {
            index: index.clone(),
            status,
            entries,
        })
    }

    fn spatial_index_status(
        &self,
        index: &GraphSpatialIndex,
        materialization: Option<&GraphSpatialIndexMaterialization>,
    ) -> GraphIndexStatus {
        let Some(materialization) = materialization else {
            return GraphIndexStatus::NotBuilt;
        };
        match self.spatial_index_source_key(index) {
            Ok(source_key) if source_key == materialization.source_key => GraphIndexStatus::Ready,
            Ok(_) | Err(_) => GraphIndexStatus::Stale,
        }
    }

    fn spatial_index_source_key(&self, index: &GraphSpatialIndex) -> Result<Vec<u8>> {
        let values = match index.entity {
            GraphIndexEntity::Node => self
                .nodes
                .iter()
                .map(|(id, node)| {
                    Value::Array(vec![
                        Value::Text(id.clone()),
                        node.props
                            .get(&index.property)
                            .map(GraphValue::to_cbor)
                            .unwrap_or(Value::Null),
                    ])
                })
                .collect(),
            GraphIndexEntity::Edge => self
                .edges
                .iter()
                .map(|(id, edge)| {
                    Value::Array(vec![
                        Value::Text(id.clone()),
                        edge.props
                            .get(&index.property)
                            .map(GraphValue::to_cbor)
                            .unwrap_or(Value::Null),
                    ])
                })
                .collect(),
        };
        Ok(cbor::encode(&Value::Array(values)))
    }

    fn node_candidate_ids(&self, query: &GraphQuery, pattern: &GraphPattern) -> Vec<String> {
        let property_ids = self
            .node_index_candidate(
                &pattern.left.variable,
                &pattern.left.props,
                &query.predicate,
            )
            .and_then(|candidate| candidate.ids);
        let spatial_ids = self
            .node_spatial_index_candidate(&pattern.left.variable, &query.predicate)
            .and_then(|candidate| candidate.ids);
        let full_text_ids = predicate_full_text_ids(&query.predicate, &pattern.left.variable);
        match (property_ids, spatial_ids, full_text_ids) {
            (Some(ids), Some(spatial_ids), Some(full_text_ids)) => ids
                .into_iter()
                .filter(|id| spatial_ids.iter().any(|spatial_id| spatial_id == id))
                .filter(|id| full_text_ids.contains(id))
                .collect(),
            (Some(ids), Some(spatial_ids), None) => ids
                .into_iter()
                .filter(|id| spatial_ids.iter().any(|spatial_id| spatial_id == id))
                .collect(),
            (Some(ids), None, Some(full_text_ids)) => ids
                .into_iter()
                .filter(|id| full_text_ids.contains(id))
                .collect(),
            (None, Some(ids), Some(full_text_ids)) => ids
                .into_iter()
                .filter(|id| full_text_ids.contains(id))
                .collect(),
            (Some(ids), None, None) | (None, Some(ids), None) => ids,
            (None, None, Some(full_text_ids)) => full_text_ids.iter().cloned().collect(),
            (None, None, None) => self.nodes.keys().cloned().collect(),
        }
    }

    fn node_index_candidate(
        &self,
        binding: &str,
        props: &Props,
        predicate: &GraphPredicate,
    ) -> Option<GraphIndexCandidate> {
        let (property, value) = props
            .iter()
            .next()
            .map(|(property, value)| (property.as_str(), value))
            .or_else(|| predicate_eq_value(predicate, binding))?;
        let index = self
            .property_indexes
            .values()
            .find(|index| index.entity == GraphIndexEntity::Node && index.property == property)?;
        let materialization = self.property_index_materializations.get(&index.name);
        let status = self.property_index_status(index, materialization);
        if status != GraphIndexStatus::Ready {
            return Some(GraphIndexCandidate {
                ids: None,
                selection: GraphQueryIndexSelection {
                    binding: binding.to_string(),
                    entity: GraphIndexEntity::Node,
                    property: property.to_string(),
                    index: Some(index.name.clone()),
                    status,
                    reason: "declared graph property index is not ready".to_string(),
                },
            });
        }
        let ids = materialization
            .and_then(|materialization| {
                graph_index_value_key(value)
                    .ok()
                    .and_then(|key| materialization.values.get(&key).cloned())
            })
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        Some(GraphIndexCandidate {
            ids: Some(ids),
            selection: GraphQueryIndexSelection {
                binding: binding.to_string(),
                entity: GraphIndexEntity::Node,
                property: property.to_string(),
                index: Some(index.name.clone()),
                status,
                reason: "ready graph property index selected".to_string(),
            },
        })
    }

    fn node_spatial_index_candidate(
        &self,
        binding: &str,
        predicate: &GraphPredicate,
    ) -> Option<GraphIndexCandidate> {
        let (property, bbox) = predicate_spatial_bbox(predicate, binding)?;
        let index = self
            .spatial_indexes
            .values()
            .find(|index| index.entity == GraphIndexEntity::Node && index.property == property)?;
        let materialization = self.spatial_index_materializations.get(&index.name);
        let status = self.spatial_index_status(index, materialization);
        if status != GraphIndexStatus::Ready {
            return Some(GraphIndexCandidate {
                ids: None,
                selection: GraphQueryIndexSelection {
                    binding: binding.to_string(),
                    entity: GraphIndexEntity::Node,
                    property: property.to_string(),
                    index: Some(index.name.clone()),
                    status,
                    reason: "declared graph spatial index is not ready".to_string(),
                },
            });
        }
        let ids = materialization
            .map(|materialization| {
                materialization
                    .boxes
                    .iter()
                    .filter(|(_, candidate)| candidate.intersects(&bbox))
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Some(GraphIndexCandidate {
            ids: Some(ids),
            selection: GraphQueryIndexSelection {
                binding: binding.to_string(),
                entity: GraphIndexEntity::Node,
                property: property.to_string(),
                index: Some(index.name.clone()),
                status,
                reason: "ready graph spatial index selected".to_string(),
            },
        })
    }

    fn edge_spatial_index_candidate(
        &self,
        binding: &str,
        predicate: &GraphPredicate,
    ) -> Option<GraphIndexCandidate> {
        let (property, bbox) = predicate_spatial_bbox(predicate, binding)?;
        let index = self
            .spatial_indexes
            .values()
            .find(|index| index.entity == GraphIndexEntity::Edge && index.property == property)?;
        let materialization = self.spatial_index_materializations.get(&index.name);
        let status = self.spatial_index_status(index, materialization);
        if status != GraphIndexStatus::Ready {
            return Some(GraphIndexCandidate {
                ids: None,
                selection: GraphQueryIndexSelection {
                    binding: binding.to_string(),
                    entity: GraphIndexEntity::Edge,
                    property: property.to_string(),
                    index: Some(index.name.clone()),
                    status,
                    reason: "declared graph edge spatial index is not ready".to_string(),
                },
            });
        }
        let ids = materialization
            .map(|materialization| {
                materialization
                    .boxes
                    .iter()
                    .filter(|(_, candidate)| candidate.intersects(&bbox))
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Some(GraphIndexCandidate {
            ids: Some(ids),
            selection: GraphQueryIndexSelection {
                binding: binding.to_string(),
                entity: GraphIndexEntity::Edge,
                property: property.to_string(),
                index: Some(index.name.clone()),
                status,
                reason: "ready graph edge spatial index selected".to_string(),
            },
        })
    }

    fn edge_spatial_candidate_ids(
        &self,
        query: &GraphQuery,
        segment: &GraphPathSegment,
    ) -> Option<BTreeSet<String>> {
        let variable = segment.edge.variable.as_deref()?;
        self.edge_spatial_index_candidate(variable, &query.predicate)
            .and_then(|candidate| candidate.ids)
            .map(|ids| ids.into_iter().collect())
    }

    fn query_patterns<'a>(
        &'a self,
        query: &GraphQuery,
        index: usize,
        bindings: &mut BTreeMap<String, GraphBinding<'a>>,
        matches: &mut Vec<BTreeMap<String, GraphBinding<'a>>>,
    ) -> Result<()> {
        if matches.len() >= GRAPH_QUERY_DEFAULT_MATCH_LIMIT {
            return Ok(());
        }
        if index == query.patterns.len() {
            if eval_predicate(&query.predicate, bindings)? {
                matches.push(bindings.clone());
            }
            return Ok(());
        }
        let pattern = &query.patterns[index];
        if pattern.segments.is_empty() {
            let node_ids = self.node_candidate_ids(query, pattern);
            for candidate_id in node_ids {
                let Some((node_id, node)) = self.nodes.get_key_value(&candidate_id) else {
                    continue;
                };
                if !node_matches(node_id, node, &pattern.left) {
                    continue;
                }
                let snapshot = bindings.clone();
                if bind_node(bindings, &pattern.left.variable, node_id, node)? {
                    self.query_patterns(query, index + 1, bindings, matches)?;
                }
                *bindings = snapshot;
                if matches.len() >= GRAPH_QUERY_DEFAULT_MATCH_LIMIT {
                    break;
                }
            }
        } else {
            let mut path_candidates = 0usize;
            let node_ids = self.node_candidate_ids(query, pattern);
            for candidate_id in node_ids {
                let Some((node_id, node)) = self.nodes.get_key_value(&candidate_id) else {
                    continue;
                };
                if !node_matches(node_id, node, &pattern.left) {
                    continue;
                }
                let snapshot = bindings.clone();
                if bind_node(bindings, &pattern.left.variable, node_id, node)? {
                    let path = GraphPath {
                        nodes: vec![query_node_value(node_id, node)],
                        edges: Vec::new(),
                    };
                    if pattern.path_selection == GraphPathSelection::Shortest {
                        self.query_shortest_path_pattern(
                            query,
                            index,
                            pattern,
                            node_id,
                            bindings,
                            path,
                            matches,
                            &mut path_candidates,
                        )?;
                    } else {
                        self.query_path_segments(
                            query,
                            index,
                            pattern,
                            0,
                            node_id,
                            bindings,
                            path,
                            matches,
                            &mut path_candidates,
                        )?;
                    }
                }
                *bindings = snapshot;
                if matches.len() >= GRAPH_QUERY_DEFAULT_MATCH_LIMIT {
                    break;
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn query_shortest_path_pattern<'a>(
        &'a self,
        query: &GraphQuery,
        pattern_index: usize,
        pattern: &GraphPattern,
        current_node_id: &'a str,
        bindings: &mut BTreeMap<String, GraphBinding<'a>>,
        path: GraphPath,
        matches: &mut Vec<BTreeMap<String, GraphBinding<'a>>>,
        path_candidates: &mut usize,
    ) -> Result<()> {
        let segment = pattern
            .segments
            .first()
            .ok_or_else(|| LoomError::invalid("graph shortestPath requires a path segment"))?;
        let mut candidates = Vec::new();
        let mut visited = BTreeSet::from([current_node_id.to_string()]);
        let edge_candidate_ids = self.edge_spatial_candidate_ids(query, segment);
        self.collect_segment_paths(
            edge_candidate_ids.as_ref(),
            segment,
            current_node_id,
            path,
            path_candidates,
            0,
            &mut visited,
            &mut candidates,
        )?;
        let Some(shortest) = candidates.iter().map(|path| path.edges.len()).min() else {
            return Ok(());
        };
        candidates.retain(|path| path.edges.len() == shortest);
        candidates.sort_by_key(path_key);
        for path in candidates {
            let Some(end_node_value) = path.nodes.last() else {
                continue;
            };
            let Some((end_node_id, bound_end_node)) = self.nodes.get_key_value(&end_node_value.id)
            else {
                continue;
            };
            let snapshot = bindings.clone();
            if bind_node(
                bindings,
                &segment.right.variable,
                end_node_id,
                bound_end_node,
            )? && bind_path(
                bindings,
                pattern.path_variable.as_deref().ok_or_else(|| {
                    LoomError::invalid("graph shortestPath requires path binding")
                })?,
                path,
            )? {
                self.query_patterns(query, pattern_index + 1, bindings, matches)?;
            }
            *bindings = snapshot;
            if matches.len() >= GRAPH_QUERY_DEFAULT_MATCH_LIMIT {
                break;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_segment_paths<'a>(
        &'a self,
        edge_candidate_ids: Option<&BTreeSet<String>>,
        segment: &GraphPathSegment,
        current_node_id: &'a str,
        path: GraphPath,
        path_candidates: &mut usize,
        depth: usize,
        visited: &mut BTreeSet<String>,
        candidates: &mut Vec<GraphPath>,
    ) -> Result<()> {
        if depth >= segment.edge.min_hops {
            let right_node = self
                .nodes
                .get(current_node_id)
                .ok_or_else(|| LoomError::corrupt("graph path endpoint is missing"))?;
            if node_matches(current_node_id, right_node, &segment.right) {
                *path_candidates += 1;
                if *path_candidates > GRAPH_QUERY_MAX_PATH_CANDIDATES {
                    return Err(LoomError::new(
                        Code::ResourceExhausted,
                        "graph query path candidate limit exceeded",
                    ));
                }
                validate_path_value_budget(&path)?;
                candidates.push(path.clone());
            }
        }
        if depth == segment.edge.max_hops {
            return Ok(());
        }
        let mut fanout = 0usize;
        for (edge_id, edge) in self.out_edges(current_node_id) {
            if edge_candidate_ids.is_some_and(|ids| !ids.contains(edge_id)) {
                continue;
            }
            if !edge_matches(edge, &segment.edge) {
                continue;
            }
            fanout += 1;
            if fanout > GRAPH_QUERY_MAX_PATH_FANOUT {
                return Err(LoomError::new(
                    Code::ResourceExhausted,
                    "graph query path fanout limit exceeded",
                ));
            }
            if visited.contains(&edge.dst) {
                continue;
            }
            let Some(next_node) = self.nodes.get(&edge.dst) else {
                continue;
            };
            visited.insert(edge.dst.clone());
            let mut next_path = path.clone();
            next_path.edges.push(query_edge_value(edge_id, edge));
            next_path.nodes.push(query_node_value(&edge.dst, next_node));
            validate_path_value_budget(&next_path)?;
            self.collect_segment_paths(
                edge_candidate_ids,
                segment,
                &edge.dst,
                next_path,
                path_candidates,
                depth + 1,
                visited,
                candidates,
            )?;
            visited.remove(&edge.dst);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn query_path_segments<'a>(
        &'a self,
        query: &GraphQuery,
        pattern_index: usize,
        pattern: &GraphPattern,
        segment_index: usize,
        current_node_id: &'a str,
        bindings: &mut BTreeMap<String, GraphBinding<'a>>,
        path: GraphPath,
        matches: &mut Vec<BTreeMap<String, GraphBinding<'a>>>,
        path_candidates: &mut usize,
    ) -> Result<()> {
        if matches.len() >= GRAPH_QUERY_DEFAULT_MATCH_LIMIT {
            return Ok(());
        }
        if segment_index == pattern.segments.len() {
            let snapshot = bindings.clone();
            if let Some(path_variable) = &pattern.path_variable
                && !bind_path(bindings, path_variable, path)?
            {
                *bindings = snapshot;
                return Ok(());
            }
            self.query_patterns(query, pattern_index + 1, bindings, matches)?;
            *bindings = snapshot;
            return Ok(());
        }
        let segment = &pattern.segments[segment_index];
        if segment.edge.min_hops == 1 && segment.edge.max_hops == 1 {
            self.query_one_hop_segment(
                query,
                pattern_index,
                pattern,
                segment_index,
                current_node_id,
                bindings,
                path,
                matches,
                path_candidates,
            )?;
        } else {
            let mut visited = BTreeSet::from([current_node_id.to_string()]);
            self.query_variable_segment(
                query,
                pattern_index,
                pattern,
                segment_index,
                current_node_id,
                bindings,
                path,
                matches,
                path_candidates,
                0,
                &mut visited,
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn query_one_hop_segment<'a>(
        &'a self,
        query: &GraphQuery,
        pattern_index: usize,
        pattern: &GraphPattern,
        segment_index: usize,
        current_node_id: &'a str,
        bindings: &mut BTreeMap<String, GraphBinding<'a>>,
        path: GraphPath,
        matches: &mut Vec<BTreeMap<String, GraphBinding<'a>>>,
        path_candidates: &mut usize,
    ) -> Result<()> {
        let segment = &pattern.segments[segment_index];
        let edge_candidate_ids = self.edge_spatial_candidate_ids(query, segment);
        let mut fanout = 0usize;
        for (edge_id, edge) in self.out_edges(current_node_id) {
            if edge_candidate_ids
                .as_ref()
                .is_some_and(|ids| !ids.contains(edge_id))
            {
                continue;
            }
            if !edge_matches(edge, &segment.edge) {
                continue;
            }
            fanout += 1;
            if fanout > GRAPH_QUERY_MAX_PATH_FANOUT {
                return Err(LoomError::new(
                    Code::ResourceExhausted,
                    "graph query path fanout limit exceeded",
                ));
            }
            let Some(right_node) = self.nodes.get(&edge.dst) else {
                continue;
            };
            if !node_matches(&edge.dst, right_node, &segment.right) {
                continue;
            }
            *path_candidates += 1;
            if *path_candidates > GRAPH_QUERY_MAX_PATH_CANDIDATES {
                return Err(LoomError::new(
                    Code::ResourceExhausted,
                    "graph query path candidate limit exceeded",
                ));
            }
            let snapshot = bindings.clone();
            let edge_bound = match &segment.edge.variable {
                Some(variable) => bind_edge(bindings, variable, edge_id, edge)?,
                None => true,
            };
            if edge_bound && bind_node(bindings, &segment.right.variable, &edge.dst, right_node)? {
                let mut next_path = path.clone();
                next_path.edges.push(query_edge_value(edge_id, edge));
                next_path
                    .nodes
                    .push(query_node_value(&edge.dst, right_node));
                validate_path_value_budget(&next_path)?;
                self.query_path_segments(
                    query,
                    pattern_index,
                    pattern,
                    segment_index + 1,
                    &edge.dst,
                    bindings,
                    next_path,
                    matches,
                    path_candidates,
                )?;
            }
            *bindings = snapshot;
            if matches.len() >= GRAPH_QUERY_DEFAULT_MATCH_LIMIT {
                break;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn query_variable_segment<'a>(
        &'a self,
        query: &GraphQuery,
        pattern_index: usize,
        pattern: &GraphPattern,
        segment_index: usize,
        current_node_id: &'a str,
        bindings: &mut BTreeMap<String, GraphBinding<'a>>,
        path: GraphPath,
        matches: &mut Vec<BTreeMap<String, GraphBinding<'a>>>,
        path_candidates: &mut usize,
        depth: usize,
        visited: &mut BTreeSet<String>,
    ) -> Result<()> {
        let segment = &pattern.segments[segment_index];
        if depth >= segment.edge.min_hops {
            let right_node = self
                .nodes
                .get(current_node_id)
                .ok_or_else(|| LoomError::corrupt("graph path endpoint is missing"))?;
            if node_matches(current_node_id, right_node, &segment.right) {
                *path_candidates += 1;
                if *path_candidates > GRAPH_QUERY_MAX_PATH_CANDIDATES {
                    return Err(LoomError::new(
                        Code::ResourceExhausted,
                        "graph query path candidate limit exceeded",
                    ));
                }
                validate_path_value_budget(&path)?;
                let snapshot = bindings.clone();
                if bind_node(
                    bindings,
                    &segment.right.variable,
                    current_node_id,
                    right_node,
                )? {
                    self.query_path_segments(
                        query,
                        pattern_index,
                        pattern,
                        segment_index + 1,
                        current_node_id,
                        bindings,
                        path.clone(),
                        matches,
                        path_candidates,
                    )?;
                }
                *bindings = snapshot;
            }
        }
        if depth == segment.edge.max_hops || matches.len() >= GRAPH_QUERY_DEFAULT_MATCH_LIMIT {
            return Ok(());
        }
        let mut fanout = 0usize;
        let edge_candidate_ids = self.edge_spatial_candidate_ids(query, segment);
        for (edge_id, edge) in self.out_edges(current_node_id) {
            if edge_candidate_ids
                .as_ref()
                .is_some_and(|ids| !ids.contains(edge_id))
            {
                continue;
            }
            if !edge_matches(edge, &segment.edge) {
                continue;
            }
            fanout += 1;
            if fanout > GRAPH_QUERY_MAX_PATH_FANOUT {
                return Err(LoomError::new(
                    Code::ResourceExhausted,
                    "graph query path fanout limit exceeded",
                ));
            }
            if visited.contains(&edge.dst) {
                continue;
            }
            let Some(next_node) = self.nodes.get(&edge.dst) else {
                continue;
            };
            visited.insert(edge.dst.clone());
            let mut next_path = path.clone();
            next_path.edges.push(query_edge_value(edge_id, edge));
            next_path.nodes.push(query_node_value(&edge.dst, next_node));
            validate_path_value_budget(&next_path)?;
            self.query_variable_segment(
                query,
                pattern_index,
                pattern,
                segment_index,
                &edge.dst,
                bindings,
                next_path,
                matches,
                path_candidates,
                depth + 1,
                visited,
            )?;
            visited.remove(&edge.dst);
            if matches.len() >= GRAPH_QUERY_DEFAULT_MATCH_LIMIT {
                break;
            }
        }
        Ok(())
    }
}

fn validate_query(query: &GraphQuery) -> Result<()> {
    if query.patterns.is_empty() {
        return Err(LoomError::invalid(
            "graph query requires at least one pattern",
        ));
    }
    if query.limit == Some(0) {
        return Err(LoomError::invalid("graph query limit must be non-zero"));
    }
    for pattern in &query.patterns {
        if let Some(path_variable) = &pattern.path_variable {
            validate_binding(path_variable)?;
            if pattern.segments.is_empty() {
                return Err(LoomError::invalid(
                    "graph query path binding requires an edge pattern",
                ));
            }
        }
        if pattern.path_selection == GraphPathSelection::Shortest {
            if pattern.path_variable.is_none() {
                return Err(LoomError::invalid(
                    "graph shortestPath requires a path binding",
                ));
            }
            if pattern.segments.len() != 1 {
                return Err(LoomError::invalid(
                    "graph shortestPath supports one path segment",
                ));
            }
            if pattern.segments[0].edge.variable.is_some() {
                return Err(LoomError::invalid(
                    "graph shortestPath relationship binding requires list values",
                ));
            }
        }
        validate_binding(&pattern.left.variable)?;
        validate_labels(&pattern.left.labels)?;
        validate_props(&pattern.left.props)?;
        for segment in &pattern.segments {
            let edge = &segment.edge;
            if let Some(variable) = &edge.variable {
                validate_binding(variable)?;
            }
            validate_props(&edge.props)?;
            if let Some(label) = &edge.label {
                validate_label(label)?;
            }
            if edge.min_hops > edge.max_hops {
                return Err(LoomError::invalid(
                    "graph query variable path minimum exceeds maximum",
                ));
            }
            if edge.max_hops > GRAPH_QUERY_MAX_PATH_HOPS {
                return Err(LoomError::invalid(
                    "graph query variable path exceeds hop limit",
                ));
            }
            if edge.min_hops != 1 || edge.max_hops != 1 {
                if edge.variable.is_some() {
                    return Err(LoomError::invalid(
                        "graph query variable-length relationship binding requires list values",
                    ));
                }
                if !edge.props.is_empty() {
                    return Err(LoomError::invalid(
                        "graph query variable-length paths do not support relationship property filters",
                    ));
                }
            }
            let right = &segment.right;
            validate_binding(&right.variable)?;
            validate_labels(&right.labels)?;
            validate_props(&right.props)?;
        }
    }
    validate_predicate(&query.predicate)?;
    for projection in &query.returns {
        match projection {
            GraphReturn::Binding(binding) => validate_binding(binding)?,
            GraphReturn::Property { binding, property } => {
                validate_binding(binding)?;
                if property.is_empty() {
                    return Err(LoomError::invalid(
                        "graph query property projection must not be empty",
                    ));
                }
            }
            GraphReturn::Count { binding, alias } => {
                if let Some(binding) = binding {
                    validate_binding(binding)?;
                }
                validate_return_alias(alias)?;
            }
            GraphReturn::PathLength { binding, alias } => {
                validate_binding(binding)?;
                validate_return_alias(alias)?;
            }
            GraphReturn::Function { function, alias } => {
                validate_graph_function(function)?;
                validate_return_alias(alias)?;
            }
        }
    }
    for order in &query.order_by {
        match &order.item {
            GraphOrderItem::Binding(binding) | GraphOrderItem::ReturnKey(binding) => {
                validate_binding(binding)?
            }
            GraphOrderItem::PathLength(binding) => validate_binding(binding)?,
            GraphOrderItem::Function(function) => validate_graph_function(function)?,
            GraphOrderItem::Property { binding, property } => {
                validate_binding(binding)?;
                if property.is_empty() {
                    return Err(LoomError::invalid(
                        "graph query order property must not be empty",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_graph_function(function: &GraphFunction) -> Result<()> {
    match function {
        GraphFunction::Id { binding }
        | GraphFunction::Type { binding }
        | GraphFunction::StartNode { binding }
        | GraphFunction::EndNode { binding }
        | GraphFunction::Labels { binding }
        | GraphFunction::Keys { binding }
        | GraphFunction::Properties { binding }
        | GraphFunction::Nodes { binding }
        | GraphFunction::Relationships { binding } => validate_binding(binding),
    }
}

fn validate_mutation_plan(plan: &GraphMutationPlan) -> Result<()> {
    if plan.mutations.is_empty() {
        return Err(LoomError::invalid(
            "graph mutation plan requires at least one mutation",
        ));
    }
    for mutation in &plan.mutations {
        match mutation {
            GraphMutation::CreateNode { id, labels, props }
            | GraphMutation::MergeNode { id, labels, props } => {
                validate_graph_id(id)?;
                validate_labels(labels)?;
                validate_props(props)?;
            }
            GraphMutation::CreateEdge {
                id,
                src,
                dst,
                label,
                props,
            }
            | GraphMutation::MergeEdge {
                id,
                src,
                dst,
                label,
                props,
            } => {
                validate_graph_id(id)?;
                validate_graph_id(src)?;
                validate_graph_id(dst)?;
                validate_label(label)?;
                validate_props(props)?;
            }
            GraphMutation::SetNodeProperty {
                id,
                property,
                value,
            }
            | GraphMutation::SetEdgeProperty {
                id,
                property,
                value,
            } => {
                validate_graph_id(id)?;
                validate_property_name(property)?;
                value.validate()?;
            }
            GraphMutation::RemoveNodeProperty { id, property }
            | GraphMutation::RemoveEdgeProperty { id, property } => {
                validate_graph_id(id)?;
                validate_property_name(property)?;
            }
            GraphMutation::DeleteNode { id, .. } | GraphMutation::DeleteEdge { id } => {
                validate_graph_id(id)?;
            }
        }
    }
    Ok(())
}

fn validate_binding(binding: &str) -> Result<()> {
    if binding.is_empty() {
        Err(LoomError::invalid("graph query binding must not be empty"))
    } else {
        Ok(())
    }
}

fn validate_return_alias(alias: &str) -> Result<()> {
    if alias.is_empty() {
        return Err(LoomError::invalid(
            "graph query aggregate alias must not be empty",
        ));
    }
    Ok(())
}

fn validate_predicate(predicate: &GraphPredicate) -> Result<()> {
    match predicate {
        GraphPredicate::All => Ok(()),
        GraphPredicate::Eq {
            binding,
            property,
            value,
        }
        | GraphPredicate::Ne {
            binding,
            property,
            value,
        }
        | GraphPredicate::Gt {
            binding,
            property,
            value,
        }
        | GraphPredicate::Gte {
            binding,
            property,
            value,
        }
        | GraphPredicate::Lt {
            binding,
            property,
            value,
        }
        | GraphPredicate::Lte {
            binding,
            property,
            value,
        } => {
            validate_binding(binding)?;
            if property.is_empty() {
                return Err(LoomError::invalid(
                    "graph query predicate property must not be empty",
                ));
            }
            value.validate()
        }
        GraphPredicate::RegexMatch {
            binding,
            property,
            pattern,
        } => {
            validate_binding(binding)?;
            if property.is_empty() {
                return Err(LoomError::invalid(
                    "graph query predicate property must not be empty",
                ));
            }
            validate_regex_pattern(pattern)
        }
        GraphPredicate::HasLabel { binding, label } => {
            validate_binding(binding)?;
            validate_label(label)
        }
        GraphPredicate::FullTextMatch { binding, ids } => {
            validate_binding(binding)?;
            for id in ids {
                validate_graph_id(id)?;
            }
            Ok(())
        }
        GraphPredicate::PointDistance {
            binding,
            property,
            point,
            distance,
            ..
        } => {
            validate_binding(binding)?;
            validate_property_name(property)?;
            point.validate()?;
            validate_graph_distance_value(distance)
        }
        GraphPredicate::PointWithinBBox {
            binding,
            property,
            min_x,
            min_y,
            max_x,
            max_y,
        } => {
            validate_binding(binding)?;
            validate_property_name(property)?;
            let min_x = graph_value_f64(min_x, "graph bbox min x")?;
            let min_y = graph_value_f64(min_y, "graph bbox min y")?;
            let max_x = graph_value_f64(max_x, "graph bbox max x")?;
            let max_y = graph_value_f64(max_y, "graph bbox max y")?;
            if min_x > max_x || min_y > max_y {
                return Err(LoomError::invalid(
                    "graph bbox minimum coordinates must not exceed maximum coordinates",
                ));
            }
            Ok(())
        }
        GraphPredicate::And(items) | GraphPredicate::Or(items) => {
            for item in items {
                validate_predicate(item)?;
            }
            Ok(())
        }
        GraphPredicate::Not(item) => validate_predicate(item),
    }
}

fn node_matches(id: &str, node: &Node, pattern: &GraphNodePattern) -> bool {
    if pattern.id.as_deref().is_some_and(|expected| expected != id) {
        return false;
    }
    if !pattern
        .labels
        .iter()
        .all(|label| node.labels.contains(label))
    {
        return false;
    }
    props_match(&node.props, &pattern.props)
}

fn edge_matches(edge: &Edge, pattern: &GraphEdgePattern) -> bool {
    if pattern
        .label
        .as_deref()
        .is_some_and(|expected| expected != edge.label)
    {
        return false;
    }
    props_match(&edge.props, &pattern.props)
}

fn props_match(actual: &Props, expected: &Props) -> bool {
    expected
        .iter()
        .all(|(key, value)| actual.get(key) == Some(value))
}

fn bind_node<'a>(
    bindings: &mut BTreeMap<String, GraphBinding<'a>>,
    variable: &str,
    id: &'a str,
    node: &'a Node,
) -> Result<bool> {
    match bindings.get(variable) {
        Some(GraphBinding::Node(existing_id, _)) => Ok(*existing_id == id),
        Some(GraphBinding::Edge(_, _)) | Some(GraphBinding::Path(_)) => Err(LoomError::invalid(
            "graph query binding has conflicting node and edge uses",
        )),
        None => {
            bindings.insert(variable.to_string(), GraphBinding::Node(id, node));
            Ok(true)
        }
    }
}

fn bind_edge<'a>(
    bindings: &mut BTreeMap<String, GraphBinding<'a>>,
    variable: &str,
    id: &'a str,
    edge: &'a Edge,
) -> Result<bool> {
    match bindings.get(variable) {
        Some(GraphBinding::Edge(existing_id, _)) => Ok(*existing_id == id),
        Some(GraphBinding::Node(_, _)) | Some(GraphBinding::Path(_)) => Err(LoomError::invalid(
            "graph query binding has conflicting node and edge uses",
        )),
        None => {
            bindings.insert(variable.to_string(), GraphBinding::Edge(id, edge));
            Ok(true)
        }
    }
}

fn bind_path<'a>(
    bindings: &mut BTreeMap<String, GraphBinding<'a>>,
    variable: &str,
    path: GraphPath,
) -> Result<bool> {
    match bindings.get(variable) {
        Some(GraphBinding::Path(existing)) => Ok(path_key(existing) == path_key(&path)),
        Some(GraphBinding::Node(_, _)) | Some(GraphBinding::Edge(_, _)) => Err(LoomError::invalid(
            "graph query binding has conflicting path and entity uses",
        )),
        None => {
            bindings.insert(variable.to_string(), GraphBinding::Path(path));
            Ok(true)
        }
    }
}

fn eval_predicate(
    predicate: &GraphPredicate,
    bindings: &BTreeMap<String, GraphBinding<'_>>,
) -> Result<bool> {
    match predicate {
        GraphPredicate::All => Ok(true),
        GraphPredicate::Eq {
            binding,
            property,
            value,
        } => Ok(binding_props(bindings, binding)?.get(property) == Some(value)),
        GraphPredicate::Ne {
            binding,
            property,
            value,
        } => Ok(binding_props(bindings, binding)?.get(property) != Some(value)),
        GraphPredicate::Gt {
            binding,
            property,
            value,
        } => {
            Ok(compare_predicate_value(bindings, binding, property, value)?
                == Some(Ordering::Greater))
        }
        GraphPredicate::Gte {
            binding,
            property,
            value,
        } => Ok(matches!(
            compare_predicate_value(bindings, binding, property, value)?,
            Some(Ordering::Greater | Ordering::Equal)
        )),
        GraphPredicate::Lt {
            binding,
            property,
            value,
        } => Ok(
            compare_predicate_value(bindings, binding, property, value)? == Some(Ordering::Less)
        ),
        GraphPredicate::Lte {
            binding,
            property,
            value,
        } => Ok(matches!(
            compare_predicate_value(bindings, binding, property, value)?,
            Some(Ordering::Less | Ordering::Equal)
        )),
        GraphPredicate::RegexMatch {
            binding,
            property,
            pattern,
        } => eval_regex_predicate(bindings, binding, property, pattern),
        GraphPredicate::HasLabel { binding, label } => match bindings.get(binding) {
            Some(GraphBinding::Node(_, node)) => Ok(node.labels.contains(label)),
            Some(GraphBinding::Edge(_, _)) | Some(GraphBinding::Path(_)) => Ok(false),
            None => Err(LoomError::invalid(
                "graph query predicate binding is not bound",
            )),
        },
        GraphPredicate::FullTextMatch { binding, ids } => match bindings.get(binding) {
            Some(GraphBinding::Node(id, _)) | Some(GraphBinding::Edge(id, _)) => {
                Ok(ids.contains(*id))
            }
            Some(GraphBinding::Path(_)) => Ok(false),
            None => Err(LoomError::invalid(
                "graph query predicate binding is not bound",
            )),
        },
        GraphPredicate::PointDistance {
            binding,
            property,
            point,
            operator,
            distance,
        } => eval_point_distance_predicate(bindings, binding, property, point, *operator, distance),
        GraphPredicate::PointWithinBBox {
            binding,
            property,
            min_x,
            min_y,
            max_x,
            max_y,
        } => eval_point_bbox_predicate(bindings, binding, property, min_x, min_y, max_x, max_y),
        GraphPredicate::And(items) => {
            for item in items {
                if !eval_predicate(item, bindings)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        GraphPredicate::Or(items) => {
            for item in items {
                if eval_predicate(item, bindings)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        GraphPredicate::Not(item) => Ok(!eval_predicate(item, bindings)?),
    }
}

fn compare_predicate_value(
    bindings: &BTreeMap<String, GraphBinding<'_>>,
    binding: &str,
    property: &str,
    value: &GraphValue,
) -> Result<Option<Ordering>> {
    Ok(binding_props(bindings, binding)?
        .get(property)
        .and_then(|actual| compare_graph_values(actual, value)))
}

fn predicate_eq_value<'a>(
    predicate: &'a GraphPredicate,
    binding: &str,
) -> Option<(&'a str, &'a GraphValue)> {
    match predicate {
        GraphPredicate::Eq {
            binding: predicate_binding,
            property,
            value,
        } if predicate_binding == binding => Some((property.as_str(), value)),
        GraphPredicate::And(items) => items
            .iter()
            .find_map(|item| predicate_eq_value(item, binding)),
        _ => None,
    }
}

fn predicate_full_text_ids<'a>(
    predicate: &'a GraphPredicate,
    binding: &str,
) -> Option<&'a BTreeSet<String>> {
    match predicate {
        GraphPredicate::FullTextMatch {
            binding: predicate_binding,
            ids,
        } if predicate_binding == binding => Some(ids),
        GraphPredicate::And(items) => items
            .iter()
            .find_map(|item| predicate_full_text_ids(item, binding)),
        _ => None,
    }
}

fn predicate_spatial_bbox<'a>(
    predicate: &'a GraphPredicate,
    binding: &str,
) -> Option<(&'a str, GraphBoundingBox)> {
    match predicate {
        GraphPredicate::PointWithinBBox {
            binding: predicate_binding,
            property,
            min_x,
            min_y,
            max_x,
            max_y,
        } if predicate_binding == binding => {
            let bbox = GraphBoundingBox {
                min_x: graph_value_f64(min_x, "graph bbox min x").ok()?,
                min_y: graph_value_f64(min_y, "graph bbox min y").ok()?,
                max_x: graph_value_f64(max_x, "graph bbox max x").ok()?,
                max_y: graph_value_f64(max_y, "graph bbox max y").ok()?,
            };
            Some((property.as_str(), bbox))
        }
        GraphPredicate::PointDistance {
            binding: predicate_binding,
            property,
            point,
            operator,
            distance,
        } if predicate_binding == binding => {
            let distance = graph_value_f64(distance, "graph distance value").ok()?;
            distance_predicate_bbox(point, *operator, distance)
                .map(|bbox| (property.as_str(), bbox))
        }
        GraphPredicate::And(items) => items
            .iter()
            .find_map(|item| predicate_spatial_bbox(item, binding)),
        _ => None,
    }
}

fn distance_predicate_bbox(
    point: &GraphPoint,
    operator: GraphDistanceOperator,
    distance: f64,
) -> Option<GraphBoundingBox> {
    if !matches!(
        operator,
        GraphDistanceOperator::Lt | GraphDistanceOperator::Lte
    ) || !distance.is_finite()
        || distance < 0.0
    {
        return None;
    }
    match point.crs {
        GraphCrs::Cartesian2d | GraphCrs::Cartesian3d => Some(GraphBoundingBox {
            min_x: point.x - distance,
            min_y: point.y - distance,
            max_x: point.x + distance,
            max_y: point.y + distance,
        }),
        GraphCrs::Crs84_2d | GraphCrs::Crs84_3d => {
            const METERS_PER_DEGREE: f64 = 111_320.0;
            let lat_delta = distance / METERS_PER_DEGREE;
            let lon_scale = point.y.to_radians().cos().abs().max(0.000001);
            let lon_delta = distance / (METERS_PER_DEGREE * lon_scale);
            Some(GraphBoundingBox {
                min_x: (point.x - lon_delta).max(-180.0),
                min_y: (point.y - lat_delta).max(-90.0),
                max_x: (point.x + lon_delta).min(180.0),
                max_y: (point.y + lat_delta).min(90.0),
            })
        }
    }
}

fn graph_predicate_and(left: GraphPredicate, right: GraphPredicate) -> GraphPredicate {
    match (left, right) {
        (GraphPredicate::All, predicate) | (predicate, GraphPredicate::All) => predicate,
        (GraphPredicate::And(mut left), GraphPredicate::And(right)) => {
            left.extend(right);
            GraphPredicate::And(left)
        }
        (GraphPredicate::And(mut items), predicate)
        | (predicate, GraphPredicate::And(mut items)) => {
            items.push(predicate);
            GraphPredicate::And(items)
        }
        (left, right) => GraphPredicate::And(vec![left, right]),
    }
}

fn validate_regex_pattern(pattern: &str) -> Result<()> {
    if pattern.len() > GRAPH_QUERY_MAX_REGEX_BYTES {
        return Err(LoomError::new(
            Code::ResourceExhausted,
            "graph query regex pattern byte limit exceeded",
        ));
    }
    regex::Regex::new(pattern)
        .map(|_| ())
        .map_err(|error| LoomError::invalid(format!("invalid graph query regex pattern: {error}")))
}

fn eval_regex_predicate(
    bindings: &BTreeMap<String, GraphBinding<'_>>,
    binding: &str,
    property: &str,
    pattern: &str,
) -> Result<bool> {
    let Some(GraphValue::Text(value)) = binding_props(bindings, binding)?.get(property) else {
        return Ok(false);
    };
    let regex = regex::Regex::new(pattern).map_err(|error| {
        LoomError::invalid(format!("invalid graph query regex pattern: {error}"))
    })?;
    Ok(regex.is_match(value))
}

fn validate_graph_distance_value(value: &GraphValue) -> Result<()> {
    let value = graph_value_f64(value, "graph distance value")?;
    if value < 0.0 {
        return Err(LoomError::invalid(
            "graph distance value must be non-negative",
        ));
    }
    Ok(())
}

fn eval_point_distance_predicate(
    bindings: &BTreeMap<String, GraphBinding<'_>>,
    binding: &str,
    property: &str,
    point: &GraphPoint,
    operator: GraphDistanceOperator,
    distance: &GraphValue,
) -> Result<bool> {
    let Some(actual) = binding_point_property(bindings, binding, property)? else {
        return Ok(false);
    };
    let Some(actual_distance) = graph_point_distance(actual, point) else {
        return Ok(false);
    };
    let expected = graph_value_f64(distance, "graph distance value")?;
    Ok(match operator {
        GraphDistanceOperator::Lt => actual_distance < expected,
        GraphDistanceOperator::Lte => actual_distance <= expected,
        GraphDistanceOperator::Gt => actual_distance > expected,
        GraphDistanceOperator::Gte => actual_distance >= expected,
    })
}

fn eval_point_bbox_predicate(
    bindings: &BTreeMap<String, GraphBinding<'_>>,
    binding: &str,
    property: &str,
    min_x: &GraphValue,
    min_y: &GraphValue,
    max_x: &GraphValue,
    max_y: &GraphValue,
) -> Result<bool> {
    let Some(point) = binding_point_property(bindings, binding, property)? else {
        return Ok(false);
    };
    let min_x = graph_value_f64(min_x, "graph bbox min x")?;
    let min_y = graph_value_f64(min_y, "graph bbox min y")?;
    let max_x = graph_value_f64(max_x, "graph bbox max x")?;
    let max_y = graph_value_f64(max_y, "graph bbox max y")?;
    Ok((min_x..=max_x).contains(&point.x) && (min_y..=max_y).contains(&point.y))
}

fn binding_point_property<'a>(
    bindings: &'a BTreeMap<String, GraphBinding<'_>>,
    binding: &str,
    property: &str,
) -> Result<Option<&'a GraphPoint>> {
    Ok(match binding_props(bindings, binding)?.get(property) {
        Some(GraphValue::Geometry(GraphGeometry::Point(point))) => Some(point),
        Some(_) | None => None,
    })
}

fn graph_value_f64(value: &GraphValue, name: &str) -> Result<f64> {
    match value {
        GraphValue::Int(value) => Ok(*value as f64),
        GraphValue::Float(value) if value.is_finite() => Ok(*value),
        _ => Err(LoomError::invalid(format!(
            "{name} must be a finite number"
        ))),
    }
}

fn graph_point_distance(left: &GraphPoint, right: &GraphPoint) -> Option<f64> {
    if left.crs != right.crs {
        return None;
    }
    match left.crs {
        GraphCrs::Crs84_2d | GraphCrs::Crs84_3d => {
            let surface = crs84_distance_meters(left.x, left.y, right.x, right.y);
            match (left.z, right.z) {
                (Some(left_z), Some(right_z)) => {
                    Some((surface.powi(2) + (left_z - right_z).powi(2)).sqrt())
                }
                (None, None) => Some(surface),
                _ => None,
            }
        }
        GraphCrs::Cartesian2d | GraphCrs::Cartesian3d => match (left.z, right.z) {
            (Some(left_z), Some(right_z)) => Some(
                ((left.x - right.x).powi(2)
                    + (left.y - right.y).powi(2)
                    + (left_z - right_z).powi(2))
                .sqrt(),
            ),
            (None, None) => Some(((left.x - right.x).powi(2) + (left.y - right.y).powi(2)).sqrt()),
            _ => None,
        },
    }
}

fn crs84_distance_meters(left_lon: f64, left_lat: f64, right_lon: f64, right_lat: f64) -> f64 {
    const MEAN_EARTH_RADIUS_METERS: f64 = 6_371_008.8;
    let left_lat = left_lat.to_radians();
    let right_lat = right_lat.to_radians();
    let delta_lat = right_lat - left_lat;
    let delta_lon = (right_lon - left_lon).to_radians();
    let half_delta_lat = (delta_lat / 2.0).sin();
    let half_delta_lon = (delta_lon / 2.0).sin();
    let a = half_delta_lat * half_delta_lat
        + left_lat.cos() * right_lat.cos() * half_delta_lon * half_delta_lon;
    2.0 * MEAN_EARTH_RADIUS_METERS * a.sqrt().atan2((1.0 - a).sqrt())
}

fn binding_props<'a>(
    bindings: &'a BTreeMap<String, GraphBinding<'_>>,
    binding: &str,
) -> Result<&'a Props> {
    match bindings.get(binding) {
        Some(GraphBinding::Node(_, node)) => Ok(&node.props),
        Some(GraphBinding::Edge(_, edge)) => Ok(&edge.props),
        Some(GraphBinding::Path(_)) => Err(LoomError::invalid(
            "graph query path binding has no properties",
        )),
        None => Err(LoomError::invalid("graph query binding is not bound")),
    }
}

fn finish_query(
    query: &GraphQuery,
    matches: &[BTreeMap<String, GraphBinding<'_>>],
) -> Result<GraphQueryResult> {
    if query_has_aggregation(query) {
        let mut rows = aggregate_rows(query, matches)?;
        if !query.order_by.is_empty() {
            rows.sort_by(|left, right| compare_rows(query, left, right));
        }
        let skip = query.skip.unwrap_or(0);
        let limit = query.limit.unwrap_or(GRAPH_QUERY_DEFAULT_ROW_LIMIT);
        return Ok(GraphQueryResult {
            rows: rows.into_iter().skip(skip).take(limit).collect(),
        });
    }
    let mut ordered_matches = matches.to_vec();
    if !query.order_by.is_empty() {
        ordered_matches.sort_by(|left, right| compare_binding_rows(query, left, right));
    }
    let skip = query.skip.unwrap_or(0);
    let limit = query.limit.unwrap_or(GRAPH_QUERY_DEFAULT_ROW_LIMIT);
    let rows = ordered_matches
        .into_iter()
        .skip(skip)
        .take(limit)
        .map(|bindings| project_row(query, &bindings))
        .collect::<Result<_>>()?;
    Ok(GraphQueryResult { rows })
}

fn query_has_aggregation(query: &GraphQuery) -> bool {
    query
        .returns
        .iter()
        .any(|projection| matches!(projection, GraphReturn::Count { .. }))
}

#[derive(Debug)]
struct GraphAggregateState {
    row: BTreeMap<String, GraphQueryValue>,
    counts: BTreeMap<String, i64>,
}

fn aggregate_rows(
    query: &GraphQuery,
    matches: &[BTreeMap<String, GraphBinding<'_>>],
) -> Result<Vec<BTreeMap<String, GraphQueryValue>>> {
    let mut groups: BTreeMap<Vec<Vec<u8>>, GraphAggregateState> = BTreeMap::new();
    for bindings in matches {
        let base_row = project_group_row(query, bindings)?;
        let key = base_row
            .values()
            .map(query_value_key)
            .collect::<Result<_>>()?;
        let state = groups.entry(key).or_insert_with(|| GraphAggregateState {
            row: base_row,
            counts: BTreeMap::new(),
        });
        for projection in &query.returns {
            if let GraphReturn::Count { binding, alias } = projection {
                if let Some(binding) = binding
                    && !bindings.contains_key(binding)
                {
                    return Err(LoomError::invalid(
                        "graph query aggregate binding is not bound",
                    ));
                }
                *state.counts.entry(alias.clone()).or_insert(0) += 1;
            }
        }
    }
    if groups.is_empty()
        && query
            .returns
            .iter()
            .all(|projection| matches!(projection, GraphReturn::Count { .. }))
    {
        let mut row = BTreeMap::new();
        for projection in &query.returns {
            if let GraphReturn::Count { alias, .. } = projection {
                row.insert(alias.clone(), GraphQueryValue::Scalar(GraphValue::Int(0)));
            }
        }
        return Ok(vec![row]);
    }
    let mut rows = Vec::new();
    for (_, mut state) in groups {
        for (alias, count) in state.counts {
            state
                .row
                .insert(alias, GraphQueryValue::Scalar(GraphValue::Int(count)));
        }
        rows.push(state.row);
    }
    Ok(rows)
}

fn project_group_row(
    query: &GraphQuery,
    bindings: &BTreeMap<String, GraphBinding<'_>>,
) -> Result<BTreeMap<String, GraphQueryValue>> {
    let mut row = BTreeMap::new();
    for projection in &query.returns {
        match projection {
            GraphReturn::Binding(binding) => {
                let value = bindings
                    .get(binding)
                    .map(binding_value)
                    .ok_or_else(|| LoomError::invalid("graph query return binding is not bound"))?;
                row.insert(binding.clone(), value);
            }
            GraphReturn::Property { binding, property } => {
                let value = binding_props(bindings, binding)?
                    .get(property)
                    .cloned()
                    .map(GraphQueryValue::Scalar)
                    .unwrap_or(GraphQueryValue::Null);
                row.insert(format!("{binding}.{property}"), value);
            }
            GraphReturn::Count { .. } => {}
            GraphReturn::PathLength { binding, alias } => {
                let value = path_length_value(bindings, binding)?;
                row.insert(alias.clone(), value);
            }
            GraphReturn::Function { function, alias } => {
                let value = graph_function_value(bindings, function)?;
                row.insert(alias.clone(), value);
            }
        }
    }
    Ok(row)
}

fn project_row(
    query: &GraphQuery,
    bindings: &BTreeMap<String, GraphBinding<'_>>,
) -> Result<BTreeMap<String, GraphQueryValue>> {
    let mut row = BTreeMap::new();
    if query.returns.is_empty() {
        for (name, binding) in bindings {
            row.insert(name.clone(), binding_value(binding));
        }
        return Ok(row);
    }
    for projection in &query.returns {
        match projection {
            GraphReturn::Binding(binding) => {
                let value = bindings
                    .get(binding)
                    .map(binding_value)
                    .ok_or_else(|| LoomError::invalid("graph query return binding is not bound"))?;
                row.insert(binding.clone(), value);
            }
            GraphReturn::Property { binding, property } => {
                let value = binding_props(bindings, binding)?
                    .get(property)
                    .cloned()
                    .map(GraphQueryValue::Scalar)
                    .unwrap_or(GraphQueryValue::Null);
                row.insert(format!("{binding}.{property}"), value);
            }
            GraphReturn::Count { .. } => {
                return Err(LoomError::invalid(
                    "graph query aggregate requires aggregate projection",
                ));
            }
            GraphReturn::PathLength { binding, alias } => {
                let value = path_length_value(bindings, binding)?;
                row.insert(alias.clone(), value);
            }
            GraphReturn::Function { function, alias } => {
                let value = graph_function_value(bindings, function)?;
                row.insert(alias.clone(), value);
            }
        }
    }
    Ok(row)
}

fn compare_rows(
    query: &GraphQuery,
    left: &BTreeMap<String, GraphQueryValue>,
    right: &BTreeMap<String, GraphQueryValue>,
) -> Ordering {
    for order in &query.order_by {
        let left_value = order_row_value(query, left, &order.item);
        let right_value = order_row_value(query, right, &order.item);
        let ordering = compare_query_values(left_value, right_value);
        let ordering = match order.direction {
            GraphOrderDirection::Asc => ordering,
            GraphOrderDirection::Desc => ordering.reverse(),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    row_key(left).cmp(&row_key(right))
}

fn compare_binding_rows(
    query: &GraphQuery,
    left: &BTreeMap<String, GraphBinding<'_>>,
    right: &BTreeMap<String, GraphBinding<'_>>,
) -> Ordering {
    for order in &query.order_by {
        let left_value = order_binding_value(left, &order.item);
        let right_value = order_binding_value(right, &order.item);
        let ordering = compare_query_values(left_value.as_ref(), right_value.as_ref());
        let ordering = match order.direction {
            GraphOrderDirection::Asc => ordering,
            GraphOrderDirection::Desc => ordering.reverse(),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    binding_row_key(left).cmp(&binding_row_key(right))
}

fn order_row_value<'a>(
    query: &GraphQuery,
    row: &'a BTreeMap<String, GraphQueryValue>,
    item: &GraphOrderItem,
) -> Option<&'a GraphQueryValue> {
    match item {
        GraphOrderItem::Binding(binding) | GraphOrderItem::ReturnKey(binding) => row.get(binding),
        GraphOrderItem::Property { binding, property } => row.get(&format!("{binding}.{property}")),
        GraphOrderItem::PathLength(binding) => row.get(&format!("length({binding})")),
        GraphOrderItem::Function(function) => row.get(&graph_function_alias(function)),
    }
    .or_else(|| match item {
        GraphOrderItem::ReturnKey(key) => query.returns.iter().find_map(|projection| {
            if let GraphReturn::Count { binding, alias } = projection
                && count_default_alias(binding.as_deref()) == *key
            {
                return row.get(alias);
            }
            None
        }),
        GraphOrderItem::PathLength(binding) => query.returns.iter().find_map(|projection| {
            if let GraphReturn::PathLength {
                binding: projection_binding,
                alias,
            } = projection
                && projection_binding == binding
            {
                return row.get(alias);
            }
            None
        }),
        GraphOrderItem::Function(function) => query.returns.iter().find_map(|projection| {
            if let GraphReturn::Function {
                function: projection_function,
                alias,
            } = projection
                && projection_function == function
            {
                return row.get(alias);
            }
            None
        }),
        _ => None,
    })
}

fn order_binding_value(
    bindings: &BTreeMap<String, GraphBinding<'_>>,
    item: &GraphOrderItem,
) -> Option<GraphQueryValue> {
    match item {
        GraphOrderItem::Binding(binding) => bindings.get(binding).map(binding_value),
        GraphOrderItem::PathLength(binding) => path_length_value(bindings, binding).ok(),
        GraphOrderItem::Function(function) => graph_function_value(bindings, function).ok(),
        GraphOrderItem::Property { binding, property } => binding_props(bindings, binding)
            .ok()
            .and_then(|props| props.get(property).cloned())
            .map(GraphQueryValue::Scalar)
            .or(Some(GraphQueryValue::Null)),
        GraphOrderItem::ReturnKey(_) => None,
    }
}

fn path_length_value(
    bindings: &BTreeMap<String, GraphBinding<'_>>,
    binding: &str,
) -> Result<GraphQueryValue> {
    match bindings.get(binding) {
        Some(GraphBinding::Path(path)) => Ok(GraphQueryValue::Scalar(GraphValue::Int(
            i64::try_from(path.edges.len())
                .map_err(|_| LoomError::invalid("graph path length exceeds i64"))?,
        ))),
        Some(GraphBinding::Node(_, _)) | Some(GraphBinding::Edge(_, _)) => Err(LoomError::invalid(
            "graph query length function requires a path binding",
        )),
        None => Err(LoomError::invalid(
            "graph query length binding is not bound",
        )),
    }
}

fn graph_function_alias(function: &GraphFunction) -> String {
    match function {
        GraphFunction::Id { binding } => format!("id({binding})"),
        GraphFunction::Type { binding } => format!("type({binding})"),
        GraphFunction::StartNode { binding } => format!("startNode({binding})"),
        GraphFunction::EndNode { binding } => format!("endNode({binding})"),
        GraphFunction::Labels { binding } => format!("labels({binding})"),
        GraphFunction::Keys { binding } => format!("keys({binding})"),
        GraphFunction::Properties { binding } => format!("properties({binding})"),
        GraphFunction::Nodes { binding } => format!("nodes({binding})"),
        GraphFunction::Relationships { binding } => format!("relationships({binding})"),
    }
}

fn graph_function_value(
    bindings: &BTreeMap<String, GraphBinding<'_>>,
    function: &GraphFunction,
) -> Result<GraphQueryValue> {
    match function {
        GraphFunction::Id { binding } => match bindings.get(binding) {
            Some(GraphBinding::Node(id, _)) | Some(GraphBinding::Edge(id, _)) => {
                Ok(GraphQueryValue::Scalar(GraphValue::Text((*id).to_string())))
            }
            Some(GraphBinding::Path(path)) => Ok(GraphQueryValue::Scalar(GraphValue::Text(
                hex::encode(path_key(path)),
            ))),
            None => Err(LoomError::invalid(
                "graph query function binding is not bound",
            )),
        },
        GraphFunction::Type { binding } => match bindings.get(binding) {
            Some(GraphBinding::Edge(_, edge)) => Ok(GraphQueryValue::Scalar(GraphValue::Text(
                edge.label.clone(),
            ))),
            Some(GraphBinding::Node(_, _)) | Some(GraphBinding::Path(_)) => Err(
                LoomError::invalid("graph query type function requires an edge binding"),
            ),
            None => Err(LoomError::invalid(
                "graph query function binding is not bound",
            )),
        },
        GraphFunction::StartNode { binding } => match bindings.get(binding) {
            Some(GraphBinding::Edge(_, edge)) => bound_node_value(bindings, &edge.src),
            Some(GraphBinding::Node(_, _)) | Some(GraphBinding::Path(_)) => Err(
                LoomError::invalid("graph query startNode function requires an edge binding"),
            ),
            None => Err(LoomError::invalid(
                "graph query function binding is not bound",
            )),
        },
        GraphFunction::EndNode { binding } => match bindings.get(binding) {
            Some(GraphBinding::Edge(_, edge)) => bound_node_value(bindings, &edge.dst),
            Some(GraphBinding::Node(_, _)) | Some(GraphBinding::Path(_)) => Err(
                LoomError::invalid("graph query endNode function requires an edge binding"),
            ),
            None => Err(LoomError::invalid(
                "graph query function binding is not bound",
            )),
        },
        GraphFunction::Labels { binding } => match bindings.get(binding) {
            Some(GraphBinding::Node(_, node)) => Ok(GraphQueryValue::List(
                node.labels
                    .iter()
                    .cloned()
                    .map(GraphValue::Text)
                    .map(GraphQueryValue::Scalar)
                    .collect(),
            )),
            Some(GraphBinding::Edge(_, _)) | Some(GraphBinding::Path(_)) => Err(
                LoomError::invalid("graph query labels function requires a node binding"),
            ),
            None => Err(LoomError::invalid(
                "graph query function binding is not bound",
            )),
        },
        GraphFunction::Keys { binding } => {
            let props = binding_props(bindings, binding)?;
            Ok(GraphQueryValue::List(
                props
                    .keys()
                    .cloned()
                    .map(GraphValue::Text)
                    .map(GraphQueryValue::Scalar)
                    .collect(),
            ))
        }
        GraphFunction::Properties { binding } => {
            let props = binding_props(bindings, binding)?;
            Ok(GraphQueryValue::Map(
                props
                    .iter()
                    .map(|(key, value)| (key.clone(), GraphQueryValue::Scalar(value.clone())))
                    .collect(),
            ))
        }
        GraphFunction::Nodes { binding } => match bindings.get(binding) {
            Some(GraphBinding::Path(path)) => Ok(GraphQueryValue::List(
                path.nodes
                    .iter()
                    .cloned()
                    .map(GraphQueryValue::Node)
                    .collect(),
            )),
            Some(GraphBinding::Node(_, _)) | Some(GraphBinding::Edge(_, _)) => Err(
                LoomError::invalid("graph query nodes function requires a path binding"),
            ),
            None => Err(LoomError::invalid(
                "graph query function binding is not bound",
            )),
        },
        GraphFunction::Relationships { binding } => match bindings.get(binding) {
            Some(GraphBinding::Path(path)) => Ok(GraphQueryValue::List(
                path.edges
                    .iter()
                    .cloned()
                    .map(GraphQueryValue::Edge)
                    .collect(),
            )),
            Some(GraphBinding::Node(_, _)) | Some(GraphBinding::Edge(_, _)) => Err(
                LoomError::invalid("graph query relationships function requires a path binding"),
            ),
            None => Err(LoomError::invalid(
                "graph query function binding is not bound",
            )),
        },
    }
}

fn bound_node_value(
    bindings: &BTreeMap<String, GraphBinding<'_>>,
    node_id: &str,
) -> Result<GraphQueryValue> {
    bindings
        .values()
        .find_map(|binding| match binding {
            GraphBinding::Node(id, node) if *id == node_id => {
                Some(GraphQueryValue::Node(query_node_value(id, node)))
            }
            _ => None,
        })
        .ok_or_else(|| LoomError::invalid("graph query endpoint node is not bound"))
}

fn compare_query_values(
    left: Option<&GraphQueryValue>,
    right: Option<&GraphQueryValue>,
) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left), Some(right)) => match (left, right) {
            (GraphQueryValue::Null, GraphQueryValue::Null) => Ordering::Equal,
            (GraphQueryValue::Null, _) => Ordering::Less,
            (_, GraphQueryValue::Null) => Ordering::Greater,
            (GraphQueryValue::Scalar(left), GraphQueryValue::Scalar(right)) => {
                sort_graph_values(left, right)
            }
            (GraphQueryValue::Node(left), GraphQueryValue::Node(right)) => left.id.cmp(&right.id),
            (GraphQueryValue::Edge(left), GraphQueryValue::Edge(right)) => left.id.cmp(&right.id),
            (GraphQueryValue::Path(left), GraphQueryValue::Path(right)) => {
                path_key(left).cmp(&path_key(right))
            }
            (GraphQueryValue::List(left), GraphQueryValue::List(right)) => left
                .iter()
                .zip(right)
                .map(|(left, right)| compare_query_values(Some(left), Some(right)))
                .find(|ordering| *ordering != Ordering::Equal)
                .unwrap_or_else(|| left.len().cmp(&right.len())),
            (GraphQueryValue::Map(left), GraphQueryValue::Map(right)) => left
                .iter()
                .zip(right)
                .map(|((left_key, left_value), (right_key, right_value))| {
                    left_key
                        .cmp(right_key)
                        .then_with(|| compare_query_values(Some(left_value), Some(right_value)))
                })
                .find(|ordering| *ordering != Ordering::Equal)
                .unwrap_or_else(|| left.len().cmp(&right.len())),
            _ => query_value_rank(left).cmp(&query_value_rank(right)),
        },
    }
}

fn compare_graph_values(left: &GraphValue, right: &GraphValue) -> Option<Ordering> {
    match (left, right) {
        (GraphValue::Null, GraphValue::Null) => Some(Ordering::Equal),
        (GraphValue::Bool(left), GraphValue::Bool(right)) => Some(left.cmp(right)),
        (GraphValue::Int(left), GraphValue::Int(right)) => Some(left.cmp(right)),
        (GraphValue::Int(left), GraphValue::Float(right)) => (*left as f64).partial_cmp(right),
        (GraphValue::Float(left), GraphValue::Int(right)) => left.partial_cmp(&(*right as f64)),
        (GraphValue::Float(left), GraphValue::Float(right)) => left.partial_cmp(right),
        (GraphValue::Text(left), GraphValue::Text(right)) => Some(left.cmp(right)),
        (GraphValue::Bytes(left), GraphValue::Bytes(right)) => Some(left.cmp(right)),
        (GraphValue::List(left), GraphValue::List(right)) => Some(
            left.iter()
                .zip(right)
                .map(|(left, right)| sort_graph_values(left, right))
                .find(|ordering| *ordering != Ordering::Equal)
                .unwrap_or_else(|| left.len().cmp(&right.len())),
        ),
        (GraphValue::Map(left), GraphValue::Map(right)) => Some(
            left.iter()
                .zip(right)
                .map(|((left_key, left_value), (right_key, right_value))| {
                    left_key
                        .cmp(right_key)
                        .then_with(|| sort_graph_values(left_value, right_value))
                })
                .find(|ordering| *ordering != Ordering::Equal)
                .unwrap_or_else(|| left.len().cmp(&right.len())),
        ),
        (GraphValue::Geometry(left), GraphValue::Geometry(right)) => {
            compare_graph_geometries(left, right)
        }
        _ => None,
    }
}

fn compare_graph_geometries(left: &GraphGeometry, right: &GraphGeometry) -> Option<Ordering> {
    match (left, right) {
        (GraphGeometry::Point(left), GraphGeometry::Point(right)) => Some(left.crs.cmp(&right.crs))
            .filter(|ordering| *ordering != Ordering::Equal)
            .or_else(|| left.x.partial_cmp(&right.x))
            .filter(|ordering| *ordering != Ordering::Equal)
            .or_else(|| left.y.partial_cmp(&right.y))
            .filter(|ordering| *ordering != Ordering::Equal)
            .or_else(|| match (left.z, right.z) {
                (None, None) => Some(Ordering::Equal),
                (None, Some(_)) => Some(Ordering::Less),
                (Some(_), None) => Some(Ordering::Greater),
                (Some(left), Some(right)) => left.partial_cmp(&right),
            }),
    }
}

fn sort_graph_values(left: &GraphValue, right: &GraphValue) -> Ordering {
    compare_graph_values(left, right)
        .unwrap_or_else(|| graph_value_rank(left).cmp(&graph_value_rank(right)))
}

fn graph_value_rank(value: &GraphValue) -> u8 {
    match value {
        GraphValue::Null => 0,
        GraphValue::Bool(_) => 1,
        GraphValue::Int(_) | GraphValue::Float(_) => 2,
        GraphValue::Text(_) => 3,
        GraphValue::Bytes(_) => 4,
        GraphValue::List(_) => 5,
        GraphValue::Map(_) => 6,
        GraphValue::Geometry(_) => 7,
    }
}

fn query_value_rank(value: &GraphQueryValue) -> u8 {
    match value {
        GraphQueryValue::Null => 0,
        GraphQueryValue::Scalar(_) => 1,
        GraphQueryValue::Node(_) => 2,
        GraphQueryValue::Edge(_) => 3,
        GraphQueryValue::Path(_) => 4,
        GraphQueryValue::List(_) => 5,
        GraphQueryValue::Map(_) => 6,
    }
}

fn query_value_key(value: &GraphQueryValue) -> Result<Vec<u8>> {
    Ok(cbor::encode(&query_value_cbor(value)))
}

fn row_key(row: &BTreeMap<String, GraphQueryValue>) -> Vec<u8> {
    cbor::encode(&Value::Array(
        row.iter()
            .map(|(key, value)| {
                Value::Array(vec![Value::Text(key.clone()), query_value_cbor(value)])
            })
            .collect(),
    ))
}

fn binding_row_key(row: &BTreeMap<String, GraphBinding<'_>>) -> Vec<u8> {
    cbor::encode(&Value::Array(
        row.iter()
            .map(|(key, value)| {
                Value::Array(vec![
                    Value::Text(key.clone()),
                    query_value_cbor(&binding_value(value)),
                ])
            })
            .collect(),
    ))
}

fn path_key(path: &GraphPath) -> Vec<u8> {
    cbor::encode(&query_value_cbor(&GraphQueryValue::Path(path.clone())))
}

fn graph_index_value_key(value: &GraphValue) -> Result<Vec<u8>> {
    value.validate()?;
    Ok(cbor::encode(&Value::Array(vec![
        Value::Text("graph-property-index-v1".to_string()),
        value.to_cbor(),
    ])))
}

fn validate_path_value_budget(path: &GraphPath) -> Result<()> {
    if path_key(path).len() > GRAPH_QUERY_MAX_PATH_BYTES {
        return Err(LoomError::new(
            Code::ResourceExhausted,
            "graph query path byte limit exceeded",
        ));
    }
    Ok(())
}

fn query_value_cbor(value: &GraphQueryValue) -> Value {
    match value {
        GraphQueryValue::Null => Value::Array(vec![Value::Text("null".to_string())]),
        GraphQueryValue::Scalar(value) => {
            Value::Array(vec![Value::Text("scalar".to_string()), value.to_cbor()])
        }
        GraphQueryValue::Node(node) => Value::Array(vec![
            Value::Text("node".to_string()),
            Value::Text(node.id.clone()),
            labels_value(&node.labels),
            props_value(&node.props),
        ]),
        GraphQueryValue::Edge(edge) => Value::Array(vec![
            Value::Text("edge".to_string()),
            Value::Text(edge.id.clone()),
            Value::Text(edge.src.clone()),
            Value::Text(edge.dst.clone()),
            Value::Text(edge.label.clone()),
            props_value(&edge.props),
        ]),
        GraphQueryValue::Path(path) => Value::Array(vec![
            Value::Text("path".to_string()),
            Value::Array(
                path.nodes
                    .iter()
                    .map(|node| query_value_cbor(&GraphQueryValue::Node(node.clone())))
                    .collect(),
            ),
            Value::Array(
                path.edges
                    .iter()
                    .map(|edge| query_value_cbor(&GraphQueryValue::Edge(edge.clone())))
                    .collect(),
            ),
        ]),
        GraphQueryValue::List(values) => Value::Array(vec![
            Value::Text("list".to_string()),
            Value::Array(values.iter().map(query_value_cbor).collect()),
        ]),
        GraphQueryValue::Map(values) => Value::Array(vec![
            Value::Text("map".to_string()),
            Value::Map(
                values
                    .iter()
                    .map(|(key, value)| (Value::Text(key.clone()), query_value_cbor(value)))
                    .collect(),
            ),
        ]),
    }
}

fn binding_value(binding: &GraphBinding<'_>) -> GraphQueryValue {
    match binding {
        GraphBinding::Node(id, node) => GraphQueryValue::Node(GraphQueryNode {
            id: (*id).to_string(),
            labels: node.labels.clone(),
            props: node.props.clone(),
        }),
        GraphBinding::Edge(id, edge) => GraphQueryValue::Edge(GraphQueryEdge {
            id: (*id).to_string(),
            src: edge.src.clone(),
            dst: edge.dst.clone(),
            label: edge.label.clone(),
            props: edge.props.clone(),
        }),
        GraphBinding::Path(path) => GraphQueryValue::Path(path.clone()),
    }
}

fn query_node_value(id: &str, node: &Node) -> GraphQueryNode {
    GraphQueryNode {
        id: id.to_string(),
        labels: node.labels.clone(),
        props: node.props.clone(),
    }
}

fn query_edge_value(id: &str, edge: &Edge) -> GraphQueryEdge {
    GraphQueryEdge {
        id: id.to_string(),
        src: edge.src.clone(),
        dst: edge.dst.clone(),
        label: edge.label.clone(),
        props: edge.props.clone(),
    }
}

fn graph_path(name: &str) -> String {
    facet_path(FacetKind::Graph, name)
}

const GRAPH_QUERY_DEFAULT_ROW_LIMIT: usize = 1024;
const GRAPH_QUERY_DEFAULT_MATCH_LIMIT: usize = 4096;
const GRAPH_QUERY_MAX_PATH_HOPS: usize = 8;
const GRAPH_QUERY_MAX_PATH_FANOUT: usize = 128;
const GRAPH_QUERY_MAX_PATH_CANDIDATES: usize = 4096;
const GRAPH_QUERY_MAX_PATH_BYTES: usize = 1024 * 1024;
const GRAPH_QUERY_MAX_REGEX_BYTES: usize = 4096;
const GRAPH_META_ENTRY: &str = "metadata";
const GRAPH_INDEX_CATALOG_ENTRY: &str = "property_index_catalog";
const GRAPH_SPATIAL_INDEX_CATALOG_ENTRY: &str = "spatial_index_catalog";
const GRAPH_NODES_ENTRY: &str = "nodes";
const GRAPH_EDGES_ENTRY: &str = "edges";
const GRAPH_FORWARD_ADJ_ENTRY: &str = "forward_adjacency";
const GRAPH_REVERSE_ADJ_ENTRY: &str = "reverse_adjacency";

/// Stage `graph` under `name` in `ns`'s graph facet as a structured graph root; `commit` snapshots it.
pub fn put_graph<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    graph: &Graph,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Write)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Graph), true)?;
    stage_graph_reserved(loom, ns, &graph_path(name), graph)
}

/// Load the graph named `name` from `ns`'s current working tree, or `NOT_FOUND`.
pub fn get_graph<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId, name: &str) -> Result<Graph> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    read_graph_reserved(loom, ns, &graph_path(name))
}

/// Load the graph named `name`, or an empty graph when it does not exist yet. Facade mutations create
/// the graph on first write; facade reads of a missing graph return absent rather than erroring.
fn load_or_new<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId, name: &str) -> Result<Graph> {
    match read_graph_reserved(loom, ns, &graph_path(name)) {
        Ok(graph) => Ok(graph),
        Err(error) if error.code == Code::NotFound => Ok(Graph::new()),
        Err(error) => Err(error),
    }
}

fn stage_graph_reserved<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    path: &str,
    graph: &Graph,
) -> Result<()> {
    let path = normalize_path(path)?;
    let metadata_addr = loom.store_content(ns, &graph_metadata())?;
    let catalog_addr = loom.store_content(ns, &property_index_catalog(&graph.property_indexes)?)?;
    let spatial_catalog_addr =
        loom.store_content(ns, &spatial_index_catalog(&graph.spatial_indexes)?)?;
    let nodes_root = graph_nodes_root(loom, graph)?;
    let edges_root = graph_edges_root(loom, graph)?;
    let forward_root = graph_forward_adjacency_root(loom, graph)?;
    let reverse_root = graph_reverse_adjacency_root(loom, graph)?;
    let root = build_graph_root_tree(
        loom,
        metadata_addr,
        catalog_addr,
        spatial_catalog_addr,
        nodes_root,
        edges_root,
        forward_root,
        reverse_root,
    )?;
    loom.work
        .entry(ns)
        .or_default()
        .insert(path, StagedEntry::Graph(root));
    Ok(())
}

fn read_graph_reserved<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    path: &str,
) -> Result<Graph> {
    let path = normalize_path(path)?;
    let root = match loom.work.get(&ns).and_then(|work| work.get(&path)) {
        Some(StagedEntry::Graph(root)) => *root,
        Some(_) => return Err(LoomError::invalid(format!("{path:?} is not a graph"))),
        None => return Err(LoomError::not_found(format!("graph {path:?} not staged"))),
    };
    graph_from_root(loom, root)
}

fn graph_from_root<S: ObjectStore>(loom: &Loom<S>, root: crate::Digest) -> Result<Graph> {
    let Object::Tree(entries) = loom.get_object(&root)? else {
        return Err(LoomError::corrupt("graph root is not a Tree"));
    };
    let mut metadata = None;
    let mut nodes = None;
    let mut edges = None;
    let mut catalog = None;
    let mut spatial_catalog = None;
    for entry in entries {
        match entry.name.as_str() {
            GRAPH_META_ENTRY if entry.kind == EntryKind::Blob => metadata = Some(entry.target),
            GRAPH_INDEX_CATALOG_ENTRY if entry.kind == EntryKind::Blob => {
                catalog = Some(entry.target)
            }
            GRAPH_SPATIAL_INDEX_CATALOG_ENTRY if entry.kind == EntryKind::Blob => {
                spatial_catalog = Some(entry.target)
            }
            GRAPH_NODES_ENTRY if entry.kind == EntryKind::ProllyMap => nodes = Some(entry.target),
            GRAPH_EDGES_ENTRY if entry.kind == EntryKind::ProllyMap => edges = Some(entry.target),
            GRAPH_FORWARD_ADJ_ENTRY | GRAPH_REVERSE_ADJ_ENTRY
                if entry.kind == EntryKind::ProllyMap => {}
            _ => return Err(LoomError::corrupt("invalid graph root entry")),
        }
    }
    decode_graph_metadata(&loom.load_content(
        metadata.ok_or_else(|| LoomError::corrupt("graph root has no metadata"))?,
    )?)?;
    let catalog =
        catalog.ok_or_else(|| LoomError::corrupt("graph root has no property index catalog"))?;
    let property_indexes = decode_property_index_catalog(&loom.load_content(catalog)?)?;
    let spatial_indexes = match spatial_catalog {
        Some(catalog) => decode_spatial_index_catalog(&loom.load_content(catalog)?)?,
        None => BTreeMap::new(),
    };
    let mut graph = Graph::new();
    graph.property_indexes = property_indexes;
    graph.spatial_indexes = spatial_indexes;
    if let Some(root) = nodes {
        for (key, value) in crate::prolly::entries(loom.store(), &root)? {
            graph
                .nodes
                .insert(node_id_from_key(&key)?, node_from_bytes(&value)?);
        }
    }
    if let Some(root) = edges {
        for (key, value) in crate::prolly::entries(loom.store(), &root)? {
            graph
                .edges
                .insert(edge_id_from_key(&key)?, edge_from_bytes(&value)?);
        }
    }
    Ok(graph)
}

fn build_graph_root_tree<S: ObjectStore>(
    loom: &mut Loom<S>,
    metadata_addr: crate::Digest,
    catalog_addr: crate::Digest,
    spatial_catalog_addr: crate::Digest,
    nodes_root: Option<crate::Digest>,
    edges_root: Option<crate::Digest>,
    forward_root: Option<crate::Digest>,
    reverse_root: Option<crate::Digest>,
) -> Result<crate::Digest> {
    let mut entries = vec![
        TreeEntry {
            name: GRAPH_META_ENTRY.to_string(),
            kind: EntryKind::Blob,
            target: metadata_addr,
            mode: 0,
        },
        TreeEntry {
            name: GRAPH_INDEX_CATALOG_ENTRY.to_string(),
            kind: EntryKind::Blob,
            target: catalog_addr,
            mode: 0,
        },
        TreeEntry {
            name: GRAPH_SPATIAL_INDEX_CATALOG_ENTRY.to_string(),
            kind: EntryKind::Blob,
            target: spatial_catalog_addr,
            mode: 0,
        },
    ];
    for (name, root) in [
        (GRAPH_NODES_ENTRY, nodes_root),
        (GRAPH_EDGES_ENTRY, edges_root),
        (GRAPH_FORWARD_ADJ_ENTRY, forward_root),
        (GRAPH_REVERSE_ADJ_ENTRY, reverse_root),
    ] {
        if let Some(target) = root {
            entries.push(TreeEntry {
                name: name.to_string(),
                kind: EntryKind::ProllyMap,
                target,
                mode: 0,
            });
        }
    }
    loom.put_object(&Object::tree(entries)?)
}

fn graph_nodes_root<S: ObjectStore>(
    loom: &mut Loom<S>,
    graph: &Graph,
) -> Result<Option<crate::Digest>> {
    let entries = graph
        .nodes
        .iter()
        .map(|(id, node)| Ok((node_key(id)?, node_bytes(node))))
        .collect::<Result<Vec<_>>>()?;
    crate::prolly::build(loom.store_mut(), &entries)
}

fn graph_edges_root<S: ObjectStore>(
    loom: &mut Loom<S>,
    graph: &Graph,
) -> Result<Option<crate::Digest>> {
    let entries = graph
        .edges
        .iter()
        .map(|(id, edge)| Ok((edge_key(id)?, edge_bytes(edge))))
        .collect::<Result<Vec<_>>>()?;
    crate::prolly::build(loom.store_mut(), &entries)
}

fn graph_forward_adjacency_root<S: ObjectStore>(
    loom: &mut Loom<S>,
    graph: &Graph,
) -> Result<Option<crate::Digest>> {
    let entries = graph
        .edges
        .iter()
        .map(|(id, edge)| {
            Ok((
                compound_key([
                    edge.src.as_str(),
                    edge.label.as_str(),
                    edge.dst.as_str(),
                    id,
                ])?,
                Vec::new(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    crate::prolly::build(loom.store_mut(), &entries)
}

fn graph_reverse_adjacency_root<S: ObjectStore>(
    loom: &mut Loom<S>,
    graph: &Graph,
) -> Result<Option<crate::Digest>> {
    let entries = graph
        .edges
        .iter()
        .map(|(id, edge)| {
            Ok((
                compound_key([
                    edge.dst.as_str(),
                    edge.label.as_str(),
                    edge.src.as_str(),
                    id,
                ])?,
                Vec::new(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    crate::prolly::build(loom.store_mut(), &entries)
}

fn graph_metadata() -> Vec<u8> {
    cbor::encode(&Value::Array(vec![
        Value::Uint(1),
        Value::Text("typed-labeled-property-graph".to_string()),
        Value::Text("utf8-id".to_string()),
        Value::Text("length-prefixed-adjacency-v1".to_string()),
        Value::Text("node-edge-merge-v1".to_string()),
        Value::Text("spatial-index-catalog-v1".to_string()),
    ]))
}

fn decode_graph_metadata(bytes: &[u8]) -> Result<()> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let version = fields.uint()?;
    let semantic_schema = fields.text()?;
    let key_codec = fields.text()?;
    let adjacency_codec = fields.text()?;
    let merge_rules = fields.text()?;
    let spatial_catalog = fields.text()?;
    fields.end()?;
    if version == 1
        && semantic_schema == "typed-labeled-property-graph"
        && key_codec == "utf8-id"
        && adjacency_codec == "length-prefixed-adjacency-v1"
        && merge_rules == "node-edge-merge-v1"
        && spatial_catalog == "spatial-index-catalog-v1"
    {
        Ok(())
    } else {
        Err(LoomError::corrupt("unsupported graph metadata"))
    }
}

#[cfg(test)]
fn empty_property_index_catalog() -> Vec<u8> {
    cbor::encode(&Value::Array(vec![Value::Uint(1), Value::Array(vec![])]))
}

#[cfg(test)]
fn empty_spatial_index_catalog() -> Vec<u8> {
    cbor::encode(&Value::Array(vec![Value::Uint(1), Value::Array(vec![])]))
}

fn property_index_catalog(indexes: &BTreeMap<String, GraphPropertyIndex>) -> Result<Vec<u8>> {
    let entries = indexes
        .values()
        .map(|index| {
            validate_property_index(index)?;
            Ok(Value::Array(vec![
                Value::Text(index.name.clone()),
                Value::Text(index_entity_name(index.entity).to_string()),
                Value::Text(index.property.clone()),
            ]))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(cbor::encode(&Value::Array(vec![
        Value::Uint(1),
        Value::Array(entries),
    ])))
}

fn decode_property_index_catalog(bytes: &[u8]) -> Result<BTreeMap<String, GraphPropertyIndex>> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let version = fields.uint()?;
    let indexes = fields.array()?;
    fields.end()?;
    if version != 1 {
        return Err(LoomError::corrupt(
            "unsupported graph property index catalog",
        ));
    }
    let mut out = BTreeMap::new();
    for item in indexes {
        let mut fields = cbor::Fields::new(cbor::as_array(item)?);
        let name = fields.text()?;
        let entity = parse_index_entity(&fields.text()?)?;
        let property = fields.text()?;
        fields.end()?;
        let index = GraphPropertyIndex {
            name: name.clone(),
            entity,
            property,
        };
        validate_property_index(&index)?;
        if out.insert(name, index).is_some() {
            return Err(LoomError::corrupt(
                "duplicate graph property index declaration",
            ));
        }
    }
    Ok(out)
}

fn validate_property_index(index: &GraphPropertyIndex) -> Result<()> {
    validate_graph_id(&index.name)?;
    validate_property_name(&index.property)
}

fn spatial_index_catalog(indexes: &BTreeMap<String, GraphSpatialIndex>) -> Result<Vec<u8>> {
    let entries = indexes
        .values()
        .map(|index| {
            validate_spatial_index(index)?;
            Ok(Value::Array(vec![
                Value::Text(index.name.clone()),
                Value::Text(index_entity_name(index.entity).to_string()),
                Value::Text(index.property.clone()),
            ]))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(cbor::encode(&Value::Array(vec![
        Value::Uint(1),
        Value::Array(entries),
    ])))
}

fn decode_spatial_index_catalog(bytes: &[u8]) -> Result<BTreeMap<String, GraphSpatialIndex>> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let version = fields.uint()?;
    let indexes = fields.array()?;
    fields.end()?;
    if version != 1 {
        return Err(LoomError::corrupt(
            "unsupported graph spatial index catalog",
        ));
    }
    let mut out = BTreeMap::new();
    for item in indexes {
        let mut fields = cbor::Fields::new(cbor::as_array(item)?);
        let name = fields.text()?;
        let entity = parse_index_entity(&fields.text()?)?;
        let property = fields.text()?;
        fields.end()?;
        let index = GraphSpatialIndex {
            name: name.clone(),
            entity,
            property,
        };
        validate_spatial_index(&index)?;
        if out.insert(name, index).is_some() {
            return Err(LoomError::corrupt(
                "duplicate graph spatial index declaration",
            ));
        }
    }
    Ok(out)
}

fn validate_spatial_index(index: &GraphSpatialIndex) -> Result<()> {
    validate_graph_id(&index.name)?;
    validate_property_name(&index.property)
}

fn index_entity_name(entity: GraphIndexEntity) -> &'static str {
    match entity {
        GraphIndexEntity::Node => "node",
        GraphIndexEntity::Edge => "edge",
    }
}

fn parse_index_entity(entity: &str) -> Result<GraphIndexEntity> {
    match entity {
        "node" => Ok(GraphIndexEntity::Node),
        "edge" => Ok(GraphIndexEntity::Edge),
        _ => Err(LoomError::corrupt("invalid graph property index entity")),
    }
}

fn node_key(id: &str) -> Result<Vec<u8>> {
    validate_graph_id(id)?;
    Ok(id.as_bytes().to_vec())
}

fn edge_key(id: &str) -> Result<Vec<u8>> {
    validate_graph_id(id)?;
    Ok(id.as_bytes().to_vec())
}

fn node_id_from_key(key: &[u8]) -> Result<String> {
    String::from_utf8(key.to_vec()).map_err(|_| LoomError::corrupt("graph node key is not utf8"))
}

fn edge_id_from_key(key: &[u8]) -> Result<String> {
    String::from_utf8(key.to_vec()).map_err(|_| LoomError::corrupt("graph edge key is not utf8"))
}

fn validate_graph_id(id: &str) -> Result<()> {
    if id.is_empty() {
        Err(LoomError::invalid("graph ids must be non-empty"))
    } else {
        Ok(())
    }
}

fn validate_label(label: &str) -> Result<()> {
    if label.is_empty() {
        Err(LoomError::invalid("graph labels must be non-empty"))
    } else {
        Ok(())
    }
}

fn validate_property_name(property: &str) -> Result<()> {
    if property.is_empty() {
        Err(LoomError::invalid("graph property names must be non-empty"))
    } else {
        Ok(())
    }
}

fn validate_labels(labels: &BTreeSet<String>) -> Result<()> {
    for label in labels {
        validate_label(label)?;
    }
    Ok(())
}

fn compound_key<const N: usize>(parts: [&str; N]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for part in parts {
        let bytes = part.as_bytes();
        let len = u32::try_from(bytes.len())
            .map_err(|_| LoomError::invalid("graph key component too long"))?;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(bytes);
    }
    Ok(out)
}

#[cfg(test)]
fn props_bytes(props: &Props) -> Vec<u8> {
    cbor::encode(&props_value(props))
}

fn labels_value(labels: &BTreeSet<String>) -> Value {
    Value::Array(labels.iter().cloned().map(Value::Text).collect())
}

fn labels_from(items: Vec<Value>) -> Result<BTreeSet<String>> {
    let mut labels = BTreeSet::new();
    for item in items {
        let label = cbor::as_text(item)?;
        validate_label(&label)?;
        labels.insert(label);
    }
    Ok(labels)
}

fn node_bytes(node: &Node) -> Vec<u8> {
    cbor::encode(&Value::Array(vec![
        labels_value(&node.labels),
        props_value(&node.props),
    ]))
}

fn node_from_bytes(bytes: &[u8]) -> Result<Node> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let labels = labels_from(fields.array()?)?;
    let props = props_from(fields.map()?)?;
    fields.end()?;
    Node::new(labels, props)
}

fn edge_bytes(edge: &Edge) -> Vec<u8> {
    cbor::encode(&Value::Array(vec![
        Value::Text(edge.src.clone()),
        Value::Text(edge.dst.clone()),
        Value::Text(edge.label.clone()),
        props_value(&edge.props),
    ]))
}

fn edge_from_bytes(bytes: &[u8]) -> Result<Edge> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let src = fields.text()?;
    let dst = fields.text()?;
    let label = fields.text()?;
    let props = props_from(fields.map()?)?;
    fields.end()?;
    Ok(Edge {
        src,
        dst,
        label,
        props,
    })
}

/// Insert or replace node `id` with `props` in the graph `name` in `ns`, creating the graph and the
/// `graph` facet if absent, and stage the result.
pub fn graph_upsert_node<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
    props: Props,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Write)?;
    let mut g = load_or_new(loom, ns, name)?;
    g.upsert_node(id, props)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Graph), true)?;
    stage_graph_reserved(loom, ns, &graph_path(name), &g)
}

pub fn graph_upsert_node_with_labels<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
    labels: BTreeSet<String>,
    props: Props,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Write)?;
    let mut g = load_or_new(loom, ns, name)?;
    g.upsert_node_with_labels(id, labels, props)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Graph), true)?;
    stage_graph_reserved(loom, ns, &graph_path(name), &g)
}

/// The properties of node `id` in `name`, or `None` when the node or the graph is absent.
pub fn graph_get_node<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
) -> Result<Option<Props>> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    Ok(load_or_new(loom, ns, name)?.node(id).cloned())
}

pub fn graph_get_node_labels<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
) -> Result<Option<BTreeSet<String>>> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    Ok(load_or_new(loom, ns, name)?.node_labels(id).cloned())
}

pub fn graph_set_node_labels<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
    labels: BTreeSet<String>,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Write)?;
    let mut g = load_or_new(loom, ns, name)?;
    g.set_node_labels(id, labels)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Graph), true)?;
    stage_graph_reserved(loom, ns, &graph_path(name), &g)
}

pub fn graph_query<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    query: &GraphQuery,
) -> Result<GraphQueryResult> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    load_or_new(loom, ns, name)?.query(query)
}

pub fn graph_query_with_full_text<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    query: &GraphQuery,
    binding: &str,
    search_collection: &str,
    request: &QueryRequest,
) -> Result<GraphQueryResult> {
    graph_query_with_full_text_auto(
        loom,
        ns,
        name,
        query,
        binding,
        search_collection,
        request,
        None,
    )
}

pub fn graph_query_with_full_text_auto<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    query: &GraphQuery,
    binding: &str,
    search_collection: &str,
    request: &QueryRequest,
    engine: Option<&dyn SearchEngine>,
) -> Result<GraphQueryResult> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    validate_query(query)?;
    validate_binding(binding)?;
    let response = search_query_auto(loom, ns, search_collection, request, engine)?;
    let mut ids = BTreeSet::new();
    for hit in response.hits {
        let id = String::from_utf8(hit.id).map_err(|_| {
            LoomError::invalid("graph full-text projection id must be a UTF-8 graph id")
        })?;
        validate_graph_id(&id)?;
        ids.insert(id);
    }
    let mut projected = query.clone();
    projected.predicate = graph_predicate_and(
        projected.predicate,
        GraphPredicate::FullTextMatch {
            binding: binding.to_string(),
            ids,
        },
    );
    load_or_new(loom, ns, name)?.query(&projected)
}

pub fn graph_explain_query<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    query: &GraphQuery,
) -> Result<GraphQueryExplain> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    load_or_new(loom, ns, name)?.explain_query(query)
}

pub fn graph_declare_property_index<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    index_name: &str,
    entity: GraphIndexEntity,
    property: &str,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Write)?;
    let mut graph = load_or_new(loom, ns, name)?;
    graph.declare_property_index(index_name, entity, property)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Graph), true)?;
    stage_graph_reserved(loom, ns, &graph_path(name), &graph)
}

pub fn graph_property_index_reports<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Vec<GraphPropertyIndexReport>> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    Ok(load_or_new(loom, ns, name)?.property_index_reports())
}

pub fn graph_declare_spatial_index<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    index_name: &str,
    entity: GraphIndexEntity,
    property: &str,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Write)?;
    let mut graph = load_or_new(loom, ns, name)?;
    graph.declare_spatial_index(index_name, entity, property)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Graph), true)?;
    stage_graph_reserved(loom, ns, &graph_path(name), &graph)
}

pub fn graph_spatial_index_reports<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Vec<GraphSpatialIndexReport>> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    Ok(load_or_new(loom, ns, name)?.spatial_index_reports())
}

pub fn graph_apply_mutations<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    plan: &GraphMutationPlan,
) -> Result<GraphMutationResult> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Write)?;
    let mut graph = load_or_new(loom, ns, name)?;
    let result = graph.apply_mutations(plan)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Graph), true)?;
    stage_graph_reserved(loom, ns, &graph_path(name), &graph)?;
    Ok(result)
}

/// Remove node `id` from `name`. `cascade=false` rejects with `CONFLICT` while incident edges exist;
/// `cascade=true` removes the node and its incident edges. `NOT_FOUND` if the node is absent.
pub fn graph_remove_node<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
    cascade: bool,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Write)?;
    let mut g = load_or_new(loom, ns, name)?;
    g.remove_node(id, cascade)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Graph), true)?;
    stage_graph_reserved(loom, ns, &graph_path(name), &g)
}

/// Insert or replace edge `id` from `src` to `dst` (both endpoints must already exist, else
/// `NOT_FOUND`) with `label` and `props` in `name`, and stage the result.
#[allow(clippy::too_many_arguments)]
pub fn graph_upsert_edge<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
    src: &str,
    dst: &str,
    label: &str,
    props: Props,
) -> Result<()> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Write)?;
    let mut g = load_or_new(loom, ns, name)?;
    g.upsert_edge(id, src, dst, label, props)?;
    loom.create_directory_reserved(ns, &facet_root(FacetKind::Graph), true)?;
    stage_graph_reserved(loom, ns, &graph_path(name), &g)
}

/// The edge `id` in `name`, or `None` when the edge or the graph is absent.
pub fn graph_get_edge<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
) -> Result<Option<Edge>> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    Ok(load_or_new(loom, ns, name)?.edge(id).cloned())
}

/// All edges in `name` as `(edge_id, edge)` in edge-id order.
pub fn graph_edges<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
) -> Result<Vec<(String, Edge)>> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    Ok(load_or_new(loom, ns, name)?
        .edges()
        .into_iter()
        .map(|(eid, e)| (eid.to_string(), e.clone()))
        .collect())
}

/// Remove edge `id` from `name`; returns whether it was present. An absent edge or graph is a no-op
/// that does not write.
pub fn graph_remove_edge<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
) -> Result<bool> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Write)?;
    let mut g = load_or_new(loom, ns, name)?;
    let present = g.edge(id).is_some();
    if present {
        g.remove_edge(id);
        loom.create_directory_reserved(ns, &facet_root(FacetKind::Graph), true)?;
        stage_graph_reserved(loom, ns, &graph_path(name), &g)?;
    }
    Ok(present)
}

/// The distinct adjacent node ids (out- and in-neighbours) of `id` in `name`, sorted; empty when the
/// node or graph is absent.
pub fn graph_neighbors<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
) -> Result<Vec<String>> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    Ok(load_or_new(loom, ns, name)?.neighbors(id))
}

/// Out-edges of `id` in `name` as `(edge_id, edge)` in edge-id order.
pub fn graph_out_edges<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
) -> Result<Vec<(String, Edge)>> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    Ok(load_or_new(loom, ns, name)?
        .out_edges(id)
        .into_iter()
        .map(|(eid, e)| (eid.to_string(), e.clone()))
        .collect())
}

/// In-edges of `id` in `name` as `(edge_id, edge)` in edge-id order.
pub fn graph_in_edges<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    id: &str,
) -> Result<Vec<(String, Edge)>> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    Ok(load_or_new(loom, ns, name)?
        .in_edges(id)
        .into_iter()
        .map(|(eid, e)| (eid.to_string(), e.clone()))
        .collect())
}

/// Node ids reachable from `start` in graph `name` (see [`Graph::reachable`]); empty when the node or
/// graph is absent.
pub fn graph_reachable<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    start: &str,
    max_depth: Option<usize>,
    via_label: Option<&str>,
) -> Result<Vec<String>> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    Ok(load_or_new(loom, ns, name)?.reachable(start, max_depth, via_label))
}

/// A shortest directed path from `from` to `to` in graph `name` (see [`Graph::shortest_path`]), or
/// `None` when no path exists or an endpoint or the graph is absent.
pub fn graph_shortest_path<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    from: &str,
    to: &str,
    via_label: Option<&str>,
) -> Result<Option<Vec<String>>> {
    loom.authorize_collection(ns, FacetKind::Graph, name, AclRight::Read)?;
    Ok(load_or_new(loom, ns, name)?.shortest_path(from, to, via_label))
}

// ---- props codec --------------------------------------------------------------------------------

fn props_value(props: &Props) -> Value {
    Value::Map(
        props
            .iter()
            .map(|(k, v)| (Value::Text(k.clone()), v.to_cbor()))
            .collect(),
    )
}

fn props_from(pairs: Vec<(Value, Value)>) -> Result<Props> {
    let mut p = Props::new();
    for (k, v) in pairs {
        p.insert(cbor::as_text(k)?, GraphValue::from_cbor(v)?);
    }
    Ok(p)
}

fn validate_props(props: &Props) -> Result<()> {
    for (property, value) in props {
        validate_property_name(property)?;
        value.validate()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    fn props(pairs: &[(&str, &[u8])]) -> Props {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), GraphValue::Bytes(v.to_vec())))
            .collect()
    }

    #[test]
    fn upsert_merges_by_id_and_traverses() {
        let mut g = Graph::new();
        g.upsert_node("alice", props(&[("kind", b"person")]))
            .unwrap();
        g.upsert_node("bob", props(&[("kind", b"person")])).unwrap();
        g.upsert_edge("e1", "alice", "bob", "knows", Props::new())
            .unwrap();
        // upsert by id replaces, not duplicates.
        g.upsert_node("alice", props(&[("kind", b"admin")]))
            .unwrap();
        assert_eq!(g.node_count(), 2);
        assert_eq!(
            g.node("alice").unwrap()["kind"],
            GraphValue::Bytes(b"admin".to_vec())
        );
        // traversal core
        assert_eq!(g.out_edges("alice").len(), 1);
        assert_eq!(g.in_edges("bob").len(), 1);
        assert_eq!(g.neighbors("alice"), vec!["bob".to_string()]);
    }

    #[test]
    fn edge_requires_existing_endpoints() {
        let mut g = Graph::new();
        g.upsert_node("a", Props::new()).unwrap();
        assert_eq!(
            g.upsert_edge("e", "a", "ghost", "x", Props::new())
                .unwrap_err()
                .code,
            Code::NotFound
        );
    }

    #[test]
    fn remove_node_cascade_semantics() {
        let mut g = Graph::new();
        g.upsert_node("a", Props::new()).unwrap();
        g.upsert_node("b", Props::new()).unwrap();
        g.upsert_edge("e", "a", "b", "x", Props::new()).unwrap();
        // cascade=false rejects while an incident edge exists.
        assert_eq!(g.remove_node("a", false).unwrap_err().code, Code::Conflict);
        // cascade=true removes the node and the incident edge.
        g.remove_node("a", true).unwrap();
        assert_eq!(g.node_count(), 1);
        assert_eq!(g.edge_count(), 0);
    }

    #[test]
    fn encode_round_trips_and_versions() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([8; 16]))
            .unwrap();
        let mut g = Graph::new();
        g.upsert_node("a", props(&[("p", b"1")])).unwrap();
        g.upsert_node("b", Props::new()).unwrap();
        g.upsert_edge("e", "a", "b", "rel", props(&[("w", b"5")]))
            .unwrap();
        assert_eq!(Graph::decode(&g.encode()).unwrap().edge_count(), 1);

        put_graph(&mut loom, ns, "g", &g).unwrap();
        let c1 = loom.commit(ns, "nas", "graph v1", 1).unwrap();
        g.upsert_node("c", Props::new()).unwrap();
        put_graph(&mut loom, ns, "g", &g).unwrap();
        loom.commit(ns, "nas", "graph v2", 2).unwrap();
        assert_eq!(get_graph(&loom, ns, "g").unwrap().node_count(), 3);
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(get_graph(&loom, ns, "g").unwrap().node_count(), 2);
    }

    #[test]
    fn graph_stages_as_structured_root_with_component_maps() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([18; 16]))
            .unwrap();
        graph_upsert_node(&mut loom, ns, "g", "a", props(&[("kind", b"person")])).unwrap();
        graph_upsert_node(&mut loom, ns, "g", "b", Props::new()).unwrap();
        graph_upsert_edge(&mut loom, ns, "g", "e", "a", "b", "rel", Props::new()).unwrap();

        let path = graph_path("g");
        let StagedEntry::Graph(root) = loom.work.get(&ns).unwrap().get(&path).unwrap() else {
            panic!("graph path must stage as a graph root");
        };
        let Object::Tree(entries) = loom.get_object(root).unwrap() else {
            panic!("graph root must be a Tree");
        };
        let kinds = entries
            .iter()
            .map(|entry| (entry.name.as_str(), entry.kind))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(kinds.get(GRAPH_META_ENTRY), Some(&EntryKind::Blob));
        assert_eq!(kinds.get(GRAPH_INDEX_CATALOG_ENTRY), Some(&EntryKind::Blob));
        assert_eq!(kinds.get(GRAPH_NODES_ENTRY), Some(&EntryKind::ProllyMap));
        assert_eq!(kinds.get(GRAPH_EDGES_ENTRY), Some(&EntryKind::ProllyMap));
        assert_eq!(
            kinds.get(GRAPH_FORWARD_ADJ_ENTRY),
            Some(&EntryKind::ProllyMap)
        );
        assert_eq!(
            kinds.get(GRAPH_REVERSE_ADJ_ENTRY),
            Some(&EntryKind::ProllyMap)
        );

        let commit = loom.commit(ns, "nas", "structured graph", 1).unwrap();
        let Object::Tree(root_entries) =
            loom.get_object(&loom.commit_tree(commit).unwrap()).unwrap()
        else {
            panic!("commit root must be a Tree");
        };
        let graph_entry = root_entries
            .iter()
            .find(|entry| entry.name == ".loom")
            .and_then(|entry| match loom.get_object(&entry.target).unwrap() {
                Object::Tree(entries) => entries
                    .into_iter()
                    .find(|entry| entry.name == "facets")
                    .map(|entry| entry.target),
                _ => None,
            })
            .and_then(|facets| match loom.get_object(&facets).unwrap() {
                Object::Tree(entries) => entries
                    .into_iter()
                    .find(|entry| entry.name == "graph")
                    .map(|entry| entry.target),
                _ => None,
            })
            .and_then(|graph_dir| match loom.get_object(&graph_dir).unwrap() {
                Object::Tree(entries) => entries.into_iter().find(|entry| entry.name == "g"),
                _ => None,
            })
            .unwrap();
        assert_eq!(graph_entry.kind, EntryKind::Graph);
    }

    #[test]
    fn graph_component_canonical_vectors_are_pinned() {
        let mut props = Props::new();
        props.insert("n".to_string(), GraphValue::Int(7));
        props.insert("t".to_string(), GraphValue::Text("x".to_string()));
        let node = Node::new(BTreeSet::from(["Person".to_string()]), props.clone()).unwrap();
        let edge = Edge {
            src: "a".to_string(),
            dst: "b".to_string(),
            label: "rel".to_string(),
            props: Props::new(),
        };

        assert_eq!(
            hex::encode(graph_metadata()),
            "8601781c74797065642d6c6162656c65642d70726f70657274792d677261706867757466382d6964781c6c656e6774682d70726566697865642d61646a6163656e63792d7631726e6f64652d656467652d6d657267652d763178187370617469616c2d696e6465782d636174616c6f672d7631"
        );
        assert_eq!(hex::encode(empty_property_index_catalog()), "820180");
        assert_eq!(hex::encode(empty_spatial_index_catalog()), "820180");
        assert_eq!(hex::encode(node_key("node-1").unwrap()), "6e6f64652d31");
        assert_eq!(
            hex::encode(compound_key(["a", "rel", "b", "e"]).unwrap()),
            "00000001610000000372656c00000001620000000165"
        );
        assert_eq!(hex::encode(props_bytes(&props)), "a2616e0761746178");
        let nested_props = BTreeMap::from([
            (
                "a".to_string(),
                GraphValue::List(vec![GraphValue::Int(1), GraphValue::Text("x".to_string())]),
            ),
            (
                "m".to_string(),
                GraphValue::Map(BTreeMap::from([(
                    "flag".to_string(),
                    GraphValue::Bool(true),
                )])),
            ),
        ]);
        assert_eq!(
            hex::encode(props_bytes(&nested_props)),
            "a2616182016178616da164666c6167f5"
        );
        assert_eq!(
            hex::encode(node_bytes(&node)),
            "828166506572736f6ea2616e0761746178"
        );
        assert_eq!(hex::encode(edge_bytes(&edge)), "84616161626372656ca0");
    }

    #[test]
    fn graph_geometry_values_are_canonical_and_validated() {
        let point =
            GraphGeometry::point(GraphCrs::Crs84_2d, 12.5, 55.0, None).expect("valid point");
        let mut geo_props = Props::new();
        geo_props.insert("loc".to_string(), GraphValue::Geometry(point.clone()));
        assert_eq!(
            hex::encode(props_bytes(&geo_props)),
            "a1636c6f6386766c6f6f6d2e67726170682e67656f6d657472792e763165706f696e746863727338345f3264fb4029000000000000fb404b800000000000f6"
        );
        assert_eq!(
            GraphValue::from_cbor(GraphValue::Geometry(point.clone()).to_cbor()).unwrap(),
            GraphValue::Geometry(point)
        );
        assert!(GraphGeometry::point(GraphCrs::Crs84_2d, 181.0, 0.0, None).is_err());
        assert!(GraphGeometry::point(GraphCrs::Crs84_3d, 12.5, 55.0, None).is_err());
        assert!(GraphGeometry::point(GraphCrs::Cartesian2d, f64::NAN, 0.0, None).is_err());
        assert!(
            GraphValue::List(vec![GraphValue::Text(GRAPH_GEOMETRY_TAG.to_string())])
                .validate()
                .is_err()
        );
    }

    #[test]
    fn graph_geo_predicates_filter_point_values() {
        let mut graph = Graph::new();
        graph
            .upsert_node_with_labels(
                "ada",
                BTreeSet::from(["Person".to_string()]),
                BTreeMap::from([(
                    "loc".to_string(),
                    GraphValue::Geometry(
                        GraphGeometry::point(GraphCrs::Crs84_2d, 12.5, 55.0, None).unwrap(),
                    ),
                )]),
            )
            .unwrap();
        graph
            .upsert_node_with_labels(
                "grace",
                BTreeSet::from(["Person".to_string()]),
                BTreeMap::from([(
                    "loc".to_string(),
                    GraphValue::Geometry(
                        GraphGeometry::point(GraphCrs::Crs84_2d, 13.0, 55.0, None).unwrap(),
                    ),
                )]),
            )
            .unwrap();
        graph
            .upsert_node_with_labels(
                "plain",
                BTreeSet::from(["Person".to_string()]),
                BTreeMap::from([("loc".to_string(), GraphValue::Text("nowhere".to_string()))]),
            )
            .unwrap();

        let near = GraphQuery::parse_opencypher(
            "MATCH (p:Person) WHERE distance(p.loc, point('crs84_2d', 12.5, 55.0)) <= 1 RETURN p ORDER BY id(p)",
        )
        .unwrap();
        let near_rows = graph.query(&near).unwrap().rows;
        assert_eq!(near_rows.len(), 1);
        assert!(matches!(
            near_rows[0].get("p"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, .. })) if id == "ada"
        ));

        let bbox = GraphQuery::parse_opencypher(
            "MATCH (p:Person) WHERE within_bbox(p.loc, 12.0, 54.0, 12.6, 56.0) RETURN p ORDER BY id(p)",
        )
        .unwrap();
        let bbox_rows = graph.query(&bbox).unwrap().rows;
        assert_eq!(bbox_rows.len(), 1);
        assert!(matches!(
            bbox_rows[0].get("p"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, .. })) if id == "ada"
        ));

        let mismatched = GraphQuery::parse_opencypher(
            "MATCH (p:Person) WHERE distance(p.loc, point('cartesian_2d', 12.5, 55.0)) <= 100000 RETURN p",
        )
        .unwrap();
        assert!(graph.query(&mismatched).unwrap().rows.is_empty());

        assert!(
            GraphQuery::parse_opencypher(
                "MATCH (p:Person) WHERE within_bbox(p.loc, 13.0, 54.0, 12.0, 56.0) RETURN p"
            )
            .is_err()
        );
    }

    #[test]
    fn node_labels_are_canonical_and_survive_structured_storage() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([22; 16]))
            .unwrap();
        graph_upsert_node_with_labels(
            &mut loom,
            ns,
            "g",
            "alice",
            BTreeSet::from(["Employee".to_string(), "Person".to_string()]),
            BTreeMap::from([("name".to_string(), GraphValue::Text("Alice".to_string()))]),
        )
        .unwrap();
        graph_upsert_node(&mut loom, ns, "g", "alice", Props::new()).unwrap();
        assert_eq!(
            graph_get_node_labels(&loom, ns, "g", "alice").unwrap(),
            Some(BTreeSet::from([
                "Employee".to_string(),
                "Person".to_string()
            ]))
        );
        graph_set_node_labels(
            &mut loom,
            ns,
            "g",
            "alice",
            BTreeSet::from(["Person".to_string()]),
        )
        .unwrap();
        assert_eq!(
            graph_get_node_labels(&loom, ns, "g", "alice").unwrap(),
            Some(BTreeSet::from(["Person".to_string()]))
        );
        let commit = loom.commit(ns, "nas", "labels", 1).unwrap();
        loom.checkout_commit(ns, commit).unwrap();
        assert_eq!(
            get_graph(&loom, ns, "g")
                .unwrap()
                .graph_node("alice")
                .unwrap()
                .labels,
            BTreeSet::from(["Person".to_string()])
        );
    }

    #[test]
    fn native_graph_query_ir_returns_typed_graph_values() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([23; 16]))
            .unwrap();
        graph_upsert_node_with_labels(
            &mut loom,
            ns,
            "org",
            "alice",
            BTreeSet::from(["Person".to_string()]),
            BTreeMap::from([("name".to_string(), GraphValue::Text("Alice".to_string()))]),
        )
        .unwrap();
        graph_upsert_node_with_labels(
            &mut loom,
            ns,
            "org",
            "acme",
            BTreeSet::from(["Organization".to_string()]),
            BTreeMap::from([("name".to_string(), GraphValue::Text("Acme".to_string()))]),
        )
        .unwrap();
        graph_upsert_edge(
            &mut loom,
            ns,
            "org",
            "e1",
            "alice",
            "acme",
            "WORKS_AT",
            Props::new(),
        )
        .unwrap();

        let mut person = GraphNodePattern::new("p");
        person.labels.insert("Person".to_string());
        let mut edge = GraphEdgePattern::new("r");
        edge.label = Some("WORKS_AT".to_string());
        let mut org = GraphNodePattern::new("o");
        org.labels.insert("Organization".to_string());
        let query = GraphQuery {
            patterns: vec![GraphPattern::directed(person, edge, org)],
            predicate: GraphPredicate::Eq {
                binding: "p".to_string(),
                property: "name".to_string(),
                value: GraphValue::Text("Alice".to_string()),
            },
            returns: vec![
                GraphReturn::Binding("p".to_string()),
                GraphReturn::Binding("r".to_string()),
                GraphReturn::Property {
                    binding: "o".to_string(),
                    property: "name".to_string(),
                },
            ],
            order_by: Vec::new(),
            skip: None,
            limit: Some(10),
        };
        let result = graph_query(&loom, ns, "org", &query).unwrap();
        assert_eq!(result.rows.len(), 1);
        let row = &result.rows[0];
        assert!(matches!(
            row.get("p"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, labels, .. }))
                if id == "alice" && labels.contains("Person")
        ));
        assert!(matches!(
            row.get("r"),
            Some(GraphQueryValue::Edge(GraphQueryEdge { id, label, src, dst, .. }))
                if id == "e1" && label == "WORKS_AT" && src == "alice" && dst == "acme"
        ));
        assert_eq!(
            row.get("o.name"),
            Some(&GraphQueryValue::Scalar(GraphValue::Text(
                "Acme".to_string()
            )))
        );
    }

    #[test]
    fn bounded_opencypher_read_profile_lowers_to_native_ir() {
        let mut graph = Graph::new();
        graph
            .upsert_node_with_labels(
                "alice",
                BTreeSet::from(["Person".to_string()]),
                BTreeMap::from([
                    ("age".to_string(), GraphValue::Int(41)),
                    ("name".to_string(), GraphValue::Text("Alice".to_string())),
                ]),
            )
            .unwrap();
        graph
            .upsert_node_with_labels(
                "acme",
                BTreeSet::from(["Organization".to_string()]),
                BTreeMap::from([("name".to_string(), GraphValue::Text("Acme".to_string()))]),
            )
            .unwrap();
        graph
            .upsert_edge("e1", "alice", "acme", "WORKS_AT", Props::new())
            .unwrap();

        let query = GraphQuery::parse_opencypher(
            "MATCH (p:Person {age: 41})-[r:WORKS_AT]->(o:Organization) \
             WHERE p.name = 'Alice' RETURN p, r, o.name LIMIT 5",
        )
        .unwrap();
        let result = graph.query(&query).unwrap();
        assert_eq!(result.rows.len(), 1);
        assert!(matches!(
            result.rows[0].get("p"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, labels, .. }))
                if id == "alice" && labels.contains("Person")
        ));
        assert!(matches!(
            result.rows[0].get("r"),
            Some(GraphQueryValue::Edge(GraphQueryEdge { id, label, .. }))
                if id == "e1" && label == "WORKS_AT"
        ));
        assert_eq!(
            result.rows[0].get("o.name"),
            Some(&GraphQueryValue::Scalar(GraphValue::Text(
                "Acme".to_string()
            )))
        );
    }

    #[test]
    fn bounded_opencypher_read_profile_supports_order_skip_count_and_comparisons() {
        let mut graph = Graph::new();
        for (id, name, age, org) in [
            ("alice", "Alice", 41, "acme"),
            ("bob", "Bob", 30, "acme"),
            ("cara", "Cara", 29, "beta"),
        ] {
            graph
                .upsert_node_with_labels(
                    id,
                    BTreeSet::from(["Person".to_string()]),
                    BTreeMap::from([
                        ("age".to_string(), GraphValue::Int(age)),
                        ("name".to_string(), GraphValue::Text(name.to_string())),
                    ]),
                )
                .unwrap();
            if graph.graph_node(org).is_none() {
                graph
                    .upsert_node_with_labels(
                        org,
                        BTreeSet::from(["Organization".to_string()]),
                        BTreeMap::from([(
                            "name".to_string(),
                            GraphValue::Text(org.to_uppercase()),
                        )]),
                    )
                    .unwrap();
            }
            graph
                .upsert_edge(&format!("{id}-{org}"), id, org, "WORKS_AT", Props::new())
                .unwrap();
        }

        let ordered = GraphQuery::parse_opencypher(
            "MATCH (p:Person) WHERE p.age >= 30 OR p.name = 'Cara' \
             RETURN p ORDER BY p.name DESC SKIP 1 LIMIT 1",
        )
        .unwrap();
        let ordered_result = graph.query(&ordered).unwrap();
        assert_eq!(ordered_result.rows.len(), 1);
        assert!(matches!(
            ordered_result.rows[0].get("p"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, .. })) if id == "bob"
        ));

        let grouped = GraphQuery::parse_opencypher(
            "MATCH (p:Person)-[r:WORKS_AT]->(o:Organization) WHERE NOT p.age < 0 \
             RETURN o.name, count(p) AS people ORDER BY count(p) DESC LIMIT 1",
        )
        .unwrap();
        let grouped_result = graph.query(&grouped).unwrap();
        assert_eq!(grouped_result.rows.len(), 1);
        assert_eq!(
            grouped_result.rows[0].get("o.name"),
            Some(&GraphQueryValue::Scalar(GraphValue::Text(
                "ACME".to_string()
            )))
        );
        assert_eq!(
            grouped_result.rows[0].get("people"),
            Some(&GraphQueryValue::Scalar(GraphValue::Int(2)))
        );
        assert!(matches!(
            grouped.returns[1],
            GraphReturn::Count {
                binding: Some(_),
                ..
            }
        ));

        let regex = GraphQuery::parse_opencypher(
            "MATCH (p:Person) WHERE p.name =~ '^A.*e$' RETURN p.name LIMIT 10",
        )
        .unwrap();
        let regex_result = graph.query(&regex).unwrap();
        assert_eq!(regex_result.rows.len(), 1);
        assert_eq!(
            regex_result.rows[0].get("p.name"),
            Some(&GraphQueryValue::Scalar(GraphValue::Text(
                "Alice".to_string()
            )))
        );
        let non_text_regex =
            GraphQuery::parse_opencypher("MATCH (p:Person) WHERE p.age =~ '^[0-9]+$' RETURN p")
                .unwrap();
        assert_eq!(graph.query(&non_text_regex).unwrap().rows.len(), 0);
        assert_eq!(
            GraphQuery::parse_opencypher("MATCH (p:Person) WHERE p.name =~ '[' RETURN p")
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );

        let functions = GraphQuery::parse_opencypher(
            "MATCH (p:Person)-[r:WORKS_AT]->(o:Organization) \
             RETURN id(p) AS pid, type(r) AS rel, startNode(r) AS start, endNode(r) AS finish \
             ORDER BY type(r), id(p) LIMIT 1",
        )
        .unwrap();
        let function_result = graph.query(&functions).unwrap();
        assert_eq!(function_result.rows.len(), 1);
        assert_eq!(
            function_result.rows[0].get("pid"),
            Some(&GraphQueryValue::Scalar(GraphValue::Text(
                "alice".to_string()
            )))
        );
        assert_eq!(
            function_result.rows[0].get("rel"),
            Some(&GraphQueryValue::Scalar(GraphValue::Text(
                "WORKS_AT".to_string()
            )))
        );
        assert!(matches!(
            function_result.rows[0].get("start"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, .. })) if id == "alice"
        ));
        assert!(matches!(
            function_result.rows[0].get("finish"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, .. })) if id == "acme"
        ));
        let labels = GraphQuery::parse_opencypher("MATCH (p:Person) RETURN labels(p)").unwrap();
        let labels = graph.query(&labels).unwrap();
        assert!(matches!(
            labels.rows[0].get("labels(p)"),
            Some(GraphQueryValue::List(values))
                if values.contains(&GraphQueryValue::Scalar(GraphValue::Text("Person".to_string())))
        ));
    }

    #[test]
    fn graph_mutation_plan_executes_atomically() {
        let mut graph = Graph::new();
        let plan = GraphMutationPlan::new(vec![
            GraphMutation::CreateNode {
                id: "alice".to_string(),
                labels: BTreeSet::from(["Person".to_string()]),
                props: BTreeMap::from([(
                    "name".to_string(),
                    GraphValue::Text("Alice".to_string()),
                )]),
            },
            GraphMutation::CreateNode {
                id: "acme".to_string(),
                labels: BTreeSet::from(["Organization".to_string()]),
                props: Props::new(),
            },
            GraphMutation::CreateEdge {
                id: "employment".to_string(),
                src: "alice".to_string(),
                dst: "acme".to_string(),
                label: "WORKS_AT".to_string(),
                props: Props::new(),
            },
            GraphMutation::SetNodeProperty {
                id: "alice".to_string(),
                property: "age".to_string(),
                value: GraphValue::Int(41),
            },
            GraphMutation::RemoveNodeProperty {
                id: "alice".to_string(),
                property: "name".to_string(),
            },
            GraphMutation::SetEdgeProperty {
                id: "employment".to_string(),
                property: "since".to_string(),
                value: GraphValue::Int(2026),
            },
        ]);
        let result = graph.apply_mutations(&plan).unwrap();
        assert_eq!(result.applied, 6);
        assert!(
            !graph
                .graph_node("alice")
                .unwrap()
                .props
                .contains_key("name")
        );
        assert_eq!(
            graph.graph_node("alice").unwrap().props.get("age"),
            Some(&GraphValue::Int(41))
        );
        assert_eq!(
            graph.edge("employment").unwrap().props.get("since"),
            Some(&GraphValue::Int(2026))
        );

        let snapshot = graph.clone();
        let failed = GraphMutationPlan::new(vec![
            GraphMutation::SetNodeProperty {
                id: "alice".to_string(),
                property: "verified".to_string(),
                value: GraphValue::Bool(true),
            },
            GraphMutation::CreateEdge {
                id: "broken".to_string(),
                src: "alice".to_string(),
                dst: "missing".to_string(),
                label: "BROKEN".to_string(),
                props: Props::new(),
            },
        ]);
        assert_eq!(
            graph.apply_mutations(&failed).unwrap_err().code,
            Code::NotFound
        );
        assert_eq!(graph.nodes, snapshot.nodes);
        assert_eq!(graph.edges, snapshot.edges);

        let conflict = GraphMutationPlan::new(vec![GraphMutation::CreateNode {
            id: "alice".to_string(),
            labels: BTreeSet::new(),
            props: Props::new(),
        }]);
        assert_eq!(
            graph.apply_mutations(&conflict).unwrap_err().code,
            Code::Conflict
        );

        let delete = GraphMutationPlan::new(vec![GraphMutation::DeleteNode {
            id: "alice".to_string(),
            detach: true,
        }]);
        graph.apply_mutations(&delete).unwrap();
        assert!(graph.graph_node("alice").is_none());
        assert!(graph.edge("employment").is_none());
    }

    #[test]
    fn graph_merge_mutations_are_idempotent_and_conflict_on_mismatch() {
        let mut graph = Graph::new();
        let merge = GraphMutationPlan::new(vec![
            GraphMutation::MergeNode {
                id: "person/ada".to_string(),
                labels: BTreeSet::from(["Person".to_string()]),
                props: BTreeMap::from([("name".to_string(), GraphValue::Text("Ada".to_string()))]),
            },
            GraphMutation::MergeNode {
                id: "org/uldren".to_string(),
                labels: BTreeSet::from(["Organization".to_string()]),
                props: BTreeMap::from([(
                    "name".to_string(),
                    GraphValue::Text("Uldren".to_string()),
                )]),
            },
            GraphMutation::MergeEdge {
                id: "employment/ada-uldren".to_string(),
                src: "person/ada".to_string(),
                dst: "org/uldren".to_string(),
                label: "WORKS_AT".to_string(),
                props: BTreeMap::from([("since".to_string(), GraphValue::Int(2026))]),
            },
        ]);
        assert_eq!(graph.apply_mutations(&merge).unwrap().applied, 3);
        assert_eq!(graph.apply_mutations(&merge).unwrap().applied, 3);
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);

        let snapshot = graph.clone();
        let node_conflict = GraphMutationPlan::new(vec![GraphMutation::MergeNode {
            id: "person/ada".to_string(),
            labels: BTreeSet::from(["Person".to_string()]),
            props: BTreeMap::from([("name".to_string(), GraphValue::Text("Grace".to_string()))]),
        }]);
        assert_eq!(
            graph.apply_mutations(&node_conflict).unwrap_err().code,
            Code::Conflict
        );
        assert_eq!(graph.nodes, snapshot.nodes);
        assert_eq!(graph.edges, snapshot.edges);

        let edge_conflict = GraphMutationPlan::new(vec![GraphMutation::MergeEdge {
            id: "employment/ada-uldren".to_string(),
            src: "org/uldren".to_string(),
            dst: "person/ada".to_string(),
            label: "WORKS_AT".to_string(),
            props: BTreeMap::from([("since".to_string(), GraphValue::Int(2026))]),
        }]);
        assert_eq!(
            graph.apply_mutations(&edge_conflict).unwrap_err().code,
            Code::Conflict
        );
        assert_eq!(graph.nodes, snapshot.nodes);
        assert_eq!(graph.edges, snapshot.edges);
    }

    #[test]
    fn graph_semantic_diff_reports_node_and_edge_records() {
        let mut base = Graph::new();
        base.upsert_node_with_labels(
            "ada",
            BTreeSet::from(["Person".to_string()]),
            BTreeMap::from([("name".to_string(), GraphValue::Text("Ada".to_string()))]),
        )
        .unwrap();
        base.upsert_node("org", Props::new()).unwrap();
        base.upsert_edge("works", "ada", "org", "WORKS_AT", Props::new())
            .unwrap();

        let mut head = base.clone();
        head.set_node_property("ada", "age", GraphValue::Int(41))
            .unwrap();
        head.remove_node_property("ada", "name").unwrap();
        head.set_node_labels(
            "ada",
            BTreeSet::from(["Engineer".to_string(), "Person".to_string()]),
        )
        .unwrap();
        head.set_edge_property("works", "since", GraphValue::Int(2026))
            .unwrap();
        head.upsert_node("project", Props::new()).unwrap();
        head.upsert_edge("builds", "ada", "project", "BUILDS", Props::new())
            .unwrap();

        let diff = Graph::semantic_diff(&base, &head);
        assert!(matches!(
            &diff.nodes[0],
            GraphNodeDiff::Updated {
                id,
                labels_added,
                props_set,
                props_removed,
                ..
            } if id == "ada"
                && labels_added.contains("Engineer")
                && props_set.get("age") == Some(&GraphValue::Int(41))
                && props_removed == &vec!["name".to_string()]
        ));
        assert!(matches!(
            &diff.nodes[1],
            GraphNodeDiff::Added { id, .. } if id == "project"
        ));
        assert!(matches!(
            &diff.edges[0],
            GraphEdgeDiff::Added { id, .. } if id == "builds"
        ));
        assert!(matches!(
            &diff.edges[1],
            GraphEdgeDiff::Updated { id, props_set, .. }
                if id == "works" && props_set.get("since") == Some(&GraphValue::Int(2026))
        ));
    }

    #[test]
    fn graph_semantic_merge_merges_independent_node_and_edge_edits() {
        let mut base = Graph::new();
        base.upsert_node_with_labels(
            "ada",
            BTreeSet::from(["Person".to_string()]),
            BTreeMap::from([("name".to_string(), GraphValue::Text("Ada".to_string()))]),
        )
        .unwrap();
        base.upsert_node("org", Props::new()).unwrap();
        base.upsert_edge("works", "ada", "org", "WORKS_AT", Props::new())
            .unwrap();

        let mut left = base.clone();
        left.set_node_property("ada", "age", GraphValue::Int(41))
            .unwrap();
        left.set_edge_property("works", "since", GraphValue::Int(2026))
            .unwrap();

        let mut right = base.clone();
        right
            .set_node_labels(
                "ada",
                BTreeSet::from(["Person".to_string(), "Researcher".to_string()]),
            )
            .unwrap();
        right.upsert_node("project", Props::new()).unwrap();
        right
            .upsert_edge("builds", "ada", "project", "BUILDS", Props::new())
            .unwrap();

        let merge = Graph::semantic_merge(&base, &left, &right).unwrap();
        assert!(merge.conflicts.is_empty());
        let graph = merge.graph.unwrap();
        assert_eq!(
            graph.graph_node("ada").unwrap().props.get("age"),
            Some(&GraphValue::Int(41))
        );
        assert!(
            graph
                .graph_node("ada")
                .unwrap()
                .labels
                .contains("Researcher")
        );
        assert_eq!(
            graph.edge("works").unwrap().props.get("since"),
            Some(&GraphValue::Int(2026))
        );
        assert!(graph.edge("builds").is_some());
    }

    #[test]
    fn graph_semantic_merge_reports_property_and_endpoint_conflicts() {
        let mut base = Graph::new();
        base.upsert_node(
            "ada",
            BTreeMap::from([("name".to_string(), GraphValue::Text("Ada".to_string()))]),
        )
        .unwrap();
        base.upsert_node("org", Props::new()).unwrap();
        base.upsert_edge("works", "ada", "org", "WORKS_AT", Props::new())
            .unwrap();

        let mut left = base.clone();
        left.set_node_property("ada", "name", GraphValue::Text("Ada Lovelace".to_string()))
            .unwrap();

        let mut right = base.clone();
        right
            .set_node_property("ada", "name", GraphValue::Text("Augusta Ada".to_string()))
            .unwrap();
        right.remove_node("org", true).unwrap();

        let merge = Graph::semantic_merge(&base, &left, &right).unwrap();
        assert!(merge.graph.is_none());
        assert!(merge.conflicts.iter().any(|conflict| matches!(
            conflict,
            GraphMergeConflict {
                entity: GraphMergeConflictEntity::Node(id),
                kind: GraphMergeConflictKind::PropertyConflict(property),
            } if id == "ada" && property == "name"
        )));
        assert!(merge.conflicts.iter().any(|conflict| matches!(
            conflict,
            GraphMergeConflict {
                entity: GraphMergeConflictEntity::Edge(id),
                kind: GraphMergeConflictKind::EndpointDeleted,
            } if id == "works"
        )));
    }

    #[test]
    fn opencypher_mutation_text_lowers_through_identity_envelope() {
        let identity = GraphMutationIdentity::new(
            BTreeMap::from([
                ("p".to_string(), "person/ada".to_string()),
                ("o".to_string(), "org/uldren".to_string()),
            ]),
            BTreeMap::from([("r".to_string(), "employment/ada-uldren".to_string())]),
        );
        let create = GraphMutationPlan::parse_opencypher(
            "CREATE (p:Person {name: 'Ada'})-[r:WORKS_AT {since: 2026}]->(o:Organization {name: 'Uldren'})",
            &identity,
        )
        .unwrap();
        assert_eq!(create.mutations.len(), 3);
        assert!(matches!(
            &create.mutations[0],
            GraphMutation::CreateNode { id, labels, props }
                if id == "person/ada"
                    && labels.contains("Person")
                    && props.get("name") == Some(&GraphValue::Text("Ada".to_string()))
        ));
        assert!(matches!(
            &create.mutations[2],
            GraphMutation::CreateEdge { id, src, dst, label, props }
                if id == "employment/ada-uldren"
                    && src == "person/ada"
                    && dst == "org/uldren"
                    && label == "WORKS_AT"
                    && props.get("since") == Some(&GraphValue::Int(2026))
        ));

        let mut graph = Graph::new();
        graph.apply_mutations(&create).unwrap();
        let set = GraphMutationPlan::parse_opencypher("SET p.age = 41, r.active = true", &identity)
            .unwrap();
        graph.apply_mutations(&set).unwrap();
        assert_eq!(
            graph.graph_node("person/ada").unwrap().props.get("age"),
            Some(&GraphValue::Int(41))
        );
        assert_eq!(
            graph
                .edge("employment/ada-uldren")
                .unwrap()
                .props
                .get("active"),
            Some(&GraphValue::Bool(true))
        );
        let remove =
            GraphMutationPlan::parse_opencypher("REMOVE p.name, r.active", &identity).unwrap();
        graph.apply_mutations(&remove).unwrap();
        assert!(
            !graph
                .graph_node("person/ada")
                .unwrap()
                .props
                .contains_key("name")
        );
        let delete = GraphMutationPlan::parse_opencypher("DETACH DELETE p", &identity).unwrap();
        graph.apply_mutations(&delete).unwrap();
        assert!(graph.graph_node("person/ada").is_none());
        assert!(graph.edge("employment/ada-uldren").is_none());
    }

    #[test]
    fn opencypher_merge_text_lowers_through_identity_envelope() {
        let identity = GraphMutationIdentity::new(
            BTreeMap::from([
                ("p".to_string(), "person/ada".to_string()),
                ("o".to_string(), "org/uldren".to_string()),
            ]),
            BTreeMap::from([("r".to_string(), "employment/ada-uldren".to_string())]),
        );
        let merge = GraphMutationPlan::parse_opencypher(
            "MERGE (p:Person {name: 'Ada'})-[r:WORKS_AT {since: 2026}]->(o:Organization {name: 'Uldren'})",
            &identity,
        )
        .unwrap();
        assert_eq!(merge.mutations.len(), 3);
        assert!(matches!(
            &merge.mutations[0],
            GraphMutation::MergeNode { id, labels, props }
                if id == "person/ada"
                    && labels.contains("Person")
                    && props.get("name") == Some(&GraphValue::Text("Ada".to_string()))
        ));
        assert!(matches!(
            &merge.mutations[2],
            GraphMutation::MergeEdge { id, src, dst, label, props }
                if id == "employment/ada-uldren"
                    && src == "person/ada"
                    && dst == "org/uldren"
                    && label == "WORKS_AT"
                    && props.get("since") == Some(&GraphValue::Int(2026))
        ));

        let mut graph = Graph::new();
        assert_eq!(graph.apply_mutations(&merge).unwrap().applied, 3);
        assert_eq!(graph.apply_mutations(&merge).unwrap().applied, 3);
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);

        let conflict =
            GraphMutationPlan::parse_opencypher("MERGE (p:Person {name: 'Grace'})", &identity)
                .unwrap();
        assert_eq!(
            graph.apply_mutations(&conflict).unwrap_err().code,
            Code::Conflict
        );
    }

    #[test]
    fn deterministic_opencypher_identity_lowers_create_and_merge_patterns() {
        let create_identity = GraphMutationIdentity::deterministic_opencypher(
            "CREATE (p:Person {name: 'Ada'})-[r:KNOWS {since: 2026}]->(q:Person {name: 'Grace'})",
        )
        .unwrap();
        assert!(create_identity.nodes["p"].starts_with("neo4j/node/"));
        assert!(create_identity.nodes["q"].starts_with("neo4j/node/"));
        assert!(create_identity.edges["r"].starts_with("neo4j/edge/"));

        let repeated = GraphMutationIdentity::deterministic_opencypher(
            "CREATE (p:Person {name: 'Ada'})-[r:KNOWS {since: 2026}]->(q:Person {name: 'Grace'})",
        )
        .unwrap();
        assert_eq!(create_identity, repeated);

        let merge = GraphMutationPlan::parse_opencypher(
            "MERGE (p:Person {name: 'Ada'})-[r:KNOWS {since: 2026}]->(q:Person {name: 'Grace'})",
            &create_identity,
        )
        .unwrap();
        let mut graph = Graph::new();
        assert_eq!(graph.apply_mutations(&merge).unwrap().applied, 3);
        assert_eq!(graph.apply_mutations(&merge).unwrap().applied, 3);
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn deterministic_opencypher_identity_rejects_unkeyed_patterns() {
        assert_eq!(
            GraphMutationIdentity::deterministic_opencypher("CREATE (p:Person)")
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            GraphMutationIdentity::deterministic_opencypher(
                "CREATE (p:Person {name: 'Ada'})-[:KNOWS]->(q:Person {name: 'Grace'})",
            )
            .unwrap_err()
            .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn bounded_opencypher_path_profile_returns_path_values() {
        let mut graph = Graph::new();
        graph
            .upsert_node_with_labels("a", BTreeSet::from(["Start".to_string()]), Props::new())
            .unwrap();
        graph.upsert_node("b", Props::new()).unwrap();
        graph.upsert_node("c", Props::new()).unwrap();
        graph
            .upsert_node_with_labels("d", BTreeSet::from(["End".to_string()]), Props::new())
            .unwrap();
        graph
            .upsert_edge("ab", "a", "b", "REL", Props::new())
            .unwrap();
        graph
            .upsert_edge("ac", "a", "c", "REL", Props::new())
            .unwrap();
        graph
            .upsert_edge("bd", "b", "d", "REL", Props::new())
            .unwrap();
        graph
            .upsert_edge("bc", "b", "c", "REL", Props::new())
            .unwrap();
        graph
            .upsert_edge("cd", "c", "d", "REL", Props::new())
            .unwrap();
        graph
            .upsert_edge("zd", "a", "d", "REL", Props::new())
            .unwrap();
        graph
            .set_node_property(
                "a",
                "tags",
                GraphValue::List(vec![
                    GraphValue::Text("source".to_string()),
                    GraphValue::Text("public".to_string()),
                ]),
            )
            .unwrap();
        graph
            .set_node_property(
                "a",
                "profile",
                GraphValue::Map(BTreeMap::from([("rank".to_string(), GraphValue::Int(1))])),
            )
            .unwrap();

        let fixed = GraphQuery::parse_opencypher(
            "MATCH p = (a:Start)-[r1:REL]->(b)-[r2:REL]->(d:End) RETURN p, r1, r2 ORDER BY p",
        )
        .unwrap();
        let fixed = graph.query(&fixed).unwrap();
        assert_eq!(fixed.rows.len(), 2);
        assert!(matches!(
            fixed.rows[0].get("p"),
            Some(GraphQueryValue::Path(GraphPath { nodes, edges }))
                if nodes.len() == 3 && edges.len() == 2
        ));
        assert!(matches!(
            fixed.rows[0].get("r1"),
            Some(GraphQueryValue::Edge(GraphQueryEdge { id, .. })) if id == "ab"
        ));

        let variable = GraphQuery::parse_opencypher(
            "MATCH p = (a:Start)-[:REL*1..3]->(d:End) RETURN p, length(p) AS hops ORDER BY length(p), p LIMIT 10",
        )
        .unwrap();
        let variable = graph.query(&variable).unwrap();
        assert_eq!(variable.rows.len(), 4);
        let path_lengths: Vec<usize> = variable
            .rows
            .iter()
            .map(|row| match row.get("p").unwrap() {
                GraphQueryValue::Path(path) => path.edges.len(),
                _ => 0,
            })
            .collect();
        assert_eq!(path_lengths, vec![1, 2, 2, 3]);
        let hops: Vec<GraphQueryValue> = variable
            .rows
            .iter()
            .map(|row| row.get("hops").unwrap().clone())
            .collect();
        assert_eq!(
            hops,
            vec![
                GraphQueryValue::Scalar(GraphValue::Int(1)),
                GraphQueryValue::Scalar(GraphValue::Int(2)),
                GraphQueryValue::Scalar(GraphValue::Int(2)),
                GraphQueryValue::Scalar(GraphValue::Int(3)),
            ]
        );

        let shortest = GraphQuery::parse_opencypher(
            "MATCH p = shortestPath((a:Start)-[:REL*1..3]->(d:End)) RETURN p, length(p) AS hops",
        )
        .unwrap();
        let shortest = graph.query(&shortest).unwrap();
        assert_eq!(shortest.rows.len(), 1);
        assert_eq!(
            shortest.rows[0].get("hops"),
            Some(&GraphQueryValue::Scalar(GraphValue::Int(1)))
        );
        let list_map = GraphQuery::parse_opencypher(
            "MATCH p = (a:Start)-[:REL*1..1]->(d:End) \
             RETURN labels(a) AS labels, keys(a) AS keys, properties(a) AS props, \
             nodes(p) AS path_nodes, relationships(p) AS path_edges",
        )
        .unwrap();
        let list_map = graph.query(&list_map).unwrap();
        assert_eq!(list_map.rows.len(), 1);
        assert_eq!(
            list_map.rows[0].get("labels"),
            Some(&GraphQueryValue::List(vec![GraphQueryValue::Scalar(
                GraphValue::Text("Start".to_string())
            )]))
        );
        assert_eq!(
            list_map.rows[0].get("keys"),
            Some(&GraphQueryValue::List(vec![
                GraphQueryValue::Scalar(GraphValue::Text("profile".to_string())),
                GraphQueryValue::Scalar(GraphValue::Text("tags".to_string())),
            ]))
        );
        assert!(matches!(
            list_map.rows[0].get("props"),
            Some(GraphQueryValue::Map(props))
                if props.get("tags")
                    == Some(&GraphQueryValue::Scalar(GraphValue::List(vec![
                        GraphValue::Text("source".to_string()),
                        GraphValue::Text("public".to_string()),
                    ])))
        ));
        assert!(matches!(
            list_map.rows[0].get("path_nodes"),
            Some(GraphQueryValue::List(nodes))
                if matches!(nodes.first(), Some(GraphQueryValue::Node(GraphQueryNode { id, .. })) if id == "a")
        ));
        assert!(matches!(
            list_map.rows[0].get("path_edges"),
            Some(GraphQueryValue::List(edges))
                if matches!(edges.first(), Some(GraphQueryValue::Edge(GraphQueryEdge { id, .. })) if id == "zd")
        ));

        assert_eq!(
            GraphQuery::parse_opencypher("MATCH p = (a)-[:REL*]->(b) RETURN p")
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            GraphQuery::parse_opencypher("MATCH p = (a)-[r:REL*1..2]->(b) RETURN p")
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn opencypher_mutation_text_requires_identity_envelope() {
        let missing = GraphMutationIdentity::default();
        assert_eq!(
            GraphMutationPlan::parse_opencypher("CREATE (p:Person)", &missing)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        let ambiguous = GraphMutationIdentity::new(
            BTreeMap::from([("x".to_string(), "node/x".to_string())]),
            BTreeMap::from([("x".to_string(), "edge/x".to_string())]),
        );
        assert_eq!(
            GraphMutationPlan::parse_opencypher("DELETE x", &ambiguous)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn graph_property_indexes_report_readiness_and_select_ready_indexes() {
        let mut graph = Graph::new();
        graph
            .upsert_node_with_labels(
                "ada",
                BTreeSet::from(["Person".to_string()]),
                BTreeMap::from([
                    ("name".to_string(), GraphValue::Text("Ada".to_string())),
                    ("team".to_string(), GraphValue::Text("Research".to_string())),
                ]),
            )
            .unwrap();
        graph
            .upsert_node_with_labels(
                "grace",
                BTreeSet::from(["Person".to_string()]),
                BTreeMap::from([("name".to_string(), GraphValue::Text("Grace".to_string()))]),
            )
            .unwrap();
        graph
            .upsert_edge(
                "knows",
                "ada",
                "grace",
                "KNOWS",
                BTreeMap::from([("since".to_string(), GraphValue::Int(2026))]),
            )
            .unwrap();

        graph
            .declare_property_index("person_name", GraphIndexEntity::Node, "name")
            .unwrap();
        graph
            .declare_property_index("edge_since", GraphIndexEntity::Edge, "since")
            .unwrap();
        let query =
            GraphQuery::parse_opencypher("MATCH (p:Person) WHERE p.name = 'Ada' RETURN p").unwrap();
        let not_built = graph.explain_query(&query).unwrap();
        assert!(not_built.fallback_scan);
        assert!(matches!(
            not_built.selections.first(),
            Some(GraphQueryIndexSelection {
                index: Some(index),
                status: GraphIndexStatus::NotBuilt,
                ..
            }) if index == "person_name"
        ));

        let reports = graph.rebuild_property_indexes().unwrap();
        assert_eq!(reports.len(), 2);
        assert!(reports.iter().any(|report| {
            report.index.name == "person_name"
                && report.status == GraphIndexStatus::Ready
                && report.entries == 2
                && report.distinct_values == 2
        }));
        assert!(reports.iter().any(|report| {
            report.index.name == "edge_since"
                && report.status == GraphIndexStatus::Ready
                && report.entries == 1
                && report.distinct_values == 1
        }));

        let ready = graph.explain_query(&query).unwrap();
        assert!(!ready.fallback_scan);
        assert!(matches!(
            ready.selections.first(),
            Some(GraphQueryIndexSelection {
                index: Some(index),
                status: GraphIndexStatus::Ready,
                ..
            }) if index == "person_name"
        ));
        let result = graph.query(&query).unwrap();
        assert_eq!(result.rows.len(), 1);
        assert!(matches!(
            result.rows[0].get("p"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, .. })) if id == "ada"
        ));

        graph
            .set_node_property("grace", "name", GraphValue::Text("Ada".to_string()))
            .unwrap();
        let stale = graph.explain_query(&query).unwrap();
        assert!(stale.fallback_scan);
        assert!(matches!(
            stale.selections.first(),
            Some(GraphQueryIndexSelection {
                index: Some(index),
                status: GraphIndexStatus::Stale,
                ..
            }) if index == "person_name"
        ));
        assert_eq!(graph.query(&query).unwrap().rows.len(), 2);
    }

    #[test]
    fn graph_spatial_indexes_report_readiness_and_select_candidates() {
        let mut graph = Graph::new();
        graph
            .upsert_node_with_labels(
                "ada",
                BTreeSet::from(["Person".to_string()]),
                BTreeMap::from([(
                    "loc".to_string(),
                    GraphValue::Geometry(
                        GraphGeometry::point(GraphCrs::Crs84_2d, 12.5, 55.0, None).unwrap(),
                    ),
                )]),
            )
            .unwrap();
        graph
            .upsert_node_with_labels(
                "grace",
                BTreeSet::from(["Person".to_string()]),
                BTreeMap::from([(
                    "loc".to_string(),
                    GraphValue::Geometry(
                        GraphGeometry::point(GraphCrs::Crs84_2d, 13.0, 55.0, None).unwrap(),
                    ),
                )]),
            )
            .unwrap();
        graph
            .declare_spatial_index("person_loc", GraphIndexEntity::Node, "loc")
            .unwrap();
        let query = GraphQuery::parse_opencypher(
            "MATCH (p:Person) WHERE within_bbox(p.loc, 12.0, 54.0, 12.6, 56.0) RETURN p ORDER BY id(p)",
        )
        .unwrap();

        let not_built = graph.explain_query(&query).unwrap();
        assert!(not_built.fallback_scan);
        assert!(matches!(
            not_built.selections.first(),
            Some(GraphQueryIndexSelection {
                index: Some(index),
                status: GraphIndexStatus::NotBuilt,
                ..
            }) if index == "person_loc"
        ));

        let reports = graph.rebuild_spatial_indexes().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].index.name, "person_loc");
        assert_eq!(reports[0].status, GraphIndexStatus::Ready);
        assert_eq!(reports[0].entries, 2);

        let ready = graph.explain_query(&query).unwrap();
        assert!(!ready.fallback_scan);
        assert_eq!(ready.spatial_indexes.len(), 1);
        assert!(matches!(
            ready.selections.first(),
            Some(GraphQueryIndexSelection {
                index: Some(index),
                status: GraphIndexStatus::Ready,
                ..
            }) if index == "person_loc"
        ));
        let result = graph.query(&query).unwrap();
        assert_eq!(result.rows.len(), 1);
        assert!(matches!(
            result.rows[0].get("p"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, .. })) if id == "ada"
        ));

        graph
            .set_node_property(
                "grace",
                "loc",
                GraphValue::Geometry(
                    GraphGeometry::point(GraphCrs::Crs84_2d, 12.55, 55.0, None).unwrap(),
                ),
            )
            .unwrap();
        let stale = graph.explain_query(&query).unwrap();
        assert!(stale.fallback_scan);
        assert!(matches!(
            stale.selections.first(),
            Some(GraphQueryIndexSelection {
                index: Some(index),
                status: GraphIndexStatus::Stale,
                ..
            }) if index == "person_loc"
        ));
        assert_eq!(graph.query(&query).unwrap().rows.len(), 2);
    }

    #[test]
    fn graph_edge_spatial_indexes_select_traversal_candidates() {
        let mut graph = Graph::new();
        graph
            .upsert_node_with_labels("ada", BTreeSet::new(), BTreeMap::new())
            .unwrap();
        graph
            .upsert_node_with_labels("grace", BTreeSet::new(), BTreeMap::new())
            .unwrap();
        graph
            .upsert_node_with_labels("katherine", BTreeSet::new(), BTreeMap::new())
            .unwrap();
        graph
            .upsert_edge(
                "near",
                "ada",
                "grace",
                "ROUTE",
                BTreeMap::from([(
                    "loc".to_string(),
                    GraphValue::Geometry(
                        GraphGeometry::point(GraphCrs::Crs84_2d, 12.5, 55.0, None).unwrap(),
                    ),
                )]),
            )
            .unwrap();
        graph
            .upsert_edge(
                "far",
                "ada",
                "katherine",
                "ROUTE",
                BTreeMap::from([(
                    "loc".to_string(),
                    GraphValue::Geometry(
                        GraphGeometry::point(GraphCrs::Crs84_2d, 13.0, 55.0, None).unwrap(),
                    ),
                )]),
            )
            .unwrap();
        graph
            .declare_spatial_index("route_loc", GraphIndexEntity::Edge, "loc")
            .unwrap();
        graph.rebuild_spatial_indexes().unwrap();

        let query = GraphQuery::parse_opencypher(
            "MATCH (a)-[r:ROUTE]->(b) WHERE within_bbox(r.loc, 12.0, 54.0, 12.6, 56.0) RETURN r ORDER BY id(r)",
        )
        .unwrap();
        let explain = graph.explain_query(&query).unwrap();
        assert!(
            explain
                .selections
                .iter()
                .any(|selection| selection.index.as_deref() == Some("route_loc")
                    && selection.status == GraphIndexStatus::Ready
                    && selection.entity == GraphIndexEntity::Edge)
        );

        let result = graph.query(&query).unwrap();
        assert_eq!(result.rows.len(), 1);
        assert!(matches!(
            result.rows[0].get("r"),
            Some(GraphQueryValue::Edge(GraphQueryEdge { id, .. })) if id == "near"
        ));
    }

    #[test]
    fn graph_full_text_predicate_filters_prepared_hit_ids() {
        let mut graph = Graph::new();
        graph
            .upsert_node_with_labels(
                "ada",
                BTreeSet::from(["Person".to_string()]),
                BTreeMap::from([("name".to_string(), GraphValue::Text("Ada".to_string()))]),
            )
            .unwrap();
        graph
            .upsert_node_with_labels(
                "grace",
                BTreeSet::from(["Person".to_string()]),
                BTreeMap::from([("name".to_string(), GraphValue::Text("Grace".to_string()))]),
            )
            .unwrap();
        let query = GraphQuery {
            patterns: vec![GraphPattern::node_from(GraphNodePattern {
                variable: "p".to_string(),
                labels: BTreeSet::from(["Person".to_string()]),
                id: None,
                props: Props::new(),
            })],
            predicate: GraphPredicate::FullTextMatch {
                binding: "p".to_string(),
                ids: BTreeSet::from(["grace".to_string()]),
            },
            returns: vec![GraphReturn::Binding("p".to_string())],
            order_by: Vec::new(),
            skip: None,
            limit: None,
        };
        let result = graph.query(&query).unwrap();
        assert_eq!(result.rows.len(), 1);
        assert!(matches!(
            result.rows[0].get("p"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, .. })) if id == "grace"
        ));
    }

    #[test]
    fn graph_query_with_full_text_uses_search_hits_as_graph_ids() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([32; 16]))
            .unwrap();
        loom.registry_mut()
            .add_facet(ns, FacetKind::Search)
            .unwrap();

        graph_upsert_node(
            &mut loom,
            ns,
            "people",
            "ada",
            BTreeMap::from([("name".to_string(), GraphValue::Text("Ada".to_string()))]),
        )
        .unwrap();
        graph_upsert_node(
            &mut loom,
            ns,
            "people",
            "grace",
            BTreeMap::from([("name".to_string(), GraphValue::Text("Grace".to_string()))]),
        )
        .unwrap();

        let mut mapping = crate::search::Mapping::new();
        mapping.insert("bio".to_string(), crate::search::FieldMapping::text());
        crate::search::search_create(&mut loom, ns, "people_text", mapping).unwrap();
        let mut ada = crate::search::Document::new();
        ada.insert(
            "bio".to_string(),
            crate::search::FieldValue::Text("analytical engine researcher".to_string()),
        );
        crate::search::search_index(&mut loom, ns, "people_text", b"ada".to_vec(), ada).unwrap();
        let mut grace = crate::search::Document::new();
        grace.insert(
            "bio".to_string(),
            crate::search::FieldValue::Text("compiler and systems pioneer".to_string()),
        );
        crate::search::search_index(&mut loom, ns, "people_text", b"grace".to_vec(), grace)
            .unwrap();

        let query = GraphQuery::parse_opencypher("MATCH (p) RETURN p ORDER BY id(p)").unwrap();
        let request = QueryRequest::new(
            crate::search::Query::Match {
                field: "bio".to_string(),
                text: "compiler".to_string(),
            },
            10,
            0,
        );
        let result =
            graph_query_with_full_text(&loom, ns, "people", &query, "p", "people_text", &request)
                .unwrap();
        assert_eq!(result.rows.len(), 1);
        assert!(matches!(
            result.rows[0].get("p"),
            Some(GraphQueryValue::Node(GraphQueryNode { id, .. })) if id == "grace"
        ));
    }

    #[test]
    fn graph_property_index_catalog_persists_declarations_only() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([30; 16]))
            .unwrap();
        let mut graph = Graph::new();
        graph
            .upsert_node(
                "ada",
                BTreeMap::from([("name".to_string(), GraphValue::Text("Ada".to_string()))]),
            )
            .unwrap();
        graph
            .declare_property_index("person_name", GraphIndexEntity::Node, "name")
            .unwrap();
        graph
            .declare_spatial_index("person_loc", GraphIndexEntity::Node, "loc")
            .unwrap();
        graph.rebuild_property_indexes().unwrap();
        graph.rebuild_spatial_indexes().unwrap();
        put_graph(&mut loom, ns, "g", &graph).unwrap();

        let loaded = get_graph(&loom, ns, "g").unwrap();
        assert_eq!(loaded.property_indexes().len(), 1);
        assert_eq!(loaded.spatial_indexes().len(), 1);
        assert_eq!(
            loaded.property_index_reports()[0].status,
            GraphIndexStatus::NotBuilt
        );
        assert_eq!(
            loaded.spatial_index_reports()[0].status,
            GraphIndexStatus::NotBuilt
        );
        let query =
            GraphQuery::parse_opencypher("MATCH (p) WHERE p.name = 'Ada' RETURN p").unwrap();
        let explain = loaded.explain_query(&query).unwrap();
        assert!(explain.fallback_scan);
        assert!(matches!(
            explain.selections.first(),
            Some(GraphQueryIndexSelection {
                index: Some(index),
                status: GraphIndexStatus::NotBuilt,
                ..
            }) if index == "person_name"
        ));
    }

    #[test]
    fn graph_component_roots_are_reachable_from_commits_and_live_set() {
        use std::collections::BTreeSet;

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([20; 16]))
            .unwrap();
        for i in 0..96 {
            graph_upsert_node(&mut loom, ns, "g", &format!("n{i:03}"), Props::new()).unwrap();
        }
        for i in 0..95 {
            graph_upsert_edge(
                &mut loom,
                ns,
                "g",
                &format!("e{i:03}"),
                &format!("n{i:03}"),
                &format!("n{:03}", i + 1),
                "next",
                Props::new(),
            )
            .unwrap();
        }
        let roots = graph_component_roots(&loom, ns, "g");
        let commit = loom.commit(ns, "nas", "graph reachability", 1).unwrap();
        let reachable = loom.reachable(&[commit], &BTreeSet::new()).unwrap();
        let live = loom.live_object_set(None).unwrap();
        for root in roots.values() {
            let nodes = crate::prolly::reachable_nodes(loom.store(), root).unwrap();
            assert!(nodes.iter().all(|node| reachable.contains(node)));
            assert!(nodes.iter().all(|node| live.contains(node)));
        }
    }

    #[test]
    fn graph_node_component_shares_prolly_nodes_after_one_node_edit() {
        use std::collections::BTreeSet;

        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([21; 16]))
            .unwrap();
        for i in 0..512 {
            graph_upsert_node(&mut loom, ns, "g", &format!("n{i:04}"), Props::new()).unwrap();
        }
        let before_root = graph_component_roots(&loom, ns, "g")[GRAPH_NODES_ENTRY];
        let before_nodes = crate::prolly::reachable_nodes(loom.store(), &before_root)
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>();

        graph_upsert_node(
            &mut loom,
            ns,
            "g",
            "n0256",
            BTreeMap::from([("kind".to_string(), GraphValue::Text("changed".to_string()))]),
        )
        .unwrap();
        let after_root = graph_component_roots(&loom, ns, "g")[GRAPH_NODES_ENTRY];
        let after_nodes = crate::prolly::reachable_nodes(loom.store(), &after_root)
            .unwrap()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let shared = before_nodes.intersection(&after_nodes).count();
        let changed = before_nodes.symmetric_difference(&after_nodes).count();

        assert!(
            before_nodes.len() > 8,
            "test must exercise a multi-node graph component"
        );
        assert!(
            shared > changed,
            "expected graph node map to share most prolly nodes after one edit: shared={shared}, changed={changed}"
        );
    }

    #[test]
    fn structured_graph_staging_survives_engine_state_roundtrip() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([19; 16]))
            .unwrap();
        graph_upsert_node(
            &mut loom,
            ns,
            "g",
            "a",
            BTreeMap::from([("kind".to_string(), GraphValue::Text("person".to_string()))]),
        )
        .unwrap();
        let state = loom.export_state();
        let mut restored = Loom::new(loom.store().clone());
        restored.import_state(&state).unwrap();
        assert_eq!(
            graph_get_node(&restored, ns, "g", "a")
                .unwrap()
                .unwrap()
                .get("kind"),
            Some(&GraphValue::Text("person".to_string()))
        );
    }

    fn graph_component_roots<S: ObjectStore>(
        loom: &Loom<S>,
        ns: WorkspaceId,
        name: &str,
    ) -> BTreeMap<&'static str, crate::Digest> {
        let path = graph_path(name);
        let StagedEntry::Graph(root) = loom.work.get(&ns).unwrap().get(&path).unwrap() else {
            panic!("graph path must stage as a graph root");
        };
        let Object::Tree(entries) = loom.get_object(root).unwrap() else {
            panic!("graph root must be a Tree");
        };
        entries
            .into_iter()
            .filter_map(|entry| {
                if entry.kind != EntryKind::ProllyMap {
                    return None;
                }
                let name = match entry.name.as_str() {
                    GRAPH_NODES_ENTRY => GRAPH_NODES_ENTRY,
                    GRAPH_EDGES_ENTRY => GRAPH_EDGES_ENTRY,
                    GRAPH_FORWARD_ADJ_ENTRY => GRAPH_FORWARD_ADJ_ENTRY,
                    GRAPH_REVERSE_ADJ_ENTRY => GRAPH_REVERSE_ADJ_ENTRY,
                    _ => return None,
                };
                Some((name, entry.target))
            })
            .collect()
    }

    #[test]
    fn facade_node_edge_ops() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([9; 16]))
            .unwrap();
        // An absent graph reads as empty, not an error.
        assert_eq!(graph_get_node(&loom, ns, "g", "a").unwrap(), None);
        assert!(graph_neighbors(&loom, ns, "g", "a").unwrap().is_empty());
        // The first write creates the graph.
        graph_upsert_node(&mut loom, ns, "g", "a", props(&[("k", b"1")])).unwrap();
        graph_upsert_node(&mut loom, ns, "g", "b", Props::new()).unwrap();
        graph_upsert_edge(&mut loom, ns, "g", "e", "a", "b", "rel", Props::new()).unwrap();
        assert_eq!(
            graph_get_node(&loom, ns, "g", "a").unwrap().unwrap()["k"],
            GraphValue::Bytes(b"1".to_vec())
        );
        assert_eq!(
            graph_neighbors(&loom, ns, "g", "a").unwrap(),
            vec!["b".to_string()]
        );
        assert_eq!(graph_out_edges(&loom, ns, "g", "a").unwrap().len(), 1);
        assert_eq!(graph_in_edges(&loom, ns, "g", "b").unwrap().len(), 1);
        assert_eq!(graph_edges(&loom, ns, "g").unwrap().len(), 1);
        // A missing endpoint is rejected through the facade.
        assert_eq!(
            graph_upsert_edge(&mut loom, ns, "g", "x", "a", "ghost", "r", Props::new())
                .unwrap_err()
                .code,
            Code::NotFound
        );
        // Cascade semantics flow through.
        assert_eq!(
            graph_remove_node(&mut loom, ns, "g", "a", false)
                .unwrap_err()
                .code,
            Code::Conflict
        );
        assert!(graph_remove_edge(&mut loom, ns, "g", "e").unwrap());
        assert!(!graph_remove_edge(&mut loom, ns, "g", "e").unwrap());
        graph_remove_node(&mut loom, ns, "g", "a", false).unwrap();
        assert_eq!(graph_get_node(&loom, ns, "g", "a").unwrap(), None);
    }

    #[test]
    fn authenticated_graph_operations_honor_collection_scopes() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([31; 16]))
            .unwrap();
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = crate::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);
        loom.acl_store_mut()
            .grant(crate::AclGrant {
                subject: crate::AclSubject::Principal(root),
                workspace: Some(ns),
                domain: Some(FacetKind::Graph.into()),
                ref_glob: None,
                scopes: vec![crate::AclScope::Prefix {
                    kind: crate::AclScopeKind::Collection,
                    prefix: b"work".to_vec(),
                }],
                rights: [crate::AclRight::Write, crate::AclRight::Read]
                    .into_iter()
                    .collect(),
                effect: crate::AclEffect::Allow,
                predicate: None,
            })
            .unwrap();

        graph_upsert_node(&mut loom, ns, "work", "a", Props::new()).unwrap();
        let mutation = GraphMutationPlan::new(vec![GraphMutation::CreateNode {
            id: "b".to_string(),
            labels: BTreeSet::new(),
            props: Props::new(),
        }]);
        assert_eq!(
            graph_apply_mutations(&mut loom, ns, "work", &mutation)
                .unwrap()
                .applied,
            1
        );
        assert!(graph_get_node(&loom, ns, "work", "b").unwrap().is_some());
        assert!(graph_get_node(&loom, ns, "work", "a").unwrap().is_some());
        assert_eq!(
            graph_apply_mutations(&mut loom, ns, "private", &mutation)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
    }

    #[test]
    fn query_reachable_and_shortest_path() {
        // a -> b -> c -> d, plus a -> d via a long "rel" path and a direct "shortcut" edge.
        let mut g = Graph::new();
        for id in ["a", "b", "c", "d"] {
            g.upsert_node(id, Props::new()).unwrap();
        }
        g.upsert_edge("e1", "a", "b", "rel", Props::new()).unwrap();
        g.upsert_edge("e2", "b", "c", "rel", Props::new()).unwrap();
        g.upsert_edge("e3", "c", "d", "rel", Props::new()).unwrap();
        g.upsert_edge("e4", "a", "d", "shortcut", Props::new())
            .unwrap();

        // Reachability is directed and sorted; bounded by depth.
        assert_eq!(g.reachable("a", None, None), vec!["b", "c", "d"]);
        assert_eq!(g.reachable("a", Some(1), None), vec!["b", "d"]);
        // Label restriction follows only "rel" edges.
        assert_eq!(g.reachable("a", None, Some("rel")), vec!["b", "c", "d"]);
        assert!(g.reachable("d", None, None).is_empty());
        assert!(g.reachable("ghost", None, None).is_empty());

        // Shortest path prefers the direct shortcut; restricting to "rel" forces the long path.
        assert_eq!(
            g.shortest_path("a", "d", None),
            Some(vec!["a".into(), "d".into()])
        );
        assert_eq!(
            g.shortest_path("a", "d", Some("rel")),
            Some(vec!["a".into(), "b".into(), "c".into(), "d".into()])
        );
        assert_eq!(g.shortest_path("d", "a", None), None);
        assert_eq!(g.shortest_path("a", "a", None), Some(vec!["a".into()]));
    }
}
