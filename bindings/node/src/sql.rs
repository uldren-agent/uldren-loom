//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Read the staged `table` of workspace `workspace` (selected with `facet`) as canonical-CBOR
/// (`{ "columns", "rows" }`). `table` is the staged table path, e.g.
/// `.loom/facets/sql/<db>/tables/<name>`. Mirrors the C ABI `loom_sql_read_table`.
#[napi]
pub fn sql_read_table(
    loom_path: String,
    workspace: String,
    table: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, "sql", &workspace)?;
    let t = loom.read_table(ns, &table).map_err(reason)?;
    let bytes = result_cbor::table_cbor(&t).map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}

/// Read `table` from historical commit `commit`, leaving the current working tree unchanged.
#[napi]
pub fn sql_read_table_at(
    loom_path: String,
    workspace: String,
    table: String,
    commit: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, "sql", &workspace)?;
    let commit = Digest::parse(&commit).map_err(reason)?;
    let t = loom.read_table_at(ns, &table, commit).map_err(reason)?;
    let bytes = result_cbor::table_cbor(&t).map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}

/// Scan secondary index `index` on `table` for the lookup `prefix` (a canonical-CBOR cell array, the
/// same codec as a result row; an empty prefix is the canonical CBOR of an empty array), returning the
/// matching rows as canonical-CBOR (`{ "columns", "rows" }`). Mirrors the C ABI `loom_sql_index_scan`.
#[napi]
pub fn sql_index_scan(
    loom_path: String,
    workspace: String,
    table: String,
    index: String,
    prefix: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, "sql", &workspace)?;
    let values = lookup_cbor::values_from_cbor(&prefix).map_err(reason)?;
    let rows = loom
        .index_scan(ns, &table, &index, &values)
        .map_err(reason)?;
    let schema = loom
        .read_table(ns, &table)
        .map_err(reason)?
        .schema()
        .clone();
    let bytes = result_cbor::rows_cbor(&schema, &rows).map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}

/// Scan secondary index `index` on `table` from historical commit `commit`.
#[napi]
pub fn sql_index_scan_at(
    loom_path: String,
    workspace: String,
    table: String,
    index: String,
    prefix: Uint8Array,
    commit: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, "sql", &workspace)?;
    let values = lookup_cbor::values_from_cbor(&prefix).map_err(reason)?;
    let commit = Digest::parse(&commit).map_err(reason)?;
    let rows = loom
        .index_scan_at(ns, &table, &index, &values, commit)
        .map_err(reason)?;
    let schema = loom
        .read_table_at(ns, &table, commit)
        .map_err(reason)?
        .schema()
        .clone();
    let bytes = result_cbor::rows_cbor(&schema, &rows).map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}

/// Blame the rows of `table` on `branch` for workspace `workspace` (selected with `facet`): each current
/// row plus the commit that last set it, as canonical-CBOR
/// (`{ "rows": [ { "commit", "values" } ] }`). Mirrors the C ABI `loom_sql_blame`.
#[napi]
pub fn sql_blame(
    loom_path: String,
    workspace: String,
    branch: String,
    table: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, "sql", &workspace)?;
    let rows = loom.blame_table(ns, &branch, &table).map_err(reason)?;
    let bytes = result_cbor::blame_cbor(&rows).map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}
/// Row-level diff of `table` between commits `fromCommit` and `toCommit` (content addresses), as
/// canonical-CBOR (`{ "diffs": [...] }`). `workspace` is validated to exist under the sql facet.
/// Mirrors the C ABI `loom_sql_diff`.
#[napi]
pub fn sql_diff(
    loom_path: String,
    workspace: String,
    table: String,
    from_commit: String,
    to_commit: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, "sql", &workspace)?;
    let from = Digest::parse(&from_commit).map_err(reason)?;
    let to = Digest::parse(&to_commit).map_err(reason)?;
    let diffs = loom.diff_table(ns, &table, from, to).map_err(reason)?;
    let bytes = result_cbor::diff_cbor(&diffs).map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}

/// Schema-aware table diff between commits. Existing `sqlDiff` remains row-only.
#[napi]
pub fn sql_table_diff(
    loom_path: String,
    workspace: String,
    table: String,
    from_commit: String,
    to_commit: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, "sql", &workspace)?;
    let from = Digest::parse(&from_commit).map_err(reason)?;
    let to = Digest::parse(&to_commit).map_err(reason)?;
    let records = loom
        .diff_table_records(ns, &table, from, to)
        .map_err(reason)?;
    let bytes = result_cbor::table_diff_cbor(&records).map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}
