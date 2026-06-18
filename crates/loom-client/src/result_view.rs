//! Local `ResultViews` over a canonical-CBOR result buffer.
//!
//! `ResultViews` methods decode a result buffer the caller already holds into an indexed, typed view and
//! never ticket a round trip. For the in-process client this wraps `loom_result::result_view::decode`, which
//! is the single shared decoder mirroring the encoder, so the typed view and the wire bytes cannot drift.
//! [`LocalResultView`] exposes the indexed accessors; the full decoded [`ResultPayload`] is available via
//! [`LocalResultView::payload`] for reader kinds beyond the tabular accessors.
//!
//! Licensed under BUSL-1.1.

use loom_core::tabular::Value;
use loom_result::result_view::{Reader, ResultPayload, Statement, decode};
use loom_types::LoomError;

/// A decoded, indexed view over one result buffer.
pub struct LocalResultView {
    payload: ResultPayload,
}

impl LocalResultView {
    /// Decode a canonical result buffer into a view (`result_open`).
    ///
    /// # Errors
    /// Returns [`LoomError`] (`CORRUPT_OBJECT`) when the buffer is not a canonical result payload.
    pub fn open(buffer: &[u8]) -> Result<Self, LoomError> {
        Ok(Self {
            payload: decode(buffer)?,
        })
    }

    /// The decoded payload, for reader kinds not covered by the indexed accessors.
    pub fn payload(&self) -> &ResultPayload {
        &self.payload
    }

    /// Whether this is a statement-list result (`result_is_statements`).
    pub fn is_statements(&self) -> bool {
        matches!(self.payload, ResultPayload::Statements(_))
    }

    /// The number of items: statements for a statement result, or `1` for a single reader
    /// (`result_len`).
    pub fn len(&self) -> usize {
        match &self.payload {
            ResultPayload::Statements(items) => items.len(),
            ResultPayload::Reader(_) => 1,
        }
    }

    /// Whether the result carries no items.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A stable kind label for item `index` (`result_item_kind`), or `None` when out of range.
    pub fn item_kind(&self, index: usize) -> Option<&'static str> {
        match &self.payload {
            ResultPayload::Statements(items) => items.get(index).map(statement_kind),
            ResultPayload::Reader(reader) if index == 0 => Some(reader_kind(reader)),
            ResultPayload::Reader(_) => None,
        }
    }

    /// The rows of the first tabular result (a `SELECT` statement or a reader `Rows`), if any.
    fn rows(&self) -> Option<&Vec<Vec<Value>>> {
        match &self.payload {
            ResultPayload::Statements(items) => items.iter().find_map(|s| match s {
                Statement::Select { rows, .. } => Some(rows),
                _ => None,
            }),
            ResultPayload::Reader(Reader::Rows { rows, .. }) => Some(rows),
            ResultPayload::Reader(_) => None,
        }
    }

    /// The column labels of the first tabular result, if any.
    fn columns(&self) -> Option<Vec<String>> {
        match &self.payload {
            ResultPayload::Statements(items) => items.iter().find_map(|s| match s {
                Statement::Select { labels, .. } => Some(labels.clone()),
                _ => None,
            }),
            ResultPayload::Reader(Reader::Rows { columns, .. }) => {
                Some(columns.iter().map(|c| c.name.clone()).collect())
            }
            ResultPayload::Reader(_) => None,
        }
    }

    /// The number of columns in the first tabular result (`result_column_count`).
    pub fn column_count(&self) -> usize {
        self.columns().map(|c| c.len()).unwrap_or(0)
    }

    /// The name of column `index` in the first tabular result (`result_column_name`).
    pub fn column_name(&self, index: usize) -> Option<String> {
        self.columns().and_then(|c| c.get(index).cloned())
    }

    /// The number of rows in the first tabular result (`result_row_count`).
    pub fn row_count(&self) -> usize {
        self.rows().map(|r| r.len()).unwrap_or(0)
    }

    /// The number of cells in row `row` of the first tabular result (`result_row_len`).
    pub fn row_len(&self, row: usize) -> usize {
        self.rows()
            .and_then(|r| r.get(row))
            .map(|cells| cells.len())
            .unwrap_or(0)
    }

    /// The typed cell at `(row, column)` of the first tabular result (`result_cell`).
    pub fn cell(&self, row: usize, column: usize) -> Option<Value> {
        self.rows()
            .and_then(|r| r.get(row))
            .and_then(|cells| cells.get(column))
            .cloned()
    }

    /// The number of row-level diffs in a reader `Diff` result (`result_diff_count`).
    pub fn diff_count(&self) -> usize {
        match &self.payload {
            ResultPayload::Reader(Reader::Diff(changes)) => changes.len(),
            _ => 0,
        }
    }

    /// The number of commits in a reader `CommitLog` result (`result_count` for a log).
    pub fn commit_log_len(&self) -> usize {
        match &self.payload {
            ResultPayload::Reader(Reader::CommitLog(commits)) => commits.len(),
            _ => 0,
        }
    }

    /// A stable label for a reader `Merge` outcome (`result_merge_outcome`), or `None` when the result is
    /// not a merge.
    pub fn merge_outcome(&self) -> Option<&'static str> {
        match &self.payload {
            ResultPayload::Reader(Reader::Merge(merge)) => Some(merge_kind(merge)),
            _ => None,
        }
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

fn merge_kind(merge: &loom_result::result_view::Merge) -> &'static str {
    use loom_result::result_view::Merge;
    match merge {
        Merge::UpToDate => "up_to_date",
        Merge::FastForward(_) => "fast_forward",
        Merge::Merged(_) => "merged",
        Merge::Conflicts(_) => "conflicts",
    }
}
