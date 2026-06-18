//! **Lossless bridge JSON** - a React Native bridge projection of a result payload.
//!
//! This is **not** the normative wire form (that is canonical CBOR) and **not** the general typed
//! binding API (that is [`crate::result_view`]). It exists only because the RN bridge (folly::dynamic /
//! JSI codegen) cannot carry `BigInt`, `Uint8Array`, or non-finite numbers: the RN native layer calls
//! one Rust helper, returns the string, and TS `JSON.parse`s it - so CBOR decoding and the cell tag
//! table stay in Rust (built on the one shared [`crate::result_view::decode`]).
//!
//! Losslessness: `null`, `bool`, in-range `i8`/`i16`/`i32`/`u8`/`u16`/`u32`, finite non-`-0.0` `f64`,
//! and text are bare JSON primitives; everything that JSON / a JS `number` cannot hold exactly is a
//! tagged single-key object. A `List` is a bare JSON array. Bytes are base64; floats that can be
//! NaN/Inf/`-0.0` carry raw IEEE-754 bits.
//!
//! Licensed under BUSL-1.1.

use crate::result_view::{self, Merge, Reader, ResultPayload, RowChange, ShowVariable, Statement};
use loom_types::error::{LoomError, Result};
use loom_types::tabular::Value;
use serde_json::{Map, Value as J, json};

/// Render a canonical result buffer to lossless bridge JSON. See the module docs.
///
/// # Errors
/// Returns [`LoomError`] when the buffer is not a canonical result payload or cannot be rendered.
pub fn to_bridge_json(bytes: &[u8]) -> Result<String> {
    let payload = result_view::decode(bytes)?;
    let v = match payload {
        ResultPayload::Statements(s) => J::Array(s.iter().map(statement_json).collect()),
        ResultPayload::Reader(r) => reader_json(&r),
    };
    serde_json::to_string(&v).map_err(|e| LoomError::corrupt(format!("bridge json: {e}")))
}

fn tagged(key: &str, value: J) -> J {
    let mut o = Map::new();
    o.insert(key.to_string(), value);
    J::Object(o)
}

fn cells(row: &[Value]) -> Vec<J> {
    row.iter().map(cell_json).collect()
}

/// One cell as a lossless bridge value (bare primitive, bare array, or single-key tagged object).
fn cell_json(v: &Value) -> J {
    match v {
        Value::Null => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::I8(x) => json!(*x),
        Value::I16(x) => json!(*x),
        Value::I32(x) => json!(*x),
        Value::U8(x) => json!(*x),
        Value::U16(x) => json!(*x),
        Value::U32(x) => json!(*x),
        Value::Int(x) => tagged("$i64", J::String(x.to_string())),
        Value::U64(x) => tagged("$u64", J::String(x.to_string())),
        Value::I128(x) => tagged("$i128", J::String(x.to_string())),
        Value::U128(x) => tagged("$u128", J::String(x.to_string())),
        Value::Float(f) => float_json(*f),
        Value::F32(f) => tagged("$f32", json!(f.to_bits())),
        Value::Text(s) => J::String(s.clone()),
        Value::Bytes(b) => tagged("$bytes", J::String(base64(b))),
        Value::Decimal { mantissa, scale } => tagged(
            "$decimal",
            json!({ "mantissa": mantissa.to_string(), "scale": scale }),
        ),
        Value::Date(d) => tagged("$date", json!(*d)),
        Value::Time(t) => tagged("$time", J::String(t.to_string())),
        Value::Timestamp(t) => tagged("$timestamp", J::String(t.to_string())),
        Value::Interval { months, micros } => tagged(
            "$interval",
            json!({ "months": months, "micros": micros.to_string() }),
        ),
        Value::Uuid(u) => tagged("$uuid", J::String(format!("{u:032x}"))),
        Value::Inet(ip) => tagged("$inet", J::String(ip.to_string())),
        Value::Point { x, y } => tagged(
            "$point",
            json!({ "x": x.to_bits().to_string(), "y": y.to_bits().to_string() }),
        ),
        Value::List(items) => J::Array(items.iter().map(cell_json).collect()),
        Value::Map(m) => tagged(
            "$map",
            J::Object(
                m.iter()
                    .map(|(k, val)| (k.clone(), cell_json(val)))
                    .collect(),
            ),
        ),
    }
}

/// A finite, non-`-0.0` f64 as a bare JSON number (round-trips exactly); otherwise raw bits under
/// `$f64` so NaN / +-Inf / `-0.0` survive.
fn float_json(f: f64) -> J {
    if f.is_finite() && !(f == 0.0 && f.is_sign_negative()) {
        serde_json::Number::from_f64(f).map_or_else(
            || tagged("$f64", J::String(f.to_bits().to_string())),
            J::Number,
        )
    } else {
        tagged("$f64", J::String(f.to_bits().to_string()))
    }
}

fn statement_json(s: &Statement) -> J {
    match s {
        Statement::Select { labels, rows } => json!({
            "kind": "select",
            "columns": labels.iter().map(|n| json!({ "name": n })).collect::<Vec<_>>(),
            "rows": rows.iter().map(|r| cells(r)).collect::<Vec<_>>(),
        }),
        Statement::SelectMap(rows) => json!({
            "kind": "selectMap",
            "rows": rows
                .iter()
                .map(|m| J::Object(m.iter().map(|(k, v)| (k.clone(), cell_json(v))).collect()))
                .collect::<Vec<_>>(),
        }),
        Statement::ShowColumns(cols) => json!({
            "kind": "showColumns",
            "columns": cols
                .iter()
                .map(|c| json!({ "name": c.name, "type": c.type_name }))
                .collect::<Vec<_>>(),
        }),
        Statement::Insert(n) => json!({ "kind": "insert", "count": n }),
        Statement::Delete(n) => json!({ "kind": "delete", "count": n }),
        Statement::Update(n) => json!({ "kind": "update", "count": n }),
        Statement::DropTable(n) => json!({ "kind": "dropTable", "count": n }),
        Statement::Create => json!({ "kind": "create" }),
        Statement::DropFunction => json!({ "kind": "dropFunction" }),
        Statement::AlterTable => json!({ "kind": "alterTable" }),
        Statement::CreateIndex => json!({ "kind": "createIndex" }),
        Statement::DropIndex => json!({ "kind": "dropIndex" }),
        Statement::StartTransaction => json!({ "kind": "startTransaction" }),
        Statement::Commit => json!({ "kind": "commit" }),
        Statement::Rollback => json!({ "kind": "rollback" }),
        Statement::ShowVariable(sv) => match sv {
            ShowVariable::Tables(v) => {
                json!({ "kind": "showVariable", "variable": "tables", "values": v })
            }
            ShowVariable::Functions(v) => {
                json!({ "kind": "showVariable", "variable": "functions", "values": v })
            }
            ShowVariable::Version(s) => {
                json!({ "kind": "showVariable", "variable": "version", "value": s })
            }
        },
    }
}

fn reader_json(r: &Reader) -> J {
    match r {
        Reader::Rows { columns, rows } => json!({
            "kind": "rows",
            "columns": columns
                .iter()
                .map(|c| json!({ "name": c.name, "type": c.type_name }))
                .collect::<Vec<_>>(),
            "rows": rows.iter().map(|r| cells(r)).collect::<Vec<_>>(),
        }),
        Reader::Blame(rows) => json!({
            "kind": "blame",
            "rows": rows
                .iter()
                .map(|b| json!({ "commit": b.commit, "values": cells(&b.values) }))
                .collect::<Vec<_>>(),
        }),
        Reader::Diff(diffs) => json!({
            "kind": "diff",
            "diffs": diffs
                .iter()
                .map(|d| match d {
                    RowChange::Added(vs) => json!({ "change": "added", "values": cells(vs) }),
                    RowChange::Removed(vs) => json!({ "change": "removed", "values": cells(vs) }),
                    RowChange::Updated { from, to } => {
                        json!({ "change": "updated", "from": cells(from), "to": cells(to) })
                    }
                })
                .collect::<Vec<_>>(),
        }),
        Reader::CommitLog(c) => json!({ "kind": "commitLog", "commits": c }),
        Reader::Merge(m) => match m {
            Merge::UpToDate => json!({ "kind": "merge", "outcome": "up_to_date" }),
            Merge::FastForward(d) => {
                json!({ "kind": "merge", "outcome": "fast_forward", "commit": d })
            }
            Merge::Merged(d) => json!({ "kind": "merge", "outcome": "merged", "commit": d }),
            Merge::Conflicts(p) => json!({ "kind": "merge", "outcome": "conflicts", "paths": p }),
        },
    }
}

/// Standard base64 (RFC 4648, `+/`, `=` padding) - avoids a dependency for the one place RN needs it.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
