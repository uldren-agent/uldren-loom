//! Typed decoding of canonical result payloads - the exact inverse of the engine-side encoder in
//! `loom_sql::result_cbor`.
//!
//! A binding that wants idiomatic typed results (not raw bytes, not the JSON debug form) decodes the
//! canonical result buffer through here into a faithful Rust view ([`ResultPayload`]), then maps that
//! view onto its language's native types (BigInt, byte array, decimal, records, ...). There is one
//! decoder mirroring the one encoder, so the typed view and the wire bytes can never drift, and every
//! scalar rides back through the shared faithful cell codec ([`loom_types::tabular::cell_from`]) so a
//! 128-bit integer, a non-finite float, an exact `f32`, a decimal, or a byte string is exact. This crate
//! is engine-free so every consumer (the hosted wire protocols, the FFI and language bindings, and both
//! Loom clients) shares one decoder.
//!
//! Licensed under BUSL-1.1.

use loom_codec::Value as Cbor;
use loom_types::error::{LoomError, Result};
use loom_types::tabular::{Value, cell_from};
use std::collections::BTreeMap;

/// A decoded result payload: either a list of SQL statement results (from `exec`) or a single reader
/// result (table / index scan / blame / diff / log / merge).
#[derive(Debug, Clone, PartialEq)]
pub enum ResultPayload {
    /// The result of one or more `;`-separated SQL statements, in order.
    Statements(Vec<Statement>),
    /// A single structured reader result.
    Reader(Reader),
}

/// A result column: its name and its `ColumnType` (or GlueSQL `DataType`) debug label.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub name: String,
    pub type_name: String,
}

/// One SQL statement's result.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Select {
        labels: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
    SelectMap(Vec<BTreeMap<String, Value>>),
    ShowColumns(Vec<Column>),
    Insert(u64),
    Delete(u64),
    Update(u64),
    DropTable(u64),
    Create,
    DropFunction,
    AlterTable,
    CreateIndex,
    DropIndex,
    StartTransaction,
    Commit,
    Rollback,
    ShowVariable(ShowVariable),
}

/// A `SHOW <variable>` result.
#[derive(Debug, Clone, PartialEq)]
pub enum ShowVariable {
    Tables(Vec<String>),
    Functions(Vec<String>),
    Version(String),
}

/// A structured (non-SQL) reader result.
#[derive(Debug, Clone, PartialEq)]
pub enum Reader {
    Rows {
        columns: Vec<Column>,
        rows: Vec<Vec<Value>>,
    },
    Blame(Vec<BlameRow>),
    Diff(Vec<RowChange>),
    CommitLog(Vec<String>),
    Merge(Merge),
}

/// One blame row: the row's values and the commit that last set them.
#[derive(Debug, Clone, PartialEq)]
pub struct BlameRow {
    pub commit: String,
    pub values: Vec<Value>,
}

/// One row-level diff entry.
#[derive(Debug, Clone, PartialEq)]
pub enum RowChange {
    Added(Vec<Value>),
    Removed(Vec<Value>),
    Updated { from: Vec<Value>, to: Vec<Value> },
}

/// A merge outcome.
#[derive(Debug, Clone, PartialEq)]
pub enum Merge {
    UpToDate,
    FastForward(String),
    Merged(String),
    Conflicts(Vec<String>),
}

/// Decode a canonical result buffer into a typed [`ResultPayload`].
///
/// # Errors
/// Returns [`LoomError`] (`CORRUPT_OBJECT`) when the buffer is not a canonical result payload.
pub fn decode(bytes: &[u8]) -> Result<ResultPayload> {
    let v =
        loom_codec::decode(bytes).map_err(|e| LoomError::corrupt(format!("result cbor: {e}")))?;
    match v {
        Cbor::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(statement(it)?);
            }
            Ok(ResultPayload::Statements(out))
        }
        Cbor::Map(entries) => Ok(ResultPayload::Reader(reader(&entries)?)),
        _ => Err(corrupt(
            "result is neither a statement array nor a reader map",
        )),
    }
}

fn corrupt(msg: &str) -> LoomError {
    LoomError::corrupt(format!("result decode: {msg}"))
}

fn get<'a>(entries: &'a [(Cbor, Cbor)], key: &str) -> Result<&'a Cbor> {
    entries
        .iter()
        .find_map(|(k, v)| match k {
            Cbor::Text(s) if s == key => Some(v),
            _ => None,
        })
        .ok_or_else(|| corrupt(&format!("missing field '{key}'")))
}

fn as_text(v: &Cbor) -> Result<String> {
    match v {
        Cbor::Text(s) => Ok(s.clone()),
        _ => Err(corrupt("expected text")),
    }
}

fn as_u64(v: &Cbor) -> Result<u64> {
    match v {
        Cbor::Uint(u) => Ok(*u),
        _ => Err(corrupt("expected uint")),
    }
}

fn as_array(v: &Cbor) -> Result<&[Cbor]> {
    match v {
        Cbor::Array(a) => Ok(a),
        _ => Err(corrupt("expected array")),
    }
}

fn cell(v: &Cbor) -> Result<Value> {
    cell_from(v.clone())
}

fn row(v: &Cbor) -> Result<Vec<Value>> {
    as_array(v)?.iter().map(cell).collect()
}

fn rows(v: &Cbor) -> Result<Vec<Vec<Value>>> {
    as_array(v)?.iter().map(row).collect()
}

fn strings(v: &Cbor) -> Result<Vec<String>> {
    as_array(v)?.iter().map(as_text).collect()
}

fn columns(v: &Cbor) -> Result<Vec<Column>> {
    as_array(v)?
        .iter()
        .map(|c| {
            let Cbor::Map(e) = c else {
                return Err(corrupt("column is not a map"));
            };
            Ok(Column {
                name: as_text(get(e, "name")?)?,
                type_name: as_text(get(e, "type")?)?,
            })
        })
        .collect()
}

fn statement(it: Cbor) -> Result<Statement> {
    let Cbor::Map(e) = it else {
        return Err(corrupt("statement is not a map"));
    };
    Ok(match as_text(get(&e, "kind")?)?.as_str() {
        "Select" => Statement::Select {
            labels: strings(get(&e, "labels")?)?,
            rows: rows(get(&e, "rows")?)?,
        },
        "SelectMap" => Statement::SelectMap(select_map_rows(get(&e, "rows")?)?),
        "ShowColumns" => Statement::ShowColumns(columns(get(&e, "columns")?)?),
        "Insert" => Statement::Insert(as_u64(get(&e, "count")?)?),
        "Delete" => Statement::Delete(as_u64(get(&e, "count")?)?),
        "Update" => Statement::Update(as_u64(get(&e, "count")?)?),
        "DropTable" => Statement::DropTable(as_u64(get(&e, "count")?)?),
        "Create" => Statement::Create,
        "DropFunction" => Statement::DropFunction,
        "AlterTable" => Statement::AlterTable,
        "CreateIndex" => Statement::CreateIndex,
        "DropIndex" => Statement::DropIndex,
        "StartTransaction" => Statement::StartTransaction,
        "Commit" => Statement::Commit,
        "Rollback" => Statement::Rollback,
        "ShowVariable" => Statement::ShowVariable(show_variable(&e)?),
        other => return Err(corrupt(&format!("unknown statement kind '{other}'"))),
    })
}

fn select_map_rows(v: &Cbor) -> Result<Vec<BTreeMap<String, Value>>> {
    as_array(v)?
        .iter()
        .map(|m| {
            let Cbor::Map(e) = m else {
                return Err(corrupt("selectmap row is not a map"));
            };
            let mut out = BTreeMap::new();
            for (k, val) in e {
                out.insert(as_text(k)?, cell(val)?);
            }
            Ok(out)
        })
        .collect()
}

fn show_variable(e: &[(Cbor, Cbor)]) -> Result<ShowVariable> {
    Ok(match as_text(get(e, "variable")?)?.as_str() {
        "Tables" => ShowVariable::Tables(strings(get(e, "values")?)?),
        "Functions" => ShowVariable::Functions(strings(get(e, "values")?)?),
        "Version" => ShowVariable::Version(as_text(get(e, "value")?)?),
        other => return Err(corrupt(&format!("unknown variable '{other}'"))),
    })
}

fn reader(e: &[(Cbor, Cbor)]) -> Result<Reader> {
    Ok(match as_text(get(e, "kind")?)?.as_str() {
        "Rows" => Reader::Rows {
            columns: columns(get(e, "columns")?)?,
            rows: rows(get(e, "rows")?)?,
        },
        "Blame" => Reader::Blame(blame_rows(get(e, "rows")?)?),
        "Diff" => Reader::Diff(diffs(get(e, "diffs")?)?),
        "CommitLog" => Reader::CommitLog(strings(get(e, "commits")?)?),
        "Merge" => Reader::Merge(merge(e)?),
        other => return Err(corrupt(&format!("unknown reader kind '{other}'"))),
    })
}

fn blame_rows(v: &Cbor) -> Result<Vec<BlameRow>> {
    as_array(v)?
        .iter()
        .map(|r| {
            let Cbor::Map(e) = r else {
                return Err(corrupt("blame row is not a map"));
            };
            Ok(BlameRow {
                commit: as_text(get(e, "commit")?)?,
                values: row(get(e, "values")?)?,
            })
        })
        .collect()
}

fn diffs(v: &Cbor) -> Result<Vec<RowChange>> {
    as_array(v)?
        .iter()
        .map(|d| {
            let Cbor::Map(e) = d else {
                return Err(corrupt("diff entry is not a map"));
            };
            Ok(match as_text(get(e, "change")?)?.as_str() {
                "added" => RowChange::Added(row(get(e, "values")?)?),
                "removed" => RowChange::Removed(row(get(e, "values")?)?),
                "updated" => RowChange::Updated {
                    from: row(get(e, "from")?)?,
                    to: row(get(e, "to")?)?,
                },
                other => return Err(corrupt(&format!("unknown change '{other}'"))),
            })
        })
        .collect()
}

fn merge(e: &[(Cbor, Cbor)]) -> Result<Merge> {
    Ok(match as_text(get(e, "outcome")?)?.as_str() {
        "up_to_date" => Merge::UpToDate,
        "fast_forward" => Merge::FastForward(as_text(get(e, "commit")?)?),
        "merged" => Merge::Merged(as_text(get(e, "commit")?)?),
        "conflicts" => Merge::Conflicts(strings(get(e, "paths")?)?),
        other => return Err(corrupt(&format!("unknown outcome '{other}'"))),
    })
}
