//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Put UTF-8 text at string `id` in collection `collection` and return the new document tags.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, id, text, expected_entity_tag=None, passphrase=None))]
pub(crate) fn doc_put_text(
    path: &str,
    workspace: &str,
    collection: &str,
    id: &str,
    text: &str,
    expected_entity_tag: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<(String, String)> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_doc_ns(&mut loom, workspace)?;
    let result = loom_core::document_put_text_with_entity_tag(
        &mut loom,
        ns,
        collection,
        id,
        text,
        expected_entity_tag,
    )
    .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok((result.digest.to_string(), result.entity_tag))
}

/// Fetch `id` as UTF-8 text with its content digest, or `None` if absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, id, passphrase=None))]
pub(crate) fn doc_get_text(
    path: &str,
    workspace: &str,
    collection: &str,
    id: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<(String, String, String)>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::document_get_text(&loom, ns, collection, id)
        .map_err(py_err)?
        .map(|document| {
            (
                document.text,
                document.digest.to_string(),
                document.entity_tag,
            )
        }))
}

/// Put binary bytes at string `id` in collection `collection` and return the new document tags.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, id, value, expected_entity_tag=None, passphrase=None))]
pub(crate) fn doc_put_binary(
    path: &str,
    workspace: &str,
    collection: &str,
    id: &str,
    value: &[u8],
    expected_entity_tag: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<(String, String)> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_doc_ns(&mut loom, workspace)?;
    let result = loom_core::document_put_binary_with_entity_tag(
        &mut loom,
        ns,
        collection,
        id,
        value.to_vec(),
        expected_entity_tag,
    )
    .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok((result.digest.to_string(), result.entity_tag))
}

/// Fetch `id` as binary bytes with its content digest, or `None` if absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, id, passphrase=None))]
pub(crate) fn doc_get_binary<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    collection: &str,
    id: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<(Bound<'py, PyBytes>, String, String)>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom_core::document_get_binary(&loom, ns, collection, id)
        .map_err(py_err)?
        .map(|document| {
            (
                PyBytes::new(py, &document.bytes),
                document.digest.to_string(),
                document.entity_tag,
            )
        }))
}

/// Remove `id` from collection `collection`; returns whether it was present.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, id, passphrase=None))]
pub(crate) fn doc_delete(
    path: &str,
    workspace: &str,
    collection: &str,
    id: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let present = loom_core::doc_delete(&mut loom, ns, collection, id).map_err(py_err)?;
    if present {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(present)
}
/// List collection `collection` as its canonical binary representation.
#[pyfunction]
#[pyo3(signature = (path, workspace, collection, passphrase=None))]
pub(crate) fn doc_list_binary<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    collection: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = loom_core::document_list_binary(&loom, ns, collection).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, collection, name, field_path, unique=false, passphrase=None))]
pub(crate) fn doc_index_create(
    path: &str,
    workspace: &str,
    collection: &str,
    name: &str,
    field_path: &str,
    unique: bool,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let index = loom_core::DocumentIndexDef::new(
        name,
        loom_core::DocumentFieldPath::dotted(field_path).map_err(py_err)?,
        unique,
    )
    .map_err(py_err)?;
    loom_core::doc_create_index(&mut loom, ns, collection, index).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, collection, declaration_json, passphrase=None))]
pub(crate) fn doc_index_create_json(
    path: &str,
    workspace: &str,
    collection: &str,
    declaration_json: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let value = serde_json::from_slice::<serde_json::Value>(declaration_json)
        .map_err(|err| py_err(loom_core::LoomError::invalid(err.to_string())))?;
    let declaration = loom_core::document_index_declaration_from_json(&value).map_err(py_err)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_core::doc_create_index_declaration(&mut loom, ns, collection, declaration)
        .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, collection, name, passphrase=None))]
pub(crate) fn doc_index_drop(
    path: &str,
    workspace: &str,
    collection: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let dropped = loom_core::doc_drop_index(&mut loom, ns, collection, name).map_err(py_err)?;
    if dropped {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(dropped)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, collection, name, passphrase=None))]
pub(crate) fn doc_index_rebuild(
    path: &str,
    workspace: &str,
    collection: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom_core::doc_rebuild_index(&mut loom, ns, collection, name).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, collection, passphrase=None))]
pub(crate) fn doc_index_list_json(
    path: &str,
    workspace: &str,
    collection: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let value = loom_core::document_index_declarations_json(
        loom_core::doc_list_index_declarations(&loom, ns, collection).map_err(py_err)?,
    );
    serde_json::to_string(&value)
        .map_err(|err| py_err(loom_core::LoomError::invalid(err.to_string())))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, collection, passphrase=None))]
pub(crate) fn doc_index_status_json(
    path: &str,
    workspace: &str,
    collection: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let value = loom_core::document_index_statuses_json(
        loom_core::doc_index_statuses(&loom, ns, collection).map_err(py_err)?,
    );
    serde_json::to_string(&value)
        .map_err(|err| py_err(loom_core::LoomError::invalid(err.to_string())))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, collection, index, value_json, passphrase=None))]
pub(crate) fn doc_find_json(
    path: &str,
    workspace: &str,
    collection: &str,
    index: &str,
    value_json: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let value = serde_json::from_str::<serde_json::Value>(value_json)
        .map_err(|err| py_err(loom_core::LoomError::invalid(err.to_string())))?;
    let value = loom_core::document_index_value_from_json(&value).map_err(py_err)?;
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let ids = loom_core::doc_find(&loom, ns, collection, index, &value).map_err(py_err)?;
    serde_json::to_string(&loom_core::document_ids_json(ids))
        .map_err(|err| py_err(loom_core::LoomError::invalid(err.to_string())))
}

#[pyfunction]
#[pyo3(signature = (path, workspace, collection, query_json, passphrase=None))]
pub(crate) fn doc_query_json(
    path: &str,
    workspace: &str,
    collection: &str,
    query_json: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let query = serde_json::from_str::<serde_json::Value>(query_json)
        .map_err(|err| py_err(loom_core::LoomError::invalid(err.to_string())))?;
    let query = loom_core::document_query_from_json(&query).map_err(py_err)?;
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let result = loom_core::doc_query(&loom, ns, collection, &query).map_err(py_err)?;
    serde_json::to_string(&loom_core::document_query_result_json(result))
        .map_err(|err| py_err(loom_core::LoomError::invalid(err.to_string())))
}
