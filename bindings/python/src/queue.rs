//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Append `entry` to `stream` in a workspace (by UUID or name, created with the `queue` facet if
/// absent); returns the assigned zero-based sequence.
#[pyfunction]
#[pyo3(signature = (path, workspace, stream, entry, passphrase=None))]
pub(crate) fn queue_append(
    path: &str,
    workspace: &str,
    stream: &str,
    entry: &[u8],
    passphrase: Option<&str>,
) -> PyResult<usize> {
    validate_stream_name(stream)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = ensure_queue_ns(&mut loom, workspace)?;
    let seq = loom.stream_append(ns, stream, entry).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(seq)
}
/// Fetch the entry at `seq` in `stream`, or `None` if out of range.
#[pyfunction]
#[pyo3(signature = (path, workspace, stream, seq, passphrase=None))]
pub(crate) fn queue_get<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    stream: &str,
    seq: usize,
    passphrase: Option<&str>,
) -> PyResult<Option<Bound<'py, PyBytes>>> {
    validate_stream_name(stream)?;
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom
        .stream_get(ns, stream, seq)
        .map_err(py_err)?
        .map(|bytes| PyBytes::new(py, &bytes)))
}
/// Read the half-open range `[lo, hi)` of `stream`, oldest first.
#[pyfunction]
#[pyo3(signature = (path, workspace, stream, lo, hi, passphrase=None))]
pub(crate) fn queue_range<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    stream: &str,
    lo: usize,
    hi: usize,
    passphrase: Option<&str>,
) -> PyResult<Vec<Bound<'py, PyBytes>>> {
    validate_stream_name(stream)?;
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom
        .stream_range(ns, stream, lo, hi)
        .map_err(py_err)?
        .into_iter()
        .map(|bytes| PyBytes::new(py, &bytes))
        .collect())
}
/// The number of entries in `stream`.
#[pyfunction]
#[pyo3(signature = (path, workspace, stream, passphrase=None))]
pub(crate) fn queue_len(
    path: &str,
    workspace: &str,
    stream: &str,
    passphrase: Option<&str>,
) -> PyResult<usize> {
    validate_stream_name(stream)?;
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom.stream_len(ns, stream).map_err(py_err)
}
/// The named consumer's next sequence for `stream`; `0` when none is stored.
#[pyfunction]
#[pyo3(signature = (path, workspace, stream, consumer_id, passphrase=None))]
pub(crate) fn queue_consumer_position(
    path: &str,
    workspace: &str,
    stream: &str,
    consumer_id: &str,
    passphrase: Option<&str>,
) -> PyResult<u64> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom.consumer_position(ns, stream, consumer_id)
        .map_err(py_err)
}
/// Read up to `max` entries from the consumer's stored next sequence in `stream`; does not advance.
#[pyfunction]
#[pyo3(signature = (path, workspace, stream, consumer_id, max, passphrase=None))]
pub(crate) fn queue_consumer_read<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    stream: &str,
    consumer_id: &str,
    max: usize,
    passphrase: Option<&str>,
) -> PyResult<Vec<Bound<'py, PyBytes>>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    Ok(loom
        .consumer_read(ns, stream, consumer_id, max)
        .map_err(py_err)?
        .into_iter()
        .map(|bytes| PyBytes::new(py, &bytes))
        .collect())
}
/// Advance the named consumer's next sequence for `stream` to `next_seq`; rejects backward movement.
#[pyfunction]
#[pyo3(signature = (path, workspace, stream, consumer_id, next_seq, passphrase=None))]
pub(crate) fn queue_consumer_advance(
    path: &str,
    workspace: &str,
    stream: &str,
    consumer_id: &str,
    next_seq: u64,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom.consumer_advance(ns, stream, consumer_id, next_seq)
        .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)
}
/// Set the named consumer's next sequence for `stream` to `next_seq`, which may move backward.
#[pyfunction]
#[pyo3(signature = (path, workspace, stream, consumer_id, next_seq, passphrase=None))]
pub(crate) fn queue_consumer_reset(
    path: &str,
    workspace: &str,
    stream: &str,
    consumer_id: &str,
    next_seq: u64,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    loom.consumer_reset(ns, stream, consumer_id, next_seq)
        .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)
}
