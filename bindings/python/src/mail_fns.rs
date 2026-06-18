//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Create (or replace the metadata of) mailbox `mailbox` under `principal` in `workspace` (UUID or name,
/// created with the `mail` facet if absent). `display_name` is the mailbox's display name.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, mailbox, display_name, passphrase=None))]
pub(crate) fn mail_create_mailbox(
    path: &str,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    display_name: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_mail_ns(&mut loom, workspace)?;
    let meta = MailboxMeta {
        display_name: display_name.to_string(),
    };
    mail::create_mailbox(&mut loom, ns, principal, mailbox, &meta).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Delete mailbox `mailbox` under `principal` and every message in it; returns whether it existed.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, mailbox, passphrase=None))]
pub(crate) fn mail_delete_mailbox(
    path: &str,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let existed = mail::delete_mailbox(&mut loom, ns, principal, mailbox).map_err(py_err)?;
    if existed {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(existed)
}
/// List the mailbox ids under `principal` as the Loom Canonical CBOR array of text strings (sorted; an
/// absent principal is the empty array).
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, passphrase=None))]
pub(crate) fn mail_list_mailboxes<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = strings_cbor(mail::list_mailboxes(&loom, ns, principal).map_err(py_err)?)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Ingest the raw RFC 5322 message `raw` into mailbox `mailbox` under `principal`, keyed by `uid`;
/// returns the body's content address as a `"algo:hex"` string.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, mailbox, uid, raw, passphrase=None))]
pub(crate) fn mail_ingest_message(
    path: &str,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
    raw: &[u8],
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let body = mail::ingest_message(&mut loom, ns, principal, mailbox, uid, raw).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(body.to_string())
}
/// Fetch the message index record at `uid` in mailbox `mailbox` as its `MailMessage` canonical CBOR, or
/// `None` if absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, mailbox, uid, passphrase=None))]
pub(crate) fn mail_get_message<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(mail::get_message(&loom, ns, principal, mailbox, uid)
        .map_err(py_err)?
        .map(|m| PyBytes::new(py, &m.encode())))
}
/// Fetch the immutable raw RFC 5322 body of the message at `uid` in mailbox `mailbox`, or `None` if
/// absent.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, mailbox, uid, passphrase=None))]
pub(crate) fn mail_to_eml<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(mail::to_eml(&loom, ns, principal, mailbox, uid)
        .map_err(py_err)?
        .map(|b| PyBytes::new(py, &b)))
}
/// Remove the message at `uid` in mailbox `mailbox` (index record, flags, and body reference); returns
/// whether it was present.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, mailbox, uid, passphrase=None))]
pub(crate) fn mail_delete_message(
    path: &str,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let present = mail::delete_message(&mut loom, ns, principal, mailbox, uid).map_err(py_err)?;
    if present {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(present)
}
/// List mailbox `mailbox` as the Loom Canonical CBOR array of per-message `MailMessage` canonical CBOR
/// byte strings (UID order; an absent mailbox is the empty array).
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, mailbox, passphrase=None))]
pub(crate) fn mail_list_messages<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let records = mail::list_messages(&loom, ns, principal, mailbox)
        .map_err(py_err)?
        .iter()
        .map(MailMessage::encode)
        .collect();
    let bytes = records_cbor(records)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Fetch the flag set of the message at `uid` in mailbox `mailbox` as the Loom Canonical CBOR array of
/// text strings (sorted; an absent message is the empty array).
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, mailbox, uid, passphrase=None))]
pub(crate) fn mail_get_flags<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let bytes = strings_cbor(mail::get_flags(&loom, ns, principal, mailbox, uid).map_err(py_err)?)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Replace the flag set of the message at `uid` in mailbox `mailbox`. `flags` is a canonical-CBOR
/// `Array(Text)` byte buffer.
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, mailbox, uid, flags, passphrase=None))]
pub(crate) fn mail_set_flags(
    path: &str,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
    flags: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let flags = flags_from_cbor(flags)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    mail::set_flags(&mut loom, ns, principal, mailbox, uid, &flags).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Search mailbox `mailbox` by a case-insensitive substring `text` over the subject and address values.
/// Returns the Loom Canonical CBOR array of per-message `MailMessage` canonical CBOR byte strings (UID
/// order).
#[pyfunction]
#[pyo3(signature = (path, workspace, principal, mailbox, text, passphrase=None))]
pub(crate) fn mail_search<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    principal: &str,
    mailbox: &str,
    text: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let records = mail::search(&loom, ns, principal, mailbox, text)
        .map_err(py_err)?
        .iter()
        .map(MailMessage::encode)
        .collect();
    let bytes = records_cbor(records)?;
    Ok(PyBytes::new(py, &bytes))
}
