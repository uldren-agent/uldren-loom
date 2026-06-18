//! Canonical CBOR codec runtime for method arguments and results.
//!
//! A method call carries its arguments as one canonical CBOR array and its result as one canonical CBOR
//! value. This module is the hand-written runtime the generated per-method codecs call: it
//! maps the IDL primitive types to and from [`loom_codec::Value`], plus opaque handle ids. Generated code
//! composes these conversions; it does not reimplement CBOR.
//!

use loom_codec::{CodecError, Value, decode, encode};

/// A decode failure for a typed argument or result: a CBOR-level error, a shape mismatch, or an integer
/// that does not fit the target type.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ArgError {
    /// The underlying canonical CBOR was invalid.
    #[error(transparent)]
    Codec(#[from] CodecError),
    /// The value was the wrong CBOR shape for the target type.
    #[error("type mismatch: expected {expected}")]
    TypeMismatch {
        /// The Rust type that was expected.
        expected: &'static str,
    },
    /// A CBOR integer did not fit the target width.
    #[error("integer out of range for {target}")]
    IntRange {
        /// The target integer type.
        target: &'static str,
    },
    /// A method argument array had the wrong arity.
    #[error("expected {expected} arguments, found {found}")]
    Arity {
        /// The expected argument count.
        expected: usize,
        /// The actual argument count.
        found: usize,
    },
}

/// A value that can be encoded to the canonical CBOR value model.
pub trait ToValue {
    /// Convert to a [`Value`].
    fn to_value(&self) -> Value;
}

/// A value that can be decoded from the canonical CBOR value model.
pub trait FromValue: Sized {
    /// Convert from a [`Value`].
    ///
    /// # Errors
    /// Returns [`ArgError`] on a shape mismatch or an out-of-range integer.
    fn from_value(value: &Value) -> Result<Self, ArgError>;
}

/// Encode a method argument list as one canonical CBOR array.
///
/// # Errors
/// Returns [`CodecError`] only on a non-finite float in an argument.
pub fn encode_args(args: &[Value]) -> Result<Vec<u8>, CodecError> {
    encode(&Value::Array(args.to_vec()))
}

/// Decode a canonical CBOR array of method arguments.
///
/// # Errors
/// Returns [`ArgError`] when the bytes are not a canonical array.
pub fn decode_args(bytes: &[u8]) -> Result<Vec<Value>, ArgError> {
    match decode(bytes)? {
        Value::Array(items) => Ok(items),
        _ => Err(ArgError::TypeMismatch {
            expected: "argument array",
        }),
    }
}

/// Take an argument at `index` from a decoded array, decoding it to `T`.
///
/// # Errors
/// Returns [`ArgError::Arity`] when the index is out of range, or a decode error for the element.
pub fn arg<T: FromValue>(args: &[Value], index: usize) -> Result<T, ArgError> {
    let value = args.get(index).ok_or(ArgError::Arity {
        expected: index + 1,
        found: args.len(),
    })?;
    T::from_value(value)
}

// ---- primitive conversions ----------------------------------------------------------------------

impl ToValue for bool {
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
}

impl FromValue for bool {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        match value {
            Value::Bool(b) => Ok(*b),
            _ => Err(ArgError::TypeMismatch { expected: "bool" }),
        }
    }
}

impl ToValue for u64 {
    fn to_value(&self) -> Value {
        Value::Uint(*self)
    }
}

impl FromValue for u64 {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        match value {
            Value::Uint(n) => Ok(*n),
            _ => Err(ArgError::TypeMismatch { expected: "u64" }),
        }
    }
}

impl ToValue for u32 {
    fn to_value(&self) -> Value {
        Value::Uint(u64::from(*self))
    }
}

impl FromValue for u32 {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        match value {
            Value::Uint(n) => u32::try_from(*n).map_err(|_| ArgError::IntRange { target: "u32" }),
            _ => Err(ArgError::TypeMismatch { expected: "u32" }),
        }
    }
}

impl ToValue for i64 {
    fn to_value(&self) -> Value {
        Value::int(*self)
    }
}

impl FromValue for i64 {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        match value {
            Value::Uint(n) => i64::try_from(*n).map_err(|_| ArgError::IntRange { target: "i64" }),
            Value::Nint(n) => i64::try_from(*n)
                .ok()
                .and_then(|v| (-1i64).checked_sub(v))
                .ok_or(ArgError::IntRange { target: "i64" }),
            _ => Err(ArgError::TypeMismatch { expected: "i64" }),
        }
    }
}

impl ToValue for i32 {
    fn to_value(&self) -> Value {
        Value::int(i64::from(*self))
    }
}

impl FromValue for i32 {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        let wide = i64::from_value(value)?;
        i32::try_from(wide).map_err(|_| ArgError::IntRange { target: "i32" })
    }
}

impl ToValue for f64 {
    fn to_value(&self) -> Value {
        Value::Float(*self)
    }
}

impl FromValue for f64 {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        match value {
            Value::Float(f) => Ok(*f),
            _ => Err(ArgError::TypeMismatch { expected: "f64" }),
        }
    }
}

impl ToValue for String {
    fn to_value(&self) -> Value {
        Value::Text(self.clone())
    }
}

impl ToValue for str {
    fn to_value(&self) -> Value {
        Value::Text(self.to_string())
    }
}

impl FromValue for String {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        match value {
            Value::Text(s) => Ok(s.clone()),
            _ => Err(ArgError::TypeMismatch { expected: "String" }),
        }
    }
}

/// A newtype for the IDL `bytes` type, distinct from `list<u8>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob(pub Vec<u8>);

impl ToValue for Blob {
    fn to_value(&self) -> Value {
        Value::Bytes(self.0.clone())
    }
}

impl FromValue for Blob {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        match value {
            Value::Bytes(b) => Ok(Blob(b.clone())),
            _ => Err(ArgError::TypeMismatch { expected: "bytes" }),
        }
    }
}

impl<T: ToValue> ToValue for Option<T> {
    fn to_value(&self) -> Value {
        match self {
            Some(inner) => inner.to_value(),
            None => Value::Null,
        }
    }
}

impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        match value {
            Value::Null => Ok(None),
            other => Ok(Some(T::from_value(other)?)),
        }
    }
}

impl<T: ToValue> ToValue for Vec<T> {
    fn to_value(&self) -> Value {
        Value::Array(self.iter().map(ToValue::to_value).collect())
    }
}

impl<T: FromValue> FromValue for Vec<T> {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        match value {
            Value::Array(items) => items.iter().map(T::from_value).collect(),
            _ => Err(ArgError::TypeMismatch { expected: "list" }),
        }
    }
}

/// An opaque remote handle id: kind, server-minted id bytes, generation, and owner session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteHandleId {
    /// The handle kind, for example `session` or `sql_session`.
    pub kind: String,
    /// The server-minted opaque id.
    pub id: Vec<u8>,
    /// The generation counter that invalidates a reclaimed id.
    pub generation: u64,
    /// The owning session id.
    pub owner_session: Vec<u8>,
}

impl ToValue for RemoteHandleId {
    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.kind.clone()),
            Value::Bytes(self.id.clone()),
            Value::Uint(self.generation),
            Value::Bytes(self.owner_session.clone()),
        ])
    }
}

impl FromValue for RemoteHandleId {
    fn from_value(value: &Value) -> Result<Self, ArgError> {
        let Value::Array(items) = value else {
            return Err(ArgError::TypeMismatch {
                expected: "handle id array",
            });
        };
        if items.len() != 4 {
            return Err(ArgError::Arity {
                expected: 4,
                found: items.len(),
            });
        }
        Ok(RemoteHandleId {
            kind: String::from_value(&items[0])?,
            id: Blob::from_value(&items[1])?.0,
            generation: u64::from_value(&items[2])?,
            owner_session: Blob::from_value(&items[3])?.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: ToValue + FromValue + PartialEq + std::fmt::Debug>(value: T) {
        let bytes = encode_args(&[value.to_value()]).unwrap();
        let decoded = decode_args(&bytes).unwrap();
        assert_eq!(T::from_value(&decoded[0]).unwrap(), value);
    }

    #[test]
    fn scalars_round_trip() {
        round_trip(true);
        round_trip(false);
        round_trip(42u64);
        round_trip(7u32);
        round_trip(-5i64);
        round_trip(-5i32);
        round_trip(1234i64);
        round_trip(2.5f64);
        round_trip("hello".to_string());
    }

    #[test]
    fn bytes_and_optional_and_list_round_trip() {
        round_trip(Blob(vec![1, 2, 3]));
        round_trip(Some(9u64));
        round_trip(Option::<u64>::None);
        round_trip(vec![1u64, 2, 3]);
        round_trip(Vec::<Blob>::new());
        round_trip(vec![Blob(vec![0]), Blob(vec![255])]);
    }

    #[test]
    fn handle_id_round_trips() {
        round_trip(RemoteHandleId {
            kind: "sql_session".to_string(),
            id: vec![7, 7, 7],
            generation: 3,
            owner_session: vec![1, 2],
        });
    }

    #[test]
    fn multi_arg_tuple_round_trips() {
        // A representative "struct-shaped" method: (string workspace, string collection, bytes key).
        let args = [
            "ns".to_value(),
            "col".to_value(),
            Blob(vec![9, 8, 7]).to_value(),
        ];
        let bytes = encode_args(&args).unwrap();
        let decoded = decode_args(&bytes).unwrap();
        assert_eq!(decoded.len(), 3);
        assert_eq!(arg::<String>(&decoded, 0).unwrap(), "ns");
        assert_eq!(arg::<String>(&decoded, 1).unwrap(), "col");
        assert_eq!(arg::<Blob>(&decoded, 2).unwrap(), Blob(vec![9, 8, 7]));
    }

    #[test]
    fn type_and_arity_errors_surface() {
        let bytes = encode_args(&["text".to_value()]).unwrap();
        let decoded = decode_args(&bytes).unwrap();
        assert!(matches!(
            arg::<u64>(&decoded, 0),
            Err(ArgError::TypeMismatch { .. })
        ));
        assert!(matches!(
            arg::<String>(&decoded, 5),
            Err(ArgError::Arity { .. })
        ));
    }

    #[test]
    fn u32_and_i32_range_is_checked() {
        let big = encode_args(&[Value::Uint(u64::from(u32::MAX) + 1)]).unwrap();
        let decoded = decode_args(&big).unwrap();
        assert!(matches!(
            u32::from_value(&decoded[0]),
            Err(ArgError::IntRange { .. })
        ));
    }
}
