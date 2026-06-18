//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Read the staged `table` of sql-facet workspace `workspace` as canonical-CBOR
/// (`{ "columns", "rows" }`). `table` is the staged table path, e.g.
/// `.loom/facets/sql/<db>/tables/<name>`. Mirrors the C ABI `loom_sql_read_table`.
#[pyfunction]
#[pyo3(signature = (path, workspace, table, passphrase=None))]
pub(crate) fn sql_read_table<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    table: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, "sql", workspace)?;
    let t = loom.read_table(ns, table).map_err(py_err)?;
    let bytes = result_cbor::table_cbor(&t).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

/// Read `table` from historical commit `commit`, leaving the current working tree unchanged.
#[pyfunction]
#[pyo3(signature = (path, workspace, table, commit, passphrase=None))]
pub(crate) fn sql_read_table_at<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    table: &str,
    commit: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, "sql", workspace)?;
    let commit = Digest::parse(commit).map_err(py_err)?;
    let t = loom.read_table_at(ns, table, commit).map_err(py_err)?;
    let bytes = result_cbor::table_cbor(&t).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

/// Scan secondary index `index` on `table` for the lookup `prefix` (a canonical-CBOR cell array, the
/// same codec as a result row; an empty prefix is the canonical CBOR of an empty array), returning the
/// matching rows as canonical-CBOR (`{ "columns", "rows" }`). Mirrors the C ABI `loom_sql_index_scan`.
#[pyfunction]
#[pyo3(signature = (path, workspace, table, index, prefix, passphrase=None))]
pub(crate) fn sql_index_scan<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    table: &str,
    index: &str,
    prefix: &[u8],
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, "sql", workspace)?;
    let values = lookup_cbor::values_from_cbor(prefix).map_err(py_err)?;
    let rows = loom.index_scan(ns, table, index, &values).map_err(py_err)?;
    let schema = loom.read_table(ns, table).map_err(py_err)?.schema().clone();
    let bytes = result_cbor::rows_cbor(&schema, &rows).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

/// Scan secondary index `index` on `table` from historical commit `commit`.
#[pyfunction]
#[pyo3(signature = (path, workspace, table, index, prefix, commit, passphrase=None))]
pub(crate) fn sql_index_scan_at<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    table: &str,
    index: &str,
    prefix: &[u8],
    commit: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, "sql", workspace)?;
    let values = lookup_cbor::values_from_cbor(prefix).map_err(py_err)?;
    let commit = Digest::parse(commit).map_err(py_err)?;
    let rows = loom
        .index_scan_at(ns, table, index, &values, commit)
        .map_err(py_err)?;
    let schema = loom
        .read_table_at(ns, table, commit)
        .map_err(py_err)?
        .schema()
        .clone();
    let bytes = result_cbor::rows_cbor(&schema, &rows).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

/// Blame the rows of `table` on `branch` for sql-facet workspace `workspace`: each current
/// row plus the commit that last set it, as canonical-CBOR (`{ "rows": [ { "commit", "values" } ] }`).
/// Mirrors the C ABI `loom_sql_blame`.
#[pyfunction]
#[pyo3(signature = (path, workspace, branch, table, passphrase=None))]
pub(crate) fn sql_blame<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    branch: &str,
    table: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, "sql", workspace)?;
    let rows = loom.blame_table(ns, branch, table).map_err(py_err)?;
    let bytes = result_cbor::blame_cbor(&rows).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Row-level diff of `table` between commits `from_commit` and `to_commit` (content addresses), as
/// canonical-CBOR (`{ "diffs": [...] }`). `workspace` is validated to exist under the sql facet.
/// Mirrors the C ABI `loom_sql_diff`.
#[pyfunction]
#[pyo3(signature = (path, workspace, table, from_commit, to_commit, passphrase=None))]
pub(crate) fn sql_diff<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    table: &str,
    from_commit: &str,
    to_commit: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, "sql", workspace)?;
    let from = Digest::parse(from_commit).map_err(py_err)?;
    let to = Digest::parse(to_commit).map_err(py_err)?;
    let diffs = loom.diff_table(ns, table, from, to).map_err(py_err)?;
    let bytes = result_cbor::diff_cbor(&diffs).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

/// Schema-aware table diff between commits. Existing ``sql_diff`` remains row-only.
#[pyfunction]
#[pyo3(signature = (path, workspace, table, from_commit, to_commit, passphrase=None))]
pub(crate) fn sql_table_diff<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    table: &str,
    from_commit: &str,
    to_commit: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, "sql", workspace)?;
    let from = Digest::parse(from_commit).map_err(py_err)?;
    let to = Digest::parse(to_commit).map_err(py_err)?;
    let records = loom
        .diff_table_records(ns, table, from, to)
        .map_err(py_err)?;
    let bytes = result_cbor::table_diff_cbor(&records).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}
