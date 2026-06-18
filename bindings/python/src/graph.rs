//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Encode a property bag as a Loom Canonical CBOR map of `text -> value`.
fn graph_value_to_cbor(value: &loom_core::GraphValue) -> CborValue {
    loom_wire::graph::graph_value_to_cbor(value)
}

fn graph_value_from_cbor(value: CborValue) -> PyResult<loom_core::GraphValue> {
    loom_wire::graph::graph_value_from_cbor(value)
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))
}

fn props_to_cbor(props: &loom_core::Props) -> Vec<u8> {
    let pairs = props
        .iter()
        .map(|(k, v)| (CborValue::Text(k.clone()), graph_value_to_cbor(v)))
        .collect();
    cbor_encode(&CborValue::Map(pairs)).unwrap_or_default()
}

/// Decode a Loom Canonical CBOR map of `text -> value` into a property bag; an empty buffer is no props.
fn props_from_cbor(bytes: &[u8]) -> PyResult<loom_core::Props> {
    if bytes.is_empty() {
        return Ok(loom_core::Props::new());
    }
    let value =
        loom_codec::decode(bytes).map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))?;
    let CborValue::Map(pairs) = value else {
        return Err(PyRuntimeError::new_err("graph props must be a CBOR map"));
    };
    let mut props = loom_core::Props::new();
    for (k, v) in pairs {
        let CborValue::Text(key) = k else {
            return Err(PyRuntimeError::new_err("graph prop key must be text"));
        };
        props.insert(key, graph_value_from_cbor(v)?);
    }
    Ok(props)
}

/// The CBOR array `[src, dst, label, props]` for an edge.
fn edge_value(edge: &loom_core::Edge) -> CborValue {
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

/// Encode a list of node-id text strings as a CBOR array.
fn strings_array_cbor(ids: Vec<String>) -> Vec<u8> {
    let items = ids.into_iter().map(CborValue::Text).collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

/// Encode `(edge_id, edge)` pairs as a CBOR array of `[edge_id, [src, dst, label, props]]`.
fn edges_array_cbor(edges: Vec<(String, loom_core::Edge)>) -> Vec<u8> {
    let items = edges
        .into_iter()
        .map(|(eid, e)| CborValue::Array(vec![CborValue::Text(eid), edge_value(&e)]))
        .collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

/// Insert or replace node `id` (with `props` as a CBOR `text -> scalar` map, or empty for none) in graph
/// `name` of `workspace` (UUID or name, created with the `graph` facet if absent).
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, props, passphrase=None))]
pub(crate) fn graph_upsert_node(
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    props: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let props = props_from_cbor(props)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_graph_ns(&mut loom, workspace)?;
    loom_core::graph_upsert_node(&mut loom, ns, name, id, props).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Fetch node `id`'s properties in graph `name` as a CBOR `text -> scalar` map, or `None` when the node or
/// graph is absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, passphrase=None))]
pub(crate) fn graph_get_node<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::graph_get_node(&loom, ns, name, id)
        .map_err(py_err)?
        .map(|p| PyBytes::new(py, &props_to_cbor(&p))))
}
/// Remove node `id` from graph `name`. `cascade` false rejects with `CONFLICT` while incident edges
/// exist; `cascade` true removes the node and its incident edges. `NOT_FOUND` if the node is absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, cascade, passphrase=None))]
pub(crate) fn graph_remove_node(
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    cascade: bool,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_core::graph_remove_node(&mut loom, ns, name, id, cascade).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Insert or replace edge `id` from `src` to `dst` (both endpoints must already exist, else `NOT_FOUND`)
/// with `label` and `props` (CBOR `text -> scalar` map, or empty) in graph `name`.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, src, dst, label, props, passphrase=None))]
pub(crate) fn graph_upsert_edge(
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    src: &str,
    dst: &str,
    label: &str,
    props: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let props = props_from_cbor(props)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_graph_ns(&mut loom, workspace)?;
    loom_core::graph_upsert_edge(&mut loom, ns, name, id, src, dst, label, props)
        .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Fetch edge `id` in graph `name` as the CBOR array `[src, dst, label, props]`, or `None` when the edge
/// or graph is absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, passphrase=None))]
pub(crate) fn graph_get_edge<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::graph_get_edge(&loom, ns, name, id)
        .map_err(py_err)?
        .map(|e| PyBytes::new(py, &cbor_encode(&edge_value(&e)).unwrap_or_default())))
}
/// Remove edge `id` from graph `name`; returns whether it was present. An absent edge or graph is a no-op.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, passphrase=None))]
pub(crate) fn graph_remove_edge(
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let present = loom_core::graph_remove_edge(&mut loom, ns, name, id).map_err(py_err)?;
    if present {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(present)
}
/// The distinct adjacent node ids of `id` in graph `name`, sorted, as a CBOR array of text (empty when
/// the node or graph is absent).
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, passphrase=None))]
pub(crate) fn graph_neighbors<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes =
        strings_array_cbor(loom_core::graph_neighbors(&loom, ns, name, id).map_err(py_err)?);
    Ok(PyBytes::new(py, &bytes))
}
/// Out-edges of `id` in graph `name` as a CBOR array of `[edge_id, [src, dst, label, props]]` in
/// edge-id order.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, passphrase=None))]
pub(crate) fn graph_out_edges<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = edges_array_cbor(loom_core::graph_out_edges(&loom, ns, name, id).map_err(py_err)?);
    Ok(PyBytes::new(py, &bytes))
}
/// In-edges of `id` in graph `name` as a CBOR array of `[edge_id, [src, dst, label, props]]` in
/// edge-id order.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, passphrase=None))]
pub(crate) fn graph_in_edges<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    id: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = edges_array_cbor(loom_core::graph_in_edges(&loom, ns, name, id).map_err(py_err)?);
    Ok(PyBytes::new(py, &bytes))
}
/// Node ids reachable from `start` in graph `name` as a CBOR array of text. `max_depth` below `0` is no
/// limit; `via_label` `None` follows every edge, else only edges with that label.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, start, max_depth, via_label=None, passphrase=None))]
pub(crate) fn graph_reachable<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    start: &str,
    max_depth: i64,
    via_label: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let depth = (max_depth >= 0).then_some(max_depth as usize);
    let bytes = strings_array_cbor(
        loom_core::graph_reachable(&loom, ns, name, start, depth, via_label).map_err(py_err)?,
    );
    Ok(PyBytes::new(py, &bytes))
}
/// A shortest directed path from `from` to `to` in graph `name` as a CBOR array of node-id text, or
/// `None` when no path exists or an endpoint or the graph is absent. `via_label` `None` follows every
/// edge, else only edges with that label.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, from, to, via_label=None, passphrase=None))]
pub(crate) fn graph_shortest_path<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    from: &str,
    to: &str,
    via_label: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(
        loom_core::graph_shortest_path(&loom, ns, name, from, to, via_label)
            .map_err(py_err)?
            .map(|p| PyBytes::new(py, &strings_array_cbor(p))),
    )
}

#[pyfunction]
#[pyo3(signature = (path, workspace, name, query, passphrase=None))]
pub(crate) fn graph_query<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    query: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let query = loom_core::GraphQuery::parse_opencypher(query).map_err(py_err)?;
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let result = loom_core::graph_query(&loom, ns, name, &query).map_err(py_err)?;
    Ok(PyBytes::new(
        py,
        &loom_wire::graph::graph_query_result_to_cbor(&result),
    ))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, name, query, passphrase=None))]
pub(crate) fn graph_explain_query<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    query: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let query = loom_core::GraphQuery::parse_opencypher(query).map_err(py_err)?;
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let explain = loom_core::graph_explain_query(&loom, ns, name, &query).map_err(py_err)?;
    Ok(PyBytes::new(
        py,
        &loom_wire::graph::graph_query_explain_to_cbor(&explain),
    ))
}
