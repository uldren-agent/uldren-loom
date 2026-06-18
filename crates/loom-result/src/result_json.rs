//! Debug-only JSON rendering of a canonical result buffer.
//!
//! Bindings consume the CBOR cells directly (via [`crate::result_view`]); this renders a canonical
//! buffer to readable JSON for debugging. A faithful cell decodes to a type-tagged scalar (e.g.
//! `{"Int":1}`, `{"Text":"hi"}`, `{"I128":"..."}`); those tag names are the tabular `Value` variants
//! and are a debug convenience, not a stable contract.
//!
//! Licensed under BUSL-1.1.

use loom_codec::{CodecError, Value as Cbor};
use loom_types::error::{LoomError, Result};
use loom_types::tabular::{Value, cell_from};
use rust_decimal::Decimal;
use serde_json::Value as Json;

/// The highest tag the faithful cell codec emits. An array that begins with a `Uint` in
/// `0..=CELL_TAG_MAX` is a faithful cell; no structural array begins with a bare `Uint`, so this is how
/// the renderer tells a cell apart from a structural array (rows, columns, diffs, ...).
const CELL_TAG_MAX: u64 = 25;

fn cbor_err(e: CodecError) -> LoomError {
    LoomError::corrupt(format!("result cbor: {e}"))
}

/// Decode a canonical result buffer back to readable JSON. **Debug only**: bindings consume the CBOR
/// cells directly. Total over any result envelope; faithful cells decode to type-tagged scalars and
/// structural parts render transparently.
///
/// # Errors
/// Returns [`LoomError`] when the buffer is not canonical CBOR or cannot be rendered.
pub fn result_to_json(bytes: &[u8]) -> Result<String> {
    let value = loom_codec::decode(bytes).map_err(cbor_err)?;
    serde_json::to_string(&render(value)?)
        .map_err(|e| LoomError::corrupt(format!("result json: {e}")))
}

/// Render a canonical result value to JSON, decoding any faithful cell it meets back to its typed form.
fn render(v: Cbor) -> Result<Json> {
    Ok(match v {
        Cbor::Array(items) if is_cell(&items) => value_to_json(cell_from(Cbor::Array(items))?),
        Cbor::Array(items) => {
            Json::Array(items.into_iter().map(render).collect::<Result<Vec<_>>>()?)
        }
        Cbor::Map(entries) => Json::Object(
            entries
                .into_iter()
                .map(|(k, val)| {
                    let key = key_text(k);
                    let rendered = if is_schema_field(&key) {
                        render_structural(val)?
                    } else {
                        render(val)?
                    };
                    Ok((key, rendered))
                })
                .collect::<Result<serde_json::Map<String, Json>>>()?,
        ),
        Cbor::Null => Json::Null,
        Cbor::Bool(b) => Json::Bool(b),
        Cbor::Uint(u) => Json::Number(u.into()),
        Cbor::Nint(n) => {
            let val = -1i128 - i128::from(n);
            i64::try_from(val).map_or_else(
                |_| Json::String(val.to_string()),
                |i| Json::Number(i.into()),
            )
        }
        Cbor::Float(f) => float_json(f),
        Cbor::Text(s) => Json::String(s),
        Cbor::Bytes(b) => Json::String(hex(&b)),
    })
}

fn render_structural(v: Cbor) -> Result<Json> {
    Ok(match v {
        Cbor::Array(items) => Json::Array(
            items
                .into_iter()
                .map(render_structural)
                .collect::<Result<Vec<_>>>()?,
        ),
        Cbor::Map(entries) => Json::Object(
            entries
                .into_iter()
                .map(|(k, val)| Ok((key_text(k), render_structural(val)?)))
                .collect::<Result<serde_json::Map<String, Json>>>()?,
        ),
        Cbor::Null => Json::Null,
        Cbor::Bool(b) => Json::Bool(b),
        Cbor::Uint(u) => Json::Number(u.into()),
        Cbor::Nint(n) => {
            let val = -1i128 - i128::from(n);
            i64::try_from(val).map_or_else(
                |_| Json::String(val.to_string()),
                |i| Json::Number(i.into()),
            )
        }
        Cbor::Float(f) => float_json(f),
        Cbor::Text(s) => Json::String(s),
        Cbor::Bytes(b) => Json::String(hex(&b)),
    })
}

fn is_schema_field(key: &str) -> bool {
    matches!(key, "columns" | "primary_key" | "indexes")
}

/// True when `items` is a faithful cell: a `[Uint(tag), ..]` array with `tag <= CELL_TAG_MAX`.
fn is_cell(items: &[Cbor]) -> bool {
    matches!(items.first(), Some(Cbor::Uint(t)) if *t <= CELL_TAG_MAX)
}

/// A tabular value as a type-tagged JSON scalar for the debug view (128-bit ints and decimals carry as
/// strings so the renderer never trips serde_json's `i64`/`u64`-only number range).
fn value_to_json(v: Value) -> Json {
    match v {
        Value::Null => Json::Null,
        Value::Bool(b) => tagged("Bool", Json::Bool(b)),
        Value::Int(i) => tagged("Int", Json::Number(i.into())),
        Value::I8(x) => tagged("I8", Json::Number(i64::from(x).into())),
        Value::I16(x) => tagged("I16", Json::Number(i64::from(x).into())),
        Value::I32(x) => tagged("I32", Json::Number(i64::from(x).into())),
        Value::I128(x) => tagged("I128", Json::String(x.to_string())),
        Value::U8(x) => tagged("U8", Json::Number(u64::from(x).into())),
        Value::U16(x) => tagged("U16", Json::Number(u64::from(x).into())),
        Value::U32(x) => tagged("U32", Json::Number(u64::from(x).into())),
        Value::U64(x) => tagged("U64", Json::Number(x.into())),
        Value::U128(x) => tagged("U128", Json::String(x.to_string())),
        Value::Float(f) => tagged("Float", float_json(f)),
        Value::F32(f) => tagged("F32", float_json(f64::from(f))),
        Value::Text(s) => tagged("Text", Json::String(s)),
        Value::Bytes(b) => tagged("Bytes", Json::String(hex(&b))),
        Value::Decimal { mantissa, scale } => {
            tagged("Decimal", Json::String(decimal_str(mantissa, scale)))
        }
        Value::Date(d) => tagged("Date", Json::Number(i64::from(d).into())),
        Value::Time(t) => tagged("Time", Json::Number(t.into())),
        Value::Timestamp(t) => tagged("Timestamp", Json::Number(t.into())),
        Value::Interval { months, micros } => tagged(
            "Interval",
            serde_json::json!({ "months": months, "micros": micros }),
        ),
        Value::Uuid(u) => tagged("Uuid", Json::String(format!("{u:032x}"))),
        Value::Inet(ip) => tagged("Inet", Json::String(ip.to_string())),
        Value::Point { x, y } => tagged(
            "Point",
            serde_json::json!({ "x": float_json(x), "y": float_json(y) }),
        ),
        Value::List(items) => tagged(
            "List",
            Json::Array(items.into_iter().map(value_to_json).collect()),
        ),
        Value::Map(m) => tagged(
            "Map",
            Json::Object(
                m.into_iter()
                    .map(|(k, val)| (k, value_to_json(val)))
                    .collect(),
            ),
        ),
    }
}

fn tagged(name: &str, inner: Json) -> Json {
    let mut o = serde_json::Map::new();
    o.insert(name.to_string(), inner);
    Json::Object(o)
}

/// Reconstruct a decimal's display form from its `(mantissa, scale)`; falls back to `<m>e-<s>` if the
/// pair is out of `rust_decimal` range (our values never are, but the renderer stays total).
fn decimal_str(mantissa: i128, scale: u32) -> String {
    Decimal::try_from_i128_with_scale(mantissa, scale)
        .map(|d| d.to_string())
        .unwrap_or_else(|_| format!("{mantissa}e-{scale}"))
}

/// A finite float as a JSON number, or its textual form (`NaN`/`inf`/`-inf`) for the debug view.
fn float_json(f: f64) -> Json {
    serde_json::Number::from_f64(f).map_or_else(|| Json::String(f.to_string()), Json::Number)
}

fn key_text(k: Cbor) -> String {
    match k {
        Cbor::Text(s) => s,
        Cbor::Bytes(b) => hex(&b),
        other => format!("{other:?}"),
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
