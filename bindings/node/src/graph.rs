//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::{Edge, GraphValue, Props};

// Node/edge properties cross as a Loom Canonical CBOR map of `text -> value`; an edge crosses as the
// CBOR array `[src, dst, label, props]`; neighbour/reachability/path results cross as a CBOR array of
// node-id text; out-/in-edge results cross as a CBOR array of `[edge_id, edge]` pairs in edge-id order.

fn graph_value_to_cbor(value: &GraphValue) -> CborValue {
    loom_wire::graph::graph_value_to_cbor(value)
}

fn graph_value_from_cbor(value: CborValue) -> napi::Result<GraphValue> {
    loom_wire::graph::graph_value_from_cbor(value)
        .map_err(|err| napi::Error::from_reason(err.to_string()))
}

fn props_to_cbor(props: &Props) -> Vec<u8> {
    let pairs = props
        .iter()
        .map(|(k, v)| (CborValue::Text(k.clone()), graph_value_to_cbor(v)))
        .collect();
    cbor_encode(&CborValue::Map(pairs)).unwrap_or_default()
}

fn props_from_cbor(bytes: &[u8]) -> napi::Result<Props> {
    if bytes.is_empty() {
        return Ok(Props::new());
    }
    let value =
        loom_codec::decode(bytes).map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))?;
    let CborValue::Map(pairs) = value else {
        return Err(napi::Error::from_reason("graph props must be a CBOR map"));
    };
    let mut props = Props::new();
    for (k, v) in pairs {
        let CborValue::Text(key) = k else {
            return Err(napi::Error::from_reason("graph prop key must be text"));
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

fn strings_array_cbor(ids: Vec<String>) -> Vec<u8> {
    let items = ids.into_iter().map(CborValue::Text).collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

fn edges_array_cbor(edges: Vec<(String, Edge)>) -> Vec<u8> {
    let items = edges
        .into_iter()
        .map(|(eid, e)| CborValue::Array(vec![CborValue::Text(eid), edge_value(&e)]))
        .collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

/// Insert or replace node `id` (with `props` as a CBOR `text -> scalar` map, or empty for none) in graph
/// `name` of workspace `workspace` (UUID or name, created with the `graph` facet if absent).
#[napi]
pub fn graph_upsert_node(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    props: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let props = props_from_cbor(&props)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_graph_ns(&mut loom, &workspace)?;
    loom_core::graph_upsert_node(&mut loom, ns, &name, &id, props).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Fetch node `id`'s properties in graph `name` as a CBOR `text -> scalar` map, or `null` if the node or
/// graph is absent.
#[napi]
pub fn graph_get_node(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::graph_get_node(&loom, ns, &name, &id)
        .map_err(reason)?
        .map(|p| Uint8Array::from(props_to_cbor(&p))))
}
/// Remove node `id` from graph `name`. `cascade=0` rejects with `CONFLICT` while incident edges exist;
/// `cascade!=0` removes the node and its incident edges. `NOT_FOUND` if the node is absent.
#[napi]
pub fn graph_remove_node(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    cascade: i32,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    loom_core::graph_remove_node(&mut loom, ns, &name, &id, cascade != 0).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Insert or replace edge `id` from `src` to `dst` (both endpoints must already exist, else `NOT_FOUND`)
/// with `label` and `props` (CBOR `text -> scalar` map, or empty) in graph `name`.
#[napi]
pub fn graph_upsert_edge(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    src: String,
    dst: String,
    label: String,
    props: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let props = props_from_cbor(&props)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = ensure_graph_ns(&mut loom, &workspace)?;
    loom_core::graph_upsert_edge(&mut loom, ns, &name, &id, &src, &dst, &label, props)
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Fetch edge `id` in graph `name` as the CBOR array `[src, dst, label, props]`, or `null` if the edge or
/// graph is absent.
#[napi]
pub fn graph_get_edge(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(loom_core::graph_get_edge(&loom, ns, &name, &id)
        .map_err(reason)?
        .map(|e| Uint8Array::from(cbor_encode(&edge_value(&e)).unwrap_or_default())))
}
/// Remove edge `id` from graph `name`; returns whether it was present. An absent edge or graph is a no-op.
#[napi]
pub fn graph_remove_edge(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let present = loom_core::graph_remove_edge(&mut loom, ns, &name, &id).map_err(reason)?;
    if present {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(present)
}
/// The distinct adjacent node ids of `id` in graph `name`, sorted, as the Loom Canonical CBOR array of
/// text (empty when the node or graph is absent).
#[napi]
pub fn graph_neighbors(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(strings_array_cbor(
        loom_core::graph_neighbors(&loom, ns, &name, &id).map_err(reason)?,
    )))
}
/// Out-edges of `id` in graph `name` as the Loom Canonical CBOR array of `[edge_id, [src, dst, label,
/// props]]` in edge-id order.
#[napi]
pub fn graph_out_edges(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(edges_array_cbor(
        loom_core::graph_out_edges(&loom, ns, &name, &id).map_err(reason)?,
    )))
}
/// In-edges of `id` in graph `name` as the Loom Canonical CBOR array of `[edge_id, [src, dst, label,
/// props]]` in edge-id order.
#[napi]
pub fn graph_in_edges(
    loom_path: String,
    workspace: String,
    name: String,
    id: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(edges_array_cbor(
        loom_core::graph_in_edges(&loom, ns, &name, &id).map_err(reason)?,
    )))
}
/// Node ids reachable from `start` in graph `name` as the Loom Canonical CBOR array of text.
/// `maxDepth < 0` is no limit; `viaLabel` empty follows every edge, else only edges with that label.
#[napi]
pub fn graph_reachable(
    loom_path: String,
    workspace: String,
    name: String,
    start: String,
    max_depth: i64,
    via_label: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let via = (!via_label.is_empty()).then_some(via_label.as_str());
    let depth = (max_depth >= 0).then_some(max_depth as usize);
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(Uint8Array::from(strings_array_cbor(
        loom_core::graph_reachable(&loom, ns, &name, &start, depth, via).map_err(reason)?,
    )))
}
/// A shortest directed path from `from` to `to` in graph `name` as the Loom Canonical CBOR array of
/// node-id text, or `null` if no path exists (or an endpoint or the graph is absent). `viaLabel` empty
/// follows every edge, else only edges with that label.
#[napi]
pub fn graph_shortest_path(
    loom_path: String,
    workspace: String,
    name: String,
    from: String,
    to: String,
    via_label: String,
    passphrase: Option<String>,
) -> napi::Result<Option<Uint8Array>> {
    let via = (!via_label.is_empty()).then_some(via_label.as_str());
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    Ok(
        loom_core::graph_shortest_path(&loom, ns, &name, &from, &to, via)
            .map_err(reason)?
            .map(|path| Uint8Array::from(strings_array_cbor(path))),
    )
}

#[napi]
pub fn graph_query(
    loom_path: String,
    workspace: String,
    name: String,
    query: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let query = loom_core::GraphQuery::parse_opencypher(&query).map_err(reason)?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let result = loom_core::graph_query(&loom, ns, &name, &query).map_err(reason)?;
    Ok(Uint8Array::from(
        loom_wire::graph::graph_query_result_to_cbor(&result),
    ))
}

#[napi]
pub fn graph_explain_query(
    loom_path: String,
    workspace: String,
    name: String,
    query: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let query = loom_core::GraphQuery::parse_opencypher(&query).map_err(reason)?;
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let explain = loom_core::graph_explain_query(&loom, ns, &name, &query).map_err(reason)?;
    Ok(Uint8Array::from(
        loom_wire::graph::graph_query_explain_to_cbor(&explain),
    ))
}
