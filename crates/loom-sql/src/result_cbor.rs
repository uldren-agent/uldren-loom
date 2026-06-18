//! Canonical-CBOR marshalling of engine results: the **normative** result-payload wire form
//! the FFI and language bindings return.
//!
//! Every result is built directly as Loom Canonical CBOR, and every scalar rides through the one
//! type-faithful cell codec ([`loom_core::tabular::cell_value`] / [`cell_from`]) shared with the
//! storage-identity path - so a 128-bit integer, a non-finite float, an exact `f32`, a decimal, or a
//! byte string crosses the boundary bit-exact. There is no serde_json route in the result path, so the
//! payload can never silently widen, truncate, or drop a value.
//!
//! JSON is never the wire form: the engine-free `loom_result::result_to_json` decoder (in the
//! `loom-result` crate) decodes a canonical buffer back to readable JSON for debugging only (the binding
//! decoders read the CBOR cells directly). This module is the encoder half of that pair; the decoder
//! lives in `loom-result` so engine-free consumers (bindings, wire frontends) need not depend on the SQL
//! engine to read a result buffer.

use loom_codec::{CodecError, Value as Cbor};
use loom_core::error::{LoomError, Result};
use loom_core::tabular::RowDiff;
use loom_core::tabular::{Predicate, Row, Schema, Table, TableDiffRecord, cell_value};
use loom_core::vcs::{Change, ChangeKind};
use loom_core::{Digest, MergeOutcome};

fn cbor_err(e: CodecError) -> LoomError {
    LoomError::corrupt(format!("result cbor: {e}"))
}

fn text(s: impl Into<String>) -> Cbor {
    Cbor::Text(s.into())
}

fn encode(v: &Cbor) -> Result<Vec<u8>> {
    loom_codec::encode(v).map_err(cbor_err)
}

/// The schema columns as `[{ "name", "type" }]` (type is the `ColumnType` debug label).
fn columns(schema: &Schema) -> Cbor {
    Cbor::Array(
        schema
            .columns
            .iter()
            .map(|(name, ty)| {
                Cbor::Map(vec![
                    (text("name"), text(name.clone())),
                    (text("type"), text(format!("{ty:?}"))),
                ])
            })
            .collect(),
    )
}

fn schema_value(schema: &Schema) -> Cbor {
    Cbor::Map(vec![
        (text("columns"), columns(schema)),
        (
            text("primary_key"),
            Cbor::Array(
                schema
                    .primary_key
                    .iter()
                    .map(|&index| Cbor::Uint(index as u64))
                    .collect(),
            ),
        ),
        (
            text("indexes"),
            Cbor::Array(
                schema
                    .indexes
                    .iter()
                    .map(|index| {
                        Cbor::Map(vec![
                            (text("name"), text(index.name.clone())),
                            (
                                text("columns"),
                                Cbor::Array(
                                    index
                                        .columns
                                        .iter()
                                        .map(|&column| Cbor::Uint(column as u64))
                                        .collect(),
                                ),
                            ),
                            (text("unique"), Cbor::Bool(index.unique)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

/// One row as a CBOR array of faithful cells (schema column order).
fn row_cells(row: &Row) -> Cbor {
    Cbor::Array(row.iter().map(cell_value).collect())
}

fn rows_envelope(schema: &Schema, rows: &[&Row]) -> Cbor {
    Cbor::Map(vec![
        (text("kind"), text("Rows")),
        (text("columns"), columns(schema)),
        (
            text("rows"),
            Cbor::Array(rows.iter().map(|r| row_cells(r)).collect()),
        ),
    ])
}

/// A materialized table as canonical CBOR (`{ kind: "Rows", columns, rows }`).
pub fn table_cbor(table: &Table) -> Result<Vec<u8>> {
    let rows = table.scan(&Predicate::All);
    encode(&rows_envelope(table.schema(), &rows))
}

/// A row list under `schema` as canonical CBOR (`{ kind: "Rows", columns, rows }`).
pub fn rows_cbor(schema: &Schema, rows: &[Row]) -> Result<Vec<u8>> {
    let refs: Vec<&Row> = rows.iter().collect();
    encode(&rows_envelope(schema, &refs))
}

/// A blame result as canonical CBOR (`{ kind: "Blame", rows: [ { commit, values } ] }`).
pub fn blame_cbor(rows: &[(Row, Digest)]) -> Result<Vec<u8>> {
    let items = rows
        .iter()
        .map(|(row, commit)| {
            Cbor::Map(vec![
                (text("commit"), text(commit.to_string())),
                (text("values"), row_cells(row)),
            ])
        })
        .collect();
    encode(&Cbor::Map(vec![
        (text("kind"), text("Blame")),
        (text("rows"), Cbor::Array(items)),
    ]))
}

/// A row-level diff as canonical CBOR (`{ kind: "Diff", diffs: [...] }`).
pub fn diff_cbor(diffs: &[RowDiff]) -> Result<Vec<u8>> {
    let items = diffs
        .iter()
        .map(|d| match d {
            RowDiff::Added(r) => Cbor::Map(vec![
                (text("change"), text("added")),
                (text("values"), row_cells(r)),
            ]),
            RowDiff::Removed(r) => Cbor::Map(vec![
                (text("change"), text("removed")),
                (text("values"), row_cells(r)),
            ]),
            RowDiff::Updated { from, to } => Cbor::Map(vec![
                (text("change"), text("updated")),
                (text("from"), row_cells(from)),
                (text("to"), row_cells(to)),
            ]),
        })
        .collect();
    encode(&Cbor::Map(vec![
        (text("kind"), text("Diff")),
        (text("diffs"), Cbor::Array(items)),
    ]))
}

/// A schema-aware table diff as canonical CBOR (`{ kind: "TableDiff", records: [...] }`).
pub fn table_diff_cbor(records: &[TableDiffRecord]) -> Result<Vec<u8>> {
    let items = records
        .iter()
        .map(|record| match record {
            TableDiffRecord::SchemaChanged { from, to } => Cbor::Map(vec![
                (text("change"), text("schema_changed")),
                (text("from"), from.as_ref().map_or(Cbor::Null, schema_value)),
                (text("to"), to.as_ref().map_or(Cbor::Null, schema_value)),
            ]),
            TableDiffRecord::Row(RowDiff::Added(row)) => Cbor::Map(vec![
                (text("change"), text("added")),
                (text("values"), row_cells(row)),
            ]),
            TableDiffRecord::Row(RowDiff::Removed(row)) => Cbor::Map(vec![
                (text("change"), text("removed")),
                (text("values"), row_cells(row)),
            ]),
            TableDiffRecord::Row(RowDiff::Updated { from, to }) => Cbor::Map(vec![
                (text("change"), text("updated")),
                (text("from"), row_cells(from)),
                (text("to"), row_cells(to)),
            ]),
        })
        .collect();
    encode(&Cbor::Map(vec![
        (text("kind"), text("TableDiff")),
        (text("records"), Cbor::Array(items)),
    ]))
}

/// A path-level diff as canonical CBOR (`{ kind: "PathDiff", changes: [{ path, change }] }`), where
/// `change` is `"added"`, `"modified"`, or `"deleted"`. The workspace/entry-level counterpart of
/// [`diff_cbor`] (which is row-level).
pub fn path_diff_cbor(changes: &[Change]) -> Result<Vec<u8>> {
    let items = changes
        .iter()
        .map(|c| {
            let change = match c.kind {
                ChangeKind::Added => "added",
                ChangeKind::Modified => "modified",
                ChangeKind::Deleted => "deleted",
            };
            Cbor::Map(vec![
                (text("path"), text(c.path.clone())),
                (text("change"), text(change)),
            ])
        })
        .collect();
    encode(&Cbor::Map(vec![
        (text("kind"), text("PathDiff")),
        (text("changes"), Cbor::Array(items)),
    ]))
}

/// A path-level blame as canonical CBOR (`{ kind: "PathBlame", paths: [{ path, commit }] }`). The
/// workspace/entry-level counterpart of [`blame_cbor`] (which is row-level).
pub fn path_blame_cbor(paths: &[(String, Digest)]) -> Result<Vec<u8>> {
    let items = paths
        .iter()
        .map(|(path, commit)| {
            Cbor::Map(vec![
                (text("path"), text(path.clone())),
                (text("commit"), text(commit.to_string())),
            ])
        })
        .collect();
    encode(&Cbor::Map(vec![
        (text("kind"), text("PathBlame")),
        (text("paths"), Cbor::Array(items)),
    ]))
}

/// A first-parent commit log as canonical CBOR (`{ kind: "CommitLog", commits: [addr, ...] }`).
pub fn commit_log_cbor(commits: &[Digest]) -> Result<Vec<u8>> {
    encode(&Cbor::Map(vec![
        (text("kind"), text("CommitLog")),
        (
            text("commits"),
            Cbor::Array(commits.iter().map(|d| text(d.to_string())).collect()),
        ),
    ]))
}

/// A merge outcome as canonical CBOR (`{ kind: "Merge", outcome, ... }`).
pub fn merge_outcome_cbor(outcome: &MergeOutcome) -> Result<Vec<u8>> {
    let mut entries = vec![(text("kind"), text("Merge"))];
    match outcome {
        MergeOutcome::UpToDate => entries.push((text("outcome"), text("up_to_date"))),
        MergeOutcome::FastForward(d) => {
            entries.push((text("outcome"), text("fast_forward")));
            entries.push((text("commit"), text(d.to_string())));
        }
        MergeOutcome::Merged(d) => {
            entries.push((text("outcome"), text("merged")));
            entries.push((text("commit"), text(d.to_string())));
        }
        MergeOutcome::Conflicts(paths) => {
            entries.push((text("outcome"), text("conflicts")));
            entries.push((
                text("paths"),
                Cbor::Array(paths.iter().map(|p| text(p.clone())).collect()),
            ));
        }
    }
    encode(&Cbor::Map(entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::tabular::{ColumnType, Schema, Table, Value, cell_from};

    fn decode(bytes: &[u8]) -> Cbor {
        loom_codec::decode(bytes).unwrap()
    }

    /// Pull the first row's cells out of a `{ kind: "Rows", columns, rows }` envelope.
    fn first_row(bytes: &[u8]) -> Vec<Value> {
        let Cbor::Map(entries) = decode(bytes) else {
            panic!("not a map")
        };
        let rows = entries
            .into_iter()
            .find_map(|(k, v)| matches!(&k, Cbor::Text(s) if s == "rows").then_some(v))
            .expect("rows field");
        let Cbor::Array(mut rows) = rows else {
            panic!("rows not array")
        };
        let Cbor::Array(cells) = rows.remove(0) else {
            panic!("row not array")
        };
        cells.into_iter().map(|c| cell_from(c).unwrap()).collect()
    }

    #[test]
    fn rows_envelope_carries_faithful_cells() {
        // The hard cases the old serde_json route broke: a 128-bit int beyond i64/u64, a NaN float, an
        // exact f32, a decimal, and raw bytes - all must survive the result payload bit-exact.
        let schema = Schema::new(
            vec![
                ("id".into(), ColumnType::Int),
                ("big".into(), ColumnType::U128),
                ("nan".into(), ColumnType::Float),
                ("f".into(), ColumnType::F32),
                ("d".into(), ColumnType::Decimal),
                ("b".into(), ColumnType::Bytes),
            ],
            vec![0],
        )
        .unwrap();
        let mut t = Table::new(schema);
        let big = u128::from(u64::MAX) + 1;
        t.insert(vec![
            Value::Int(1),
            Value::U128(big),
            Value::Float(f64::NAN),
            Value::F32(0.1f32),
            Value::Decimal {
                mantissa: 12_345,
                scale: 2,
            },
            Value::Bytes(vec![0, 1, 2, 255]),
        ])
        .unwrap();

        let bytes = table_cbor(&t).unwrap();
        let row = first_row(&bytes);
        assert_eq!(row[1], Value::U128(big));
        match row[2] {
            Value::Float(f) => assert!(f.is_nan(), "nan must round-trip"),
            ref other => panic!("expected float, got {other:?}"),
        }
        assert_eq!(row[3], Value::F32(0.1f32));
        assert_eq!(
            row[4],
            Value::Decimal {
                mantissa: 12_345,
                scale: 2
            }
        );
        assert_eq!(row[5], Value::Bytes(vec![0, 1, 2, 255]));
    }

    #[test]
    fn debug_render_shows_kind_and_typed_cells() {
        let schema = Schema::new(
            vec![
                ("id".into(), ColumnType::Int),
                ("name".into(), ColumnType::Text),
            ],
            vec![0],
        )
        .unwrap();
        let mut t = Table::new(schema);
        t.insert(vec![Value::Int(1), Value::Text("a".into())])
            .unwrap();
        let json = loom_result::result_to_json(&table_cbor(&t).unwrap()).unwrap();
        assert!(json.contains("\"Rows\""), "{json}");
        assert!(json.contains("\"columns\""), "{json}");
        assert!(
            json.contains("\"Int\"") && json.contains("\"Text\""),
            "{json}"
        );
        assert!(json.contains("\"a\""), "{json}");
    }

    #[test]
    fn diff_and_blame_envelopes_render() {
        let diffs = vec![
            RowDiff::Added(vec![Value::Int(1), Value::Text("a".into())]),
            RowDiff::Updated {
                from: vec![Value::Int(2), Value::Text("b".into())],
                to: vec![Value::Int(2), Value::Text("c".into())],
            },
        ];
        let dj = loom_result::result_to_json(&diff_cbor(&diffs).unwrap()).unwrap();
        assert!(
            dj.contains("\"added\"") && dj.contains("\"updated\""),
            "{dj}"
        );
        assert!(dj.contains("\"c\""), "{dj}");

        let bj = loom_result::result_to_json(
            &blame_cbor(&[(
                vec![Value::Int(1), Value::Text("a".into())],
                Digest::blake3(b"x"),
            )])
            .unwrap(),
        )
        .unwrap();
        assert!(bj.contains("\"commit\"") && bj.contains("blake3:"), "{bj}");
    }
}
