//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Create (or replace the metadata of) calendar collection `collection` under `principal` in `workspace`
/// (UUID or name, created with the `calendar` facet if absent). `display_name` is the collection's
/// display name; `components` is a comma-separated component set ("event,todo"; "" is the empty set).
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, collection, display_name, components, passphrase=None))]
pub(crate) fn cal_create_collection(
    path: &str,
    workspace: &str,
    principal: &str,
    collection: &str,
    display_name: &str,
    components: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_cal_ns(&mut loom, workspace)?;
    let meta = CollectionMeta {
        display_name: display_name.to_string(),
        component_set: parse_component_set(components)?,
    };
    calendar::create_collection(&mut loom, ns, principal, collection, &meta).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Delete calendar collection `collection` under `principal` and every entry in it; returns whether it
/// existed.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, collection, passphrase=None))]
pub(crate) fn cal_delete_collection(
    path: &str,
    workspace: &str,
    principal: &str,
    collection: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let existed =
        calendar::delete_collection(&mut loom, ns, principal, collection).map_err(py_err)?;
    if existed {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(existed)
}
/// List the calendar collection ids under `principal` as the Loom Canonical CBOR array of text strings
/// (sorted; an absent principal is the empty array).
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, passphrase=None))]
pub(crate) fn cal_list_collections<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = strings_cbor(calendar::list_collections(&loom, ns, principal).map_err(py_err)?)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Put the calendar `entry` (its `CalendarEntry` canonical CBOR) into the existing collection
/// `collection` under `principal`, keyed by its UID. A later put at the same UID replaces it.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, collection, entry, passphrase=None))]
pub(crate) fn cal_put_entry(
    path: &str,
    workspace: &str,
    principal: &str,
    collection: &str,
    entry: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let entry = CalendarEntry::decode(entry).map_err(py_err)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    calendar::put_entry(&mut loom, ns, principal, collection, &entry).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Fetch the calendar entry at `uid` in collection `collection` as its `CalendarEntry` canonical CBOR,
/// or `None` if absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, collection, uid, passphrase=None))]
pub(crate) fn cal_get_entry<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    collection: &str,
    uid: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(calendar::get_entry(&loom, ns, principal, collection, uid)
        .map_err(py_err)?
        .map(|e| PyBytes::new(py, &e.encode())))
}
/// Remove the calendar entry at `uid` in collection `collection`; returns whether it was present.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, collection, uid, passphrase=None))]
pub(crate) fn cal_delete_entry(
    path: &str,
    workspace: &str,
    principal: &str,
    collection: &str,
    uid: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let present =
        calendar::delete_entry(&mut loom, ns, principal, collection, uid).map_err(py_err)?;
    if present {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(present)
}
/// List collection `collection` as the Loom Canonical CBOR array of per-entry `CalendarEntry` canonical
/// CBOR byte strings (UID order; an absent collection is the empty array).
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, collection, passphrase=None))]
pub(crate) fn cal_list_entries<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    collection: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let records = calendar::list_entries(&loom, ns, principal, collection)
        .map_err(py_err)?
        .iter()
        .map(CalendarEntry::encode)
        .collect();
    let bytes = records_cbor(records)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Expand collection `collection` into occurrences within the half-open wall-clock window `[from, to)`
/// (both `YYYYMMDDTHHMMSS`) as the Loom Canonical CBOR array of `[uid, "YYYYMMDDTHHMMSS"]` pairs.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, collection, from, to, passphrase=None))]
pub(crate) fn cal_range<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    collection: &str,
    from: &str,
    to: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let from = parse_window_bound(from, "from")?;
    let to = parse_window_bound(to, "to")?;
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let occ = calendar::range(&loom, ns, principal, collection, from, to).map_err(py_err)?;
    let items = occ
        .into_iter()
        .map(|o| {
            CborValue::Array(vec![
                CborValue::Text(o.uid),
                CborValue::Text(format_window_bound(&o.start)),
            ])
        })
        .collect();
    let bytes = cbor_encode(&CborValue::Array(items))
        .map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))?;
    Ok(PyBytes::new(py, &bytes))
}
/// Search collection `collection` by component filter and substring. `component` is "" (any), "event",
/// or "todo"; `text` is a case-insensitive substring over the summary ("" matches any). Returns the Loom
/// Canonical CBOR array of per-entry `CalendarEntry` canonical CBOR byte strings (UID order).
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, collection, component, text, passphrase=None))]
pub(crate) fn cal_search<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    collection: &str,
    component: &str,
    text: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let component = parse_component_filter(component)?;
    let text = if text.is_empty() { None } else { Some(text) };
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let records = calendar::search(&loom, ns, principal, collection, component, text)
        .map_err(py_err)?
        .iter()
        .map(CalendarEntry::encode)
        .collect();
    let bytes = records_cbor(records)?;
    Ok(PyBytes::new(py, &bytes))
}
/// The on-demand iCalendar (`.ics`) projection of the entry at `uid`, or `None` if absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, collection, uid, passphrase=None))]
pub(crate) fn cal_entry_ics(
    path: &str,
    workspace: &str,
    principal: &str,
    collection: &str,
    uid: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<String>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    calendar::entry_ics(&loom, ns, principal, collection, uid).map_err(py_err)
}
/// Parse iCalendar document `ics` and store it as a record in collection `collection` (the validated
/// write-in path); returns the new ETag as a `"algo:hex"` string.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, collection, ics, passphrase=None))]
pub(crate) fn cal_put_ics(
    path: &str,
    workspace: &str,
    principal: &str,
    collection: &str,
    ics: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let etag = calendar::put_ics(&mut loom, ns, principal, collection, ics).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(etag.to_string())
}
