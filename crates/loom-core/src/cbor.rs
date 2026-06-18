//! Shared Loom Canonical CBOR (`loom_codec`) helpers for the object model and the facets.
//!
//! One codec defines every content-addressed byte form. The object model frames objects as
//! `[epoch, type, ...fields]` via [`loom_codec::encode_object`]; the facets store bare canonical CBOR
//! values (arrays/maps). Both share the value <-> domain conversions here so there is a single decode
//! discipline, not one re-derived per module.

use crate::digest::{DIGEST_LEN, Digest};
use crate::error::{LoomError, Result};

pub(crate) use loom_codec::Value;

/// Map a codec error into a corrupt-object error.
pub(crate) fn err(e: loom_codec::CodecError) -> LoomError {
    LoomError::corrupt(format!("cbor: {e}"))
}

/// Encode a value whose data is known finite (no NaN/infinity) with duplicate-free maps - true for
/// every object and facet built from validated engine state, so encoding cannot fail.
pub(crate) fn encode(value: &Value) -> Vec<u8> {
    loom_codec::encode(value).expect("loom canonical CBOR encode is infallible for engine data")
}

/// Strictly decode a single canonical value.
pub(crate) fn decode(bytes: &[u8]) -> Result<Value> {
    loom_codec::decode(bytes).map_err(err)
}

/// Strictly decode a top-level array (the common facet framing).
pub(crate) fn decode_array(bytes: &[u8]) -> Result<Vec<Value>> {
    as_array(decode(bytes)?)
}

/// A digest as a 32-byte CBOR byte string.
pub(crate) fn digest_value(d: &Digest) -> Value {
    Value::Bytes(d.bytes().to_vec())
}

pub(crate) fn as_uint(v: Value) -> Result<u64> {
    match v {
        Value::Uint(n) => Ok(n),
        _ => Err(LoomError::corrupt("expected a uint")),
    }
}

pub(crate) fn u8_from(n: u64) -> Result<u8> {
    u8::try_from(n).map_err(|_| LoomError::corrupt("value out of u8 range"))
}

/// A signed integer from either CBOR major type 0 (`Uint`) or 1 (`Nint`).
pub(crate) fn as_int(v: Value) -> Result<i64> {
    match v {
        Value::Uint(n) => {
            i64::try_from(n).map_err(|_| LoomError::corrupt("integer out of i64 range"))
        }
        Value::Nint(n) => {
            let n = i64::try_from(n).map_err(|_| LoomError::corrupt("integer out of i64 range"))?;
            Ok(-1 - n)
        }
        _ => Err(LoomError::corrupt("expected an integer")),
    }
}

pub(crate) fn as_bool(v: Value) -> Result<bool> {
    match v {
        Value::Bool(b) => Ok(b),
        _ => Err(LoomError::corrupt("expected a bool")),
    }
}

pub(crate) fn as_bytes(v: Value) -> Result<Vec<u8>> {
    match v {
        Value::Bytes(b) => Ok(b),
        _ => Err(LoomError::corrupt("expected a byte string")),
    }
}

pub(crate) fn as_text(v: Value) -> Result<String> {
    match v {
        Value::Text(s) => Ok(s),
        _ => Err(LoomError::corrupt("expected a text string")),
    }
}

pub(crate) fn as_array(v: Value) -> Result<Vec<Value>> {
    match v {
        Value::Array(a) => Ok(a),
        _ => Err(LoomError::corrupt("expected an array")),
    }
}

pub(crate) fn as_map(v: Value) -> Result<Vec<(Value, Value)>> {
    match v {
        Value::Map(m) => Ok(m),
        _ => Err(LoomError::corrupt("expected a map")),
    }
}

pub(crate) fn as_digest(v: Value) -> Result<Digest> {
    let bytes = as_bytes(v)?;
    let arr: [u8; DIGEST_LEN] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt("digest field is not 32 bytes"))?;
    Ok(Digest::from_blake3_bytes(arr))
}

/// Consumes a decoded array's positional fields with a per-field type check; [`Fields::end`] rejects
/// any extra trailing field.
pub(crate) struct Fields {
    items: std::vec::IntoIter<Value>,
}

impl Fields {
    pub(crate) fn new(items: Vec<Value>) -> Self {
        Self {
            items: items.into_iter(),
        }
    }

    pub(crate) fn next_field(&mut self) -> Result<Value> {
        self.items
            .next()
            .ok_or_else(|| LoomError::corrupt("missing field"))
    }

    pub(crate) fn uint(&mut self) -> Result<u64> {
        as_uint(self.next_field()?)
    }

    pub(crate) fn int(&mut self) -> Result<i64> {
        as_int(self.next_field()?)
    }

    pub(crate) fn bool(&mut self) -> Result<bool> {
        as_bool(self.next_field()?)
    }

    pub(crate) fn bytes(&mut self) -> Result<Vec<u8>> {
        as_bytes(self.next_field()?)
    }

    pub(crate) fn text(&mut self) -> Result<String> {
        as_text(self.next_field()?)
    }

    pub(crate) fn array(&mut self) -> Result<Vec<Value>> {
        as_array(self.next_field()?)
    }

    pub(crate) fn map(&mut self) -> Result<Vec<(Value, Value)>> {
        as_map(self.next_field()?)
    }

    pub(crate) fn digest(&mut self) -> Result<Digest> {
        as_digest(self.next_field()?)
    }

    pub(crate) fn end(mut self) -> Result<()> {
        if self.items.next().is_some() {
            Err(LoomError::corrupt("unexpected extra fields"))
        } else {
            Ok(())
        }
    }
}
