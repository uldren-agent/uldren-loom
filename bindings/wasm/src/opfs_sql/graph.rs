//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::{
    Edge, GraphQuery, Props, graph_explain_query, graph_get_edge, graph_get_node, graph_in_edges,
    graph_neighbors, graph_out_edges, graph_query, graph_reachable, graph_remove_edge,
    graph_remove_node, graph_shortest_path, graph_upsert_edge, graph_upsert_node,
};

// Property graph (Graph facet) - nodes and directed labelled edges in a named graph within a
// workspace. Node/edge properties cross as a Loom Canonical CBOR map of `text -> value`; an edge
// crosses as the CBOR array `[src, dst, label, props]`; neighbour/reachability/path results cross as
// a CBOR array of node-id text; out-/in-edge results cross as a CBOR array of `[edge_id, edge]` pairs
// in edge-id order.

/// Resolve a workspace for a graph write by UUID or name, ensuring the `graph` facet exists.
fn ensure_graph_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Graph,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Graph)
        .map_err(le)?;
    Ok(ns)
}

/// Encode a [`Props`] map as the canonical-CBOR `text -> value` map wire form.
fn graph_value_to_cbor(value: &loom_core::GraphValue) -> CborValue {
    loom_wire::graph::graph_value_to_cbor(value)
}

fn graph_value_from_cbor(value: CborValue) -> Result<loom_core::GraphValue, JsError> {
    loom_wire::graph::graph_value_from_cbor(value).map_err(|err| JsError::new(&err.to_string()))
}

fn props_to_cbor(props: &Props) -> Vec<u8> {
    let pairs = props
        .iter()
        .map(|(k, v)| (CborValue::Text(k.clone()), graph_value_to_cbor(v)))
        .collect();
    cbor_encode(&CborValue::Map(pairs)).unwrap_or_default()
}

/// Decode a canonical-CBOR `text -> value` map (empty buffer is the empty map) into a [`Props`].
fn props_from_cbor(bytes: &[u8]) -> Result<Props, JsError> {
    if bytes.is_empty() {
        return Ok(Props::new());
    }
    let value = loom_codec::decode(bytes).map_err(|e| JsError::new(&format!("cbor: {e}")))?;
    let CborValue::Map(pairs) = value else {
        return Err(JsError::new("graph props must be a CBOR map"));
    };
    let mut props = Props::new();
    for (k, v) in pairs {
        let CborValue::Text(key) = k else {
            return Err(JsError::new("graph prop key must be text"));
        };
        props.insert(key, graph_value_from_cbor(v)?);
    }
    Ok(props)
}

/// One [`Edge`] as the canonical-CBOR array `[src, dst, label, props]`.
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

/// Encode a list of node-id text as a canonical-CBOR array of text.
fn strings_array_cbor(ids: Vec<String>) -> Vec<u8> {
    let items = ids.into_iter().map(CborValue::Text).collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

/// Encode `[edge_id, edge]` pairs as a canonical-CBOR array of `[edge_id, [src, dst, label, props]]`.
fn edges_array_cbor(edges: Vec<(String, Edge)>) -> Vec<u8> {
    let items = edges
        .into_iter()
        .map(|(eid, e)| CborValue::Array(vec![CborValue::Text(eid), edge_value(&e)]))
        .collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

#[wasm_bindgen]
impl LoomSql {
    /// Insert or replace node `id` (with `props` as a CBOR `text -> scalar` map, or empty for none) in
    /// graph `name` of `workspace` (UUID or name, created with the `graph` facet if absent).
    pub fn graph_upsert_node(
        &mut self,
        workspace: String,
        name: String,
        id: String,
        props: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let props = props_from_cbor(&props)?;
        let ns = ensure_graph_ns(&mut self.loom, &workspace)?;
        graph_upsert_node(&mut self.loom, ns, &name, &id, props).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Fetch node `id`'s properties in graph `name` as a CBOR `text -> scalar` map, or null if absent.
    pub fn graph_get_node(
        &self,
        workspace: String,
        name: String,
        id: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(graph_get_node(&self.loom, ns, &name, &id)
            .map_err(le)?
            .map(|p| Uint8Array::from(props_to_cbor(&p).as_slice())))
    }

    /// Remove node `id` from graph `name`. `cascade=false` rejects while incident edges exist;
    /// `cascade=true` removes the node and its incident edges.
    pub fn graph_remove_node(
        &mut self,
        workspace: String,
        name: String,
        id: String,
        cascade: bool,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        graph_remove_node(&mut self.loom, ns, &name, &id, cascade).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Insert or replace edge `id` from `src` to `dst` (both endpoints must already exist) with
    /// `label` and `props` (CBOR `text -> scalar` map, or empty) in graph `name`.
    pub fn graph_upsert_edge(
        &mut self,
        workspace: String,
        name: String,
        id: String,
        src: String,
        dst: String,
        label: String,
        props: Vec<u8>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let props = props_from_cbor(&props)?;
        let ns = ensure_graph_ns(&mut self.loom, &workspace)?;
        graph_upsert_edge(&mut self.loom, ns, &name, &id, &src, &dst, &label, props).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Fetch edge `id` in graph `name` as the CBOR array `[src, dst, label, props]`, or null if absent.
    pub fn graph_get_edge(
        &self,
        workspace: String,
        name: String,
        id: String,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(graph_get_edge(&self.loom, ns, &name, &id)
            .map_err(le)?
            .map(|e| Uint8Array::from(cbor_encode(&edge_value(&e)).unwrap_or_default().as_slice())))
    }

    /// Remove edge `id` from graph `name`; returns whether it was present.
    pub fn graph_remove_edge(
        &mut self,
        workspace: String,
        name: String,
        id: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let present = graph_remove_edge(&mut self.loom, ns, &name, &id).map_err(le)?;
        if present {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(present)
    }

    /// The distinct adjacent node ids of `id` in graph `name`, sorted, as a CBOR array of text (the
    /// empty array when the node or graph is absent).
    pub fn graph_neighbors(
        &self,
        workspace: String,
        name: String,
        id: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(strings_array_cbor(
            graph_neighbors(&self.loom, ns, &name, &id).map_err(le)?,
        ))
    }

    /// Out-edges of `id` in graph `name` as a CBOR array of `[edge_id, [src, dst, label, props]]` in
    /// edge-id order.
    pub fn graph_out_edges(
        &self,
        workspace: String,
        name: String,
        id: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(edges_array_cbor(
            graph_out_edges(&self.loom, ns, &name, &id).map_err(le)?,
        ))
    }

    /// In-edges of `id` in graph `name` as a CBOR array of `[edge_id, [src, dst, label, props]]` in
    /// edge-id order.
    pub fn graph_in_edges(
        &self,
        workspace: String,
        name: String,
        id: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(edges_array_cbor(
            graph_in_edges(&self.loom, ns, &name, &id).map_err(le)?,
        ))
    }

    /// Node ids reachable from `start` in graph `name` as a CBOR array of text. A null `max_depth` is
    /// no limit; a null `via_label` follows every edge, else only edges with that label.
    pub fn graph_reachable(
        &self,
        workspace: String,
        name: String,
        start: String,
        max_depth: Option<usize>,
        via_label: Option<String>,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(strings_array_cbor(
            graph_reachable(
                &self.loom,
                ns,
                &name,
                &start,
                max_depth,
                via_label.as_deref(),
            )
            .map_err(le)?,
        ))
    }

    /// A shortest directed path from `from` to `to` in graph `name` as a CBOR array of node-id text,
    /// or null if no path exists (or an endpoint or graph is missing). A null `via_label` follows
    /// every edge, else only edges with that label.
    pub fn graph_shortest_path(
        &self,
        workspace: String,
        name: String,
        from: String,
        to: String,
        via_label: Option<String>,
    ) -> Result<Option<Uint8Array>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(
            graph_shortest_path(&self.loom, ns, &name, &from, &to, via_label.as_deref())
                .map_err(le)?
                .map(|path| Uint8Array::from(strings_array_cbor(path).as_slice())),
        )
    }

    pub fn graph_query(
        &self,
        workspace: String,
        name: String,
        query: String,
    ) -> Result<Vec<u8>, JsError> {
        let query = GraphQuery::parse_opencypher(&query).map_err(le)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let result = graph_query(&self.loom, ns, &name, &query).map_err(le)?;
        Ok(loom_wire::graph::graph_query_result_to_cbor(&result))
    }

    pub fn graph_explain_query(
        &self,
        workspace: String,
        name: String,
        query: String,
    ) -> Result<Vec<u8>, JsError> {
        let query = GraphQuery::parse_opencypher(&query).map_err(le)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let explain = graph_explain_query(&self.loom, ns, &name, &query).map_err(le)?;
        Ok(loom_wire::graph::graph_query_explain_to_cbor(&explain))
    }
}
