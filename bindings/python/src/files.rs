//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Create-or-replace file `file_path` with `content` and `mode` (default `0o100644`). Parent must exist.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, file_path, content, mode=None, passphrase=None))]
pub(crate) fn write_file(
    path: &str,
    facet: &str,
    workspace: &str,
    file_path: &str,
    content: &[u8],
    mode: Option<u32>,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.write_file(ns, file_path, content, mode.unwrap_or(0o100644))
        .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Read file `file_path` from the workspace working tree.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, file_path, passphrase=None))]
pub(crate) fn read_file<'py>(
    py: Python<'py>,
    path: &str,
    facet: &str,
    workspace: &str,
    file_path: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    let bytes = loom.read_file(ns, file_path).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Append `content` to file `file_path`, creating it if absent (the parent directory must exist).
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, file_path, content, passphrase=None))]
pub(crate) fn append_file(
    path: &str,
    facet: &str,
    workspace: &str,
    file_path: &str,
    content: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.append_file(ns, file_path, content).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Remove file `file_path` from the workspace working tree.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, file_path, passphrase=None))]
pub(crate) fn remove_file(
    path: &str,
    facet: &str,
    workspace: &str,
    file_path: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.remove_file(ns, file_path).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Create a symbolic link at `link_path` whose target is `target` (opaque; may be dangling). The parent
/// must exist; `link_path` must be free.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, target, link_path, passphrase=None))]
pub(crate) fn symlink(
    path: &str,
    facet: &str,
    workspace: &str,
    target: &str,
    link_path: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.symlink(ns, target, link_path).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Read the target of the symbolic link at `file_path` (errors if absent or not a symlink).
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, file_path, passphrase=None))]
pub(crate) fn read_link(
    path: &str,
    facet: &str,
    workspace: &str,
    file_path: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.read_link(ns, file_path).map_err(py_err)
}
/// Read up to `len` bytes from byte `offset` of file `file_path` (reads past the end clamp).
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, file_path, offset, len, passphrase=None))]
pub(crate) fn read_at<'py>(
    py: Python<'py>,
    path: &str,
    facet: &str,
    workspace: &str,
    file_path: &str,
    offset: u64,
    len: u64,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    let bytes = loom.read_at(ns, file_path, offset, len).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Write `content` at byte `offset` of file `file_path`, creating it if absent and zero-filling any gap.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, file_path, offset, content, passphrase=None))]
pub(crate) fn write_at(
    path: &str,
    facet: &str,
    workspace: &str,
    file_path: &str,
    offset: u64,
    content: &[u8],
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.write_at(ns, file_path, offset, content)
        .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Resize file `file_path` to `size`, zero-extending or dropping bytes; a missing file is created
/// zero-filled.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, file_path, size, passphrase=None))]
pub(crate) fn truncate_file(
    path: &str,
    facet: &str,
    workspace: &str,
    file_path: &str,
    size: u64,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.truncate_file(ns, file_path, size).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Open a file handle on `file_path` with `mode` (`read`|`write`|`read_write`|`append`), returning the
/// handle id (valid until `file_close`).
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, file_path, mode, passphrase=None))]
pub(crate) fn file_open(
    path: &str,
    facet: &str,
    workspace: &str,
    file_path: &str,
    mode: &str,
    passphrase: Option<&str>,
) -> PyResult<u64> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    let handle = loom
        .file_open(ns, file_path, parse_open_mode(mode)?)
        .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(handle)
}
/// Sequentially read up to `len` bytes from handle `file` at its cursor, advancing it.
#[pyfunction]
#[pyo3(signature = (path, file, len, passphrase=None))]
pub(crate) fn file_read<'py>(
    py: Python<'py>,
    path: &str,
    file: u64,
    len: u64,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let bytes = loom.file_read(file, len).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Positionally read up to `len` bytes at `offset` from handle `file` without moving its cursor.
#[pyfunction]
#[pyo3(signature = (path, file, offset, len, passphrase=None))]
pub(crate) fn file_read_at<'py>(
    py: Python<'py>,
    path: &str,
    file: u64,
    offset: u64,
    len: u64,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let bytes = loom.file_read_at(file, offset, len).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Sequentially write `content` to handle `file` at its cursor (or end of file for an append handle),
/// advancing it; returns the byte count.
#[pyfunction]
#[pyo3(signature = (path, file, content, passphrase=None))]
pub(crate) fn file_write(
    path: &str,
    file: u64,
    content: &[u8],
    passphrase: Option<&str>,
) -> PyResult<u64> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let n = loom.file_write(file, content).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(n)
}
/// Positionally write `content` at `offset` of handle `file` without moving its cursor; returns the byte
/// count.
#[pyfunction]
#[pyo3(signature = (path, file, offset, content, passphrase=None))]
pub(crate) fn file_write_at(
    path: &str,
    file: u64,
    offset: u64,
    content: &[u8],
    passphrase: Option<&str>,
) -> PyResult<u64> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let n = loom.file_write_at(file, offset, content).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(n)
}
/// Resize handle `file` to `size` bytes.
#[pyfunction]
#[pyo3(signature = (path, file, size, passphrase=None))]
pub(crate) fn file_truncate(
    path: &str,
    file: u64,
    size: u64,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    loom.file_truncate(file, size).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Flush handle `file` (validates the handle; writes already apply per operation).
#[pyfunction]
#[pyo3(signature = (path, file, passphrase=None))]
pub(crate) fn file_flush(path: &str, file: u64, passphrase: Option<&str>) -> PyResult<()> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    loom.file_flush(file).map_err(py_err)?;
    Ok(())
}
/// The live `(size, mode)` of handle `file`.
#[pyfunction]
#[pyo3(signature = (path, file, passphrase=None))]
pub(crate) fn file_stat(path: &str, file: u64, passphrase: Option<&str>) -> PyResult<(u64, u32)> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let st = loom.file_stat(file).map_err(py_err)?;
    Ok((st.size, st.mode))
}
/// Close handle `file`, releasing it (delete-on-last-close for an unlinked inode).
#[pyfunction]
#[pyo3(signature = (path, file, passphrase=None))]
pub(crate) fn file_close(path: &str, file: u64, passphrase: Option<&str>) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    loom.file_close(file).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
