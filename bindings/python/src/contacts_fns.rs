//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Create (or replace the metadata of) address book `book` under `principal` in `workspace` (UUID or
/// name, created with the `contacts` facet if absent). `display_name` is the book's display name.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, book, display_name, passphrase=None))]
pub(crate) fn card_create_book(
    path: &str,
    workspace: &str,
    principal: &str,
    book: &str,
    display_name: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_card_ns(&mut loom, workspace)?;
    let meta = BookMeta {
        display_name: display_name.to_string(),
    };
    contacts::create_book(&mut loom, ns, principal, book, &meta).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Delete address book `book` under `principal` and every contact in it; returns whether it existed.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, book, passphrase=None))]
pub(crate) fn card_delete_book(
    path: &str,
    workspace: &str,
    principal: &str,
    book: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let existed = contacts::delete_book(&mut loom, ns, principal, book).map_err(py_err)?;
    if existed {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(existed)
}
/// List the address-book ids under `principal` as the Loom Canonical CBOR array of text strings (sorted;
/// an absent principal is the empty array).
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, passphrase=None))]
pub(crate) fn card_list_books<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = strings_cbor(contacts::list_books(&loom, ns, principal).map_err(py_err)?)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Put the contact `entry` (its `ContactEntry` canonical CBOR) into the existing address book `book`
/// under `principal`, keyed by its UID. A later put at the same UID replaces it.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, book, entry, passphrase=None))]
pub(crate) fn card_put_entry(
    path: &str,
    workspace: &str,
    principal: &str,
    book: &str,
    entry: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let entry = ContactEntry::decode(entry).map_err(py_err)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    contacts::put_entry(&mut loom, ns, principal, book, &entry).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Fetch the contact at `uid` in address book `book` as its `ContactEntry` canonical CBOR, or `None` if
/// absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, book, uid, passphrase=None))]
pub(crate) fn card_get_entry<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    book: &str,
    uid: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(contacts::get_entry(&loom, ns, principal, book, uid)
        .map_err(py_err)?
        .map(|e| PyBytes::new(py, &e.encode())))
}
/// Remove the contact at `uid` in address book `book`; returns whether it was present.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, book, uid, passphrase=None))]
pub(crate) fn card_delete_entry(
    path: &str,
    workspace: &str,
    principal: &str,
    book: &str,
    uid: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let present = contacts::delete_entry(&mut loom, ns, principal, book, uid).map_err(py_err)?;
    if present {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(present)
}
/// List address book `book` as the Loom Canonical CBOR array of per-contact `ContactEntry` canonical
/// CBOR byte strings (UID order; an absent book is the empty array).
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, book, passphrase=None))]
pub(crate) fn card_list_entries<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    book: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let records = contacts::list_entries(&loom, ns, principal, book)
        .map_err(py_err)?
        .iter()
        .map(ContactEntry::encode)
        .collect();
    let bytes = records_cbor(records)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Search address book `book` by a case-insensitive substring `text` over the formatted name,
/// organization, and email values. Returns the Loom Canonical CBOR array of per-contact `ContactEntry`
/// canonical CBOR byte strings (UID order).
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, book, text, passphrase=None))]
pub(crate) fn card_search<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    book: &str,
    text: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let records = contacts::search(&loom, ns, principal, book, text)
        .map_err(py_err)?
        .iter()
        .map(ContactEntry::encode)
        .collect();
    let bytes = records_cbor(records)?;
    Ok(PyBytes::new(py, &bytes))
}
/// The on-demand vCard (`.vcf`) projection of the contact at `uid`, or `None` if absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, book, uid, passphrase=None))]
pub(crate) fn card_entry_vcard(
    path: &str,
    workspace: &str,
    principal: &str,
    book: &str,
    uid: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<String>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    contacts::entry_vcard(&loom, ns, principal, book, uid).map_err(py_err)
}
/// Parse vCard document `vcf` and store it as a record in address book `book` (the validated write-in
/// path); returns the new ETag as a `"algo:hex"` string.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, book, vcf, passphrase=None))]
pub(crate) fn card_put_vcard(
    path: &str,
    workspace: &str,
    principal: &str,
    book: &str,
    vcf: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let etag = contacts::put_vcard(&mut loom, ns, principal, book, vcf).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(etag.to_string())
}
