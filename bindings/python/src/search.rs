//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_core::{Document, FieldValue, Mapping, QueryRequest, QueryResponse};

/// Resolve a workspace for a search write by UUID or name, ensuring the `search` facet exists.
fn ensure_search_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Search,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Search)
        .map_err(py_err)?;
    Ok(ns)
}

/// Decode a CBOR map of `field -> [type_tag, stored, faceted]` into a field mapping.
fn mapping_from_cbor(bytes: &[u8]) -> PyResult<Mapping> {
    loom_core::search_mapping_from_cbor(bytes)
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))
}

/// The CBOR encoding of a document field value: text stays text, bytes stay bytes.
fn field_value_cbor(value: &FieldValue) -> CborValue {
    match value {
        FieldValue::Text(s) => CborValue::Text(s.clone()),
        FieldValue::Bytes(b) => CborValue::Bytes(b.clone()),
    }
}

/// Encode a document as a CBOR map of `field -> value` (each value text or bytes).
fn document_to_cbor(doc: &Document) -> Vec<u8> {
    let pairs = doc
        .iter()
        .map(|(field, v)| (CborValue::Text(field.clone()), field_value_cbor(v)))
        .collect();
    cbor_encode(&CborValue::Map(pairs)).unwrap_or_default()
}

/// Decode a CBOR map of `field -> value` into a document (each value text or bytes).
fn document_from_cbor(bytes: &[u8]) -> PyResult<Document> {
    let value =
        loom_codec::decode(bytes).map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))?;
    let CborValue::Map(pairs) = value else {
        return Err(PyRuntimeError::new_err(
            "search document must be a CBOR map",
        ));
    };
    let mut doc = Document::new();
    for (k, v) in pairs {
        let CborValue::Text(field) = k else {
            return Err(PyRuntimeError::new_err(
                "search document field name must be text",
            ));
        };
        let value = match v {
            CborValue::Text(s) => FieldValue::Text(s),
            CborValue::Bytes(b) => FieldValue::Bytes(b),
            _ => {
                return Err(PyRuntimeError::new_err(
                    "search document value must be text or bytes",
                ));
            }
        };
        doc.insert(field, value);
    }
    Ok(doc)
}

/// Decode the request CBOR array `[query, limit, offset]` into a [`QueryRequest`].
fn query_request_from_cbor(bytes: &[u8]) -> PyResult<QueryRequest> {
    loom_core::search_request_from_cbor(bytes)
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))
}

/// Encode a query response as the CBOR array `[reduced, [[id, score_cell] ...]]`.
fn response_to_cbor(response: &QueryResponse) -> Vec<u8> {
    loom_core::search_response_cbor(response)
}

/// Encode document ids as a CBOR array of byte strings.
fn ids_to_cbor(ids: Vec<Vec<u8>>) -> Vec<u8> {
    let items = ids.into_iter().map(CborValue::Bytes).collect();
    cbor_encode(&CborValue::Array(items)).unwrap_or_default()
}

/// Create search collection `name` with the field `mapping` (CBOR `field -> [type_tag, stored, faceted]`,
/// type 0 text, 1 keyword) in workspace `workspace` (UUID or name, created with the `search` facet if
/// absent). `CONFLICT` if the collection already exists.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, mapping, passphrase=None))]
pub(crate) fn search_create(
    path: &str,
    workspace: &str,
    name: &str,
    mapping: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mapping = mapping_from_cbor(mapping)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_search_ns(&mut loom, workspace)?;
    loom_core::search_create(&mut loom, ns, name, mapping).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Insert or replace the document at `id` (opaque bytes) in collection `name`; `doc` is a CBOR
/// `field -> value` map (each value text or bytes). `NOT_FOUND` if the collection was never created.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, doc, passphrase=None))]
pub(crate) fn search_index(
    path: &str,
    workspace: &str,
    name: &str,
    id: &[u8],
    doc: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let doc = document_from_cbor(doc)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_core::search_index(&mut loom, ns, name, id.to_vec(), doc).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Fetch the document at `id` in collection `name` as a CBOR `field -> value` map, or `None` when `id` is
/// absent. `NOT_FOUND` if the collection does not exist.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, passphrase=None))]
pub(crate) fn search_get<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    id: &[u8],
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::search_get(&loom, ns, name, id)
        .map_err(py_err)?
        .map(|d| PyBytes::new(py, &document_to_cbor(&d))))
}
/// Remove `id` from collection `name`; returns whether it was present. `NOT_FOUND` if the collection does
/// not exist.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, id, passphrase=None))]
pub(crate) fn search_delete(
    path: &str,
    workspace: &str,
    name: &str,
    id: &[u8],
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let present = loom_core::search_delete(&mut loom, ns, name, id).map_err(py_err)?;
    if present {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(present)
}
/// Document ids in collection `name` as a CBOR array of byte strings. When `has_prefix` is true only ids
/// starting with `prefix` are returned; otherwise every id is returned. `NOT_FOUND` if the collection
/// does not exist.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, prefix, has_prefix, passphrase=None))]
pub(crate) fn search_ids<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    prefix: &[u8],
    has_prefix: bool,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let prefix = has_prefix.then_some(prefix);
    let bytes = ids_to_cbor(loom_core::search_ids(&loom, ns, name, prefix).map_err(py_err)?);
    Ok(PyBytes::new(py, &bytes))
}
/// Replace the field mapping of collection `name` (CBOR `field -> [type_tag, stored, faceted]`).
/// `NOT_FOUND` if the collection does not exist.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, mapping, passphrase=None))]
pub(crate) fn search_remap(
    path: &str,
    workspace: &str,
    name: &str,
    mapping: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mapping = mapping_from_cbor(mapping)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_core::search_remap(&mut loom, ns, name, mapping).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Run the portable linear-scan query over collection `name`. `request` is the CBOR array
/// `[query, limit, offset]` (`query` a recursive node: `[0, field, text]` match, `[1, field, value]` term,
/// `[2, field, [terms], slop]` phrase, `[3, field, lower, upper, incl_lower, incl_upper]` range,
/// `[4, [must], [should], [must_not]]` bool). The response is the CBOR array
/// `[reduced, [[id, score_cell] ...]]`. `NOT_FOUND` if the collection does not exist; `NO_SUCH_FIELD` for
/// an unmapped query field.
#[pyfunction]
#[pyo3(signature = (path, workspace, name, request, passphrase=None))]
pub(crate) fn search_query<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    name: &str,
    request: &[u8],
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let request = query_request_from_cbor(request)?;
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes =
        response_to_cbor(&loom_core::search_query(&loom, ns, name, &request).map_err(py_err)?);
    Ok(PyBytes::new(py, &bytes))
}
