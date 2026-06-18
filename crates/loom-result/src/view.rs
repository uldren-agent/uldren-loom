//! Engine-free `ResultViews` accessors over a decoded [`ResultPayload`].
//!
//! `ResultViews` is a local, indexed, typed view over a canonical result buffer the caller already holds:
//! no method tickets a round trip. The accessors here are pure functions over the shared decoder's
//! [`ResultPayload`] model, so both the in-process [`LocalLoomClient`](loom_client) and the engine-free
//! `RemoteLoomClient` present the exact same `ResultViews` surface without duplicating the traversal. A
//! client wraps these with a handle registry (open/close) and forwards each generated trait method here.
//!
//! Composite results (a `ResultKind`, `VariableKind`, `MergeOutcome`, `DiffChange`, `MapEntry`, or a
//! `LoomValue` cell) cross the generated Rust surface as canonical CBOR `Vec<u8>`: kinds and outcomes as
//! canonical CBOR text of a stable lowercase label, a cell as the one faithful cell codec's bytes, and a
//! map entry as the canonical CBOR array `[key_text, cell]`. These are a local (`Lo`) contract shared by
//! the two Rust clients; they never cross the wire.
//!
//! Licensed under BUSL-1.1.

use crate::result_view::{Merge, Reader, ResultPayload, RowChange, ShowVariable, Statement};
use loom_codec::Value as Cbor;
use loom_types::error::{LoomError, Result};
use loom_types::tabular::{Value, cell_value};

/// One addressable item in a result: a SQL statement, or the single reader result.
enum Item<'a> {
    Statement(&'a Statement),
    Reader(&'a Reader),
}

fn out_of_range(what: &str) -> LoomError {
    LoomError::corrupt(format!("result view: {what} out of range"))
}

fn not_available(what: &str) -> LoomError {
    LoomError::corrupt(format!("result view: {what} not available for this item"))
}

/// The number of addressable items: statements for a statement result, or `1` for a single reader
/// (`result_len`).
pub fn len(payload: &ResultPayload) -> u64 {
    match payload {
        ResultPayload::Statements(items) => items.len() as u64,
        ResultPayload::Reader(_) => 1,
    }
}

/// Whether the result is a statement list (`Some(true)`), a single reader (`Some(false)`).
/// `result_is_statements` always resolves for a decoded payload.
pub fn is_statements(payload: &ResultPayload) -> Option<bool> {
    Some(matches!(payload, ResultPayload::Statements(_)))
}

fn item_at(payload: &ResultPayload, item: u64) -> Result<Item<'_>> {
    match payload {
        ResultPayload::Statements(items) => items
            .get(item as usize)
            .map(Item::Statement)
            .ok_or_else(|| out_of_range("item")),
        ResultPayload::Reader(reader) if item == 0 => Ok(Item::Reader(reader)),
        ResultPayload::Reader(_) => Err(out_of_range("item")),
    }
}

fn statement_kind(statement: &Statement) -> &'static str {
    match statement {
        Statement::Select { .. } => "select",
        Statement::SelectMap(_) => "select_map",
        Statement::ShowColumns(_) => "show_columns",
        Statement::Insert(_) => "insert",
        Statement::Delete(_) => "delete",
        Statement::Update(_) => "update",
        Statement::DropTable(_) => "drop_table",
        Statement::Create => "create",
        Statement::DropFunction => "drop_function",
        Statement::AlterTable => "alter_table",
        Statement::CreateIndex => "create_index",
        Statement::DropIndex => "drop_index",
        Statement::StartTransaction => "start_transaction",
        Statement::Commit => "commit",
        Statement::Rollback => "rollback",
        Statement::ShowVariable(_) => "show_variable",
    }
}

fn reader_kind(reader: &Reader) -> &'static str {
    match reader {
        Reader::Rows { .. } => "rows",
        Reader::Blame(_) => "blame",
        Reader::Diff(_) => "diff",
        Reader::CommitLog(_) => "commit_log",
        Reader::Merge(_) => "merge",
    }
}

/// A stable lowercase `ResultKind` label for `item`, as canonical CBOR text (`result_item_kind`), or
/// `None` when `item` is out of range.
pub fn item_kind(payload: &ResultPayload, item: u64) -> Option<Vec<u8>> {
    let label = match item_at(payload, item).ok()? {
        Item::Statement(statement) => statement_kind(statement),
        Item::Reader(reader) => reader_kind(reader),
    };
    Some(text_cbor(label))
}

/// The tabular rows of a `Select`/`Rows`/`Blame` item, if any.
fn item_rows<'a>(item: &Item<'a>) -> Option<Vec<&'a [Value]>> {
    match item {
        Item::Statement(Statement::Select { rows, .. }) => {
            Some(rows.iter().map(Vec::as_slice).collect())
        }
        Item::Reader(Reader::Rows { rows, .. }) => Some(rows.iter().map(Vec::as_slice).collect()),
        Item::Reader(Reader::Blame(blame)) => {
            Some(blame.iter().map(|r| r.values.as_slice()).collect())
        }
        _ => None,
    }
}

/// The column labels of a tabular item, if the item carries them.
fn item_columns<'a>(item: &Item<'a>) -> Option<Vec<(&'a str, &'a str)>> {
    match item {
        Item::Statement(Statement::Select { labels, .. }) => {
            Some(labels.iter().map(|l| (l.as_str(), "")).collect())
        }
        Item::Statement(Statement::ShowColumns(columns)) => Some(
            columns
                .iter()
                .map(|c| (c.name.as_str(), c.type_name.as_str()))
                .collect(),
        ),
        Item::Reader(Reader::Rows { columns, .. }) => Some(
            columns
                .iter()
                .map(|c| (c.name.as_str(), c.type_name.as_str()))
                .collect(),
        ),
        _ => None,
    }
}

/// The number of columns in a tabular item (`result_column_count`). A `Blame` item, which carries no
/// column metadata, reports the cell count of its first row.
pub fn column_count(payload: &ResultPayload, item: u64) -> Result<u64> {
    let item = item_at(payload, item)?;
    if let Some(columns) = item_columns(&item) {
        return Ok(columns.len() as u64);
    }
    if let Some(rows) = item_rows(&item) {
        return Ok(rows.first().map(|r| r.len()).unwrap_or(0) as u64);
    }
    Ok(0)
}

/// The name of column `col` in a tabular item (`result_column_name`).
pub fn column_name(payload: &ResultPayload, item: u64, col: u64) -> Result<String> {
    let item = item_at(payload, item)?;
    let columns = item_columns(&item).ok_or_else(|| not_available("columns"))?;
    columns
        .get(col as usize)
        .map(|(name, _)| (*name).to_string())
        .ok_or_else(|| out_of_range("column"))
}

/// The declared type label of column `col` in a tabular item (`result_column_type`). A bare `SELECT`
/// projection carries names but no types, so its column types are the empty string.
pub fn column_type(payload: &ResultPayload, item: u64, col: u64) -> Result<String> {
    let item = item_at(payload, item)?;
    let columns = item_columns(&item).ok_or_else(|| not_available("columns"))?;
    columns
        .get(col as usize)
        .map(|(_, ty)| (*ty).to_string())
        .ok_or_else(|| out_of_range("column"))
}

/// The number of rows in a tabular item (`result_row_count`).
pub fn row_count(payload: &ResultPayload, item: u64) -> Result<u64> {
    let item = item_at(payload, item)?;
    Ok(item_rows(&item).map(|r| r.len()).unwrap_or(0) as u64)
}

/// The number of cells in row `row` of a tabular item (`result_row_len`).
pub fn row_len(payload: &ResultPayload, item: u64, row: u64) -> Result<u64> {
    let item = item_at(payload, item)?;
    let rows = item_rows(&item).ok_or_else(|| not_available("rows"))?;
    rows.get(row as usize)
        .map(|cells| cells.len() as u64)
        .ok_or_else(|| out_of_range("row"))
}

/// The faithful cell at `(row, col)` of a tabular item as canonical CBOR (`result_cell`).
pub fn cell(payload: &ResultPayload, item: u64, row: u64, col: u64) -> Result<Vec<u8>> {
    let item = item_at(payload, item)?;
    let rows = item_rows(&item).ok_or_else(|| not_available("rows"))?;
    let cells = rows.get(row as usize).ok_or_else(|| out_of_range("row"))?;
    let value = cells
        .get(col as usize)
        .ok_or_else(|| out_of_range("column"))?;
    cell_cbor(value)
}

/// The commit that last set row `row` of a `Blame` item (`result_row_commit`).
pub fn row_commit(payload: &ResultPayload, item: u64, row: u64) -> Result<String> {
    match item_at(payload, item)? {
        Item::Reader(Reader::Blame(blame)) => blame
            .get(row as usize)
            .map(|r| r.commit.clone())
            .ok_or_else(|| out_of_range("row")),
        _ => Err(not_available("row commit")),
    }
}

/// The affected-row count of an `Insert`/`Delete`/`Update`/`DropTable` item (`result_count`).
pub fn count(payload: &ResultPayload, item: u64) -> Result<u64> {
    match item_at(payload, item)? {
        Item::Statement(
            Statement::Insert(n)
            | Statement::Delete(n)
            | Statement::Update(n)
            | Statement::DropTable(n),
        ) => Ok(*n),
        _ => Err(not_available("count")),
    }
}

/// The strings of a `CommitLog` reader or a `ShowVariable` list item.
fn item_strings<'a>(item: &Item<'a>) -> Option<Vec<&'a str>> {
    match item {
        Item::Reader(Reader::CommitLog(commits)) => {
            Some(commits.iter().map(String::as_str).collect())
        }
        Item::Statement(Statement::ShowVariable(ShowVariable::Tables(values)))
        | Item::Statement(Statement::ShowVariable(ShowVariable::Functions(values))) => {
            Some(values.iter().map(String::as_str).collect())
        }
        Item::Statement(Statement::ShowVariable(ShowVariable::Version(value))) => {
            Some(vec![value.as_str()])
        }
        _ => None,
    }
}

/// The number of strings a `CommitLog`/`ShowVariable` item exposes (`result_string_count`).
pub fn string_count(payload: &ResultPayload, item: u64) -> Result<u64> {
    let item = item_at(payload, item)?;
    Ok(item_strings(&item).map(|s| s.len()).unwrap_or(0) as u64)
}

/// The `i`-th string of a `CommitLog`/`ShowVariable` item (`result_string`).
pub fn string(payload: &ResultPayload, item: u64, i: u64) -> Result<String> {
    let item = item_at(payload, item)?;
    let strings = item_strings(&item).ok_or_else(|| not_available("strings"))?;
    strings
        .get(i as usize)
        .map(|s| (*s).to_string())
        .ok_or_else(|| out_of_range("string"))
}

/// A stable lowercase `VariableKind` label for a `ShowVariable` item, as canonical CBOR text
/// (`result_variable_kind`).
pub fn variable_kind(payload: &ResultPayload, item: u64) -> Result<Vec<u8>> {
    match item_at(payload, item)? {
        Item::Statement(Statement::ShowVariable(variable)) => {
            let label = match variable {
                ShowVariable::Tables(_) => "tables",
                ShowVariable::Functions(_) => "functions",
                ShowVariable::Version(_) => "version",
            };
            Ok(text_cbor(label))
        }
        _ => Err(not_available("variable kind")),
    }
}

/// A stable lowercase `MergeOutcome` label for a `Merge` item, as canonical CBOR text
/// (`result_merge_outcome`).
pub fn merge_outcome(payload: &ResultPayload, item: u64) -> Result<Vec<u8>> {
    match item_at(payload, item)? {
        Item::Reader(Reader::Merge(merge)) => {
            let label = match merge {
                Merge::UpToDate => "up_to_date",
                Merge::FastForward(_) => "fast_forward",
                Merge::Merged(_) => "merged",
                Merge::Conflicts(_) => "conflicts",
            };
            Ok(text_cbor(label))
        }
        _ => Err(not_available("merge outcome")),
    }
}

fn item_diffs<'a>(item: &Item<'a>) -> Option<&'a [RowChange]> {
    match item {
        Item::Reader(Reader::Diff(changes)) => Some(changes),
        _ => None,
    }
}

/// The number of row-level changes in a `Diff` item (`result_diff_count`).
pub fn diff_count(payload: &ResultPayload, item: u64) -> Result<u64> {
    let item = item_at(payload, item)?;
    Ok(item_diffs(&item).map(<[RowChange]>::len).unwrap_or(0) as u64)
}

/// A stable lowercase `DiffChange` label for change `entry` of a `Diff` item, as canonical CBOR text
/// (`result_diff_change`).
pub fn diff_change(payload: &ResultPayload, item: u64, entry: u64) -> Result<Vec<u8>> {
    let item = item_at(payload, item)?;
    let diffs = item_diffs(&item).ok_or_else(|| not_available("diff"))?;
    let change = diffs
        .get(entry as usize)
        .ok_or_else(|| out_of_range("diff"))?;
    let label = match change {
        RowChange::Added(_) => "added",
        RowChange::Removed(_) => "removed",
        RowChange::Updated { .. } => "updated",
    };
    Ok(text_cbor(label))
}

/// Which side of a diff change a cell accessor addresses. `from` is the old row (a `removed` row or the
/// pre-image of an `updated` row); `to` is the new row (an `added` row or the post-image of an `updated`
/// row).
fn side_cells<'a>(change: &'a RowChange, side: &str) -> Result<&'a [Value]> {
    match (change, side) {
        (RowChange::Added(values), "to") => Ok(values),
        (RowChange::Removed(values), "from") => Ok(values),
        (RowChange::Updated { from, .. }, "from") => Ok(from),
        (RowChange::Updated { to, .. }, "to") => Ok(to),
        (_, "from" | "to") => Ok(&[]),
        _ => Err(LoomError::corrupt(format!(
            "result view: unknown diff side '{side}'"
        ))),
    }
}

fn decode_side(side: &[u8]) -> Result<String> {
    match loom_codec::decode(side)
        .map_err(|e| LoomError::corrupt(format!("result view: diff side cbor: {e}")))?
    {
        Cbor::Text(text) => Ok(text),
        _ => Err(LoomError::corrupt("result view: diff side is not text")),
    }
}

/// The number of cells on `side` of change `entry` in a `Diff` item (`result_diff_len`).
pub fn diff_len(payload: &ResultPayload, item: u64, entry: u64, side: &[u8]) -> Result<u64> {
    let side = decode_side(side)?;
    let item = item_at(payload, item)?;
    let diffs = item_diffs(&item).ok_or_else(|| not_available("diff"))?;
    let change = diffs
        .get(entry as usize)
        .ok_or_else(|| out_of_range("diff"))?;
    Ok(side_cells(change, &side)?.len() as u64)
}

/// The faithful cell at column `col` on `side` of change `entry` in a `Diff` item, as canonical CBOR
/// (`result_diff_cell`).
pub fn diff_cell(
    payload: &ResultPayload,
    item: u64,
    entry: u64,
    side: &[u8],
    col: u64,
) -> Result<Vec<u8>> {
    let side = decode_side(side)?;
    let item = item_at(payload, item)?;
    let diffs = item_diffs(&item).ok_or_else(|| not_available("diff"))?;
    let change = diffs
        .get(entry as usize)
        .ok_or_else(|| out_of_range("diff"))?;
    let cells = side_cells(change, &side)?;
    let value = cells
        .get(col as usize)
        .ok_or_else(|| out_of_range("column"))?;
    cell_cbor(value)
}

fn item_map_rows<'a>(item: &Item<'a>) -> Option<&'a [std::collections::BTreeMap<String, Value>]> {
    match item {
        Item::Statement(Statement::SelectMap(rows)) => Some(rows),
        _ => None,
    }
}

/// The number of entries in row `row` of a `SelectMap` item (`result_map_len`).
pub fn map_len(payload: &ResultPayload, item: u64, row: u64) -> Result<u64> {
    let item = item_at(payload, item)?;
    let rows = item_map_rows(&item).ok_or_else(|| not_available("map rows"))?;
    rows.get(row as usize)
        .map(|entries| entries.len() as u64)
        .ok_or_else(|| out_of_range("row"))
}

/// Entry `idx` of row `row` of a `SelectMap` item, as canonical CBOR `[key_text, cell]`
/// (`result_map_entry`). Entries are in key order.
pub fn map_entry(payload: &ResultPayload, item: u64, row: u64, idx: u64) -> Result<Vec<u8>> {
    let item = item_at(payload, item)?;
    let rows = item_map_rows(&item).ok_or_else(|| not_available("map rows"))?;
    let entries = rows.get(row as usize).ok_or_else(|| out_of_range("row"))?;
    let (key, value) = entries
        .iter()
        .nth(idx as usize)
        .ok_or_else(|| out_of_range("map entry"))?;
    let array = Cbor::Array(vec![Cbor::Text(key.clone()), cell_value(value)]);
    loom_codec::encode(&array)
        .map_err(|e| LoomError::corrupt(format!("result view: map entry: {e}")))
}

fn cell_cbor(value: &Value) -> Result<Vec<u8>> {
    loom_codec::encode(&cell_value(value))
        .map_err(|e| LoomError::corrupt(format!("result view: cell cbor: {e}")))
}

fn text_cbor(label: &str) -> Vec<u8> {
    loom_codec::encode(&Cbor::Text(label.to_string())).expect("text cbor is always encodable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result_view::{BlameRow, Column};
    use loom_types::tabular::cell_from;
    use std::collections::BTreeMap;

    fn text_of(bytes: &[u8]) -> String {
        match loom_codec::decode(bytes).unwrap() {
            Cbor::Text(s) => s,
            other => panic!("expected text, got {other:?}"),
        }
    }

    fn cell_int(bytes: &[u8]) -> i64 {
        match cell_from(loom_codec::decode(bytes).unwrap()).unwrap() {
            Value::Int(n) => n,
            other => panic!("expected int cell, got {other:?}"),
        }
    }

    #[test]
    fn select_item_exposes_columns_rows_and_cells() {
        let payload = ResultPayload::Statements(vec![Statement::Select {
            labels: vec!["id".into(), "name".into()],
            rows: vec![vec![Value::Int(1), Value::Text("a".into())]],
        }]);
        assert_eq!(len(&payload), 1);
        assert_eq!(is_statements(&payload), Some(true));
        assert_eq!(text_of(&item_kind(&payload, 0).unwrap()), "select");
        assert_eq!(column_count(&payload, 0).unwrap(), 2);
        assert_eq!(column_name(&payload, 0, 1).unwrap(), "name");
        assert_eq!(column_type(&payload, 0, 0).unwrap(), "");
        assert_eq!(row_count(&payload, 0).unwrap(), 1);
        assert_eq!(row_len(&payload, 0, 0).unwrap(), 2);
        assert_eq!(cell_int(&cell(&payload, 0, 0, 0).unwrap()), 1);
        assert!(item_kind(&payload, 5).is_none());
        assert!(cell(&payload, 0, 0, 9).is_err());
    }

    #[test]
    fn rows_reader_reports_typed_columns() {
        let payload = ResultPayload::Reader(Reader::Rows {
            columns: vec![Column {
                name: "id".into(),
                type_name: "I64".into(),
            }],
            rows: vec![vec![Value::Int(7)]],
        });
        assert_eq!(len(&payload), 1);
        assert_eq!(is_statements(&payload), Some(false));
        assert_eq!(text_of(&item_kind(&payload, 0).unwrap()), "rows");
        assert_eq!(column_type(&payload, 0, 0).unwrap(), "I64");
    }

    #[test]
    fn count_and_strings_and_variable() {
        let payload = ResultPayload::Statements(vec![
            Statement::Insert(3),
            Statement::ShowVariable(ShowVariable::Tables(vec!["t1".into(), "t2".into()])),
        ]);
        assert_eq!(count(&payload, 0).unwrap(), 3);
        assert_eq!(string_count(&payload, 1).unwrap(), 2);
        assert_eq!(string(&payload, 1, 1).unwrap(), "t2");
        assert_eq!(text_of(&variable_kind(&payload, 1).unwrap()), "tables");
        assert!(count(&payload, 1).is_err());
    }

    #[test]
    fn blame_exposes_row_commit() {
        let payload = ResultPayload::Reader(Reader::Blame(vec![BlameRow {
            commit: "blake3:ab".into(),
            values: vec![Value::Int(1)],
        }]));
        assert_eq!(column_count(&payload, 0).unwrap(), 1);
        assert_eq!(row_commit(&payload, 0, 0).unwrap(), "blake3:ab");
    }

    #[test]
    fn diff_sides_and_merge() {
        let payload = ResultPayload::Reader(Reader::Diff(vec![RowChange::Updated {
            from: vec![Value::Int(1)],
            to: vec![Value::Int(2), Value::Int(3)],
        }]));
        assert_eq!(diff_count(&payload, 0).unwrap(), 1);
        assert_eq!(text_of(&diff_change(&payload, 0, 0).unwrap()), "updated");
        let from = text_cbor("from");
        let to = text_cbor("to");
        assert_eq!(diff_len(&payload, 0, 0, &from).unwrap(), 1);
        assert_eq!(diff_len(&payload, 0, 0, &to).unwrap(), 2);
        assert_eq!(cell_int(&diff_cell(&payload, 0, 0, &to, 1).unwrap()), 3);

        let merge = ResultPayload::Reader(Reader::Merge(Merge::FastForward("blake3:cd".into())));
        assert_eq!(text_of(&merge_outcome(&merge, 0).unwrap()), "fast_forward");
    }

    #[test]
    fn select_map_entries_are_key_ordered() {
        let mut row0 = BTreeMap::new();
        row0.insert("b".to_string(), Value::Int(2));
        row0.insert("a".to_string(), Value::Int(1));
        let payload = ResultPayload::Statements(vec![Statement::SelectMap(vec![row0])]);
        assert_eq!(map_len(&payload, 0, 0).unwrap(), 2);
        let entry = map_entry(&payload, 0, 0, 0).unwrap();
        let Cbor::Array(parts) = loom_codec::decode(&entry).unwrap() else {
            panic!("map entry is not an array");
        };
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], Cbor::Text("a".to_string()));
    }
}
