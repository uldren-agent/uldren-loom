//! Request, response, and error envelopes.
//!
//! The v1 carrier frames Loom Canonical CBOR payloads. A request envelope names the interface, method,
//! and canonical-CBOR argument tuple; a response envelope carries either the canonical-CBOR return value
//! or a stable error object. This module is transport-agnostic: it encodes and decodes the envelope
//! maps, and leaves HTTP/2 framing to the carrier.
//!
//! Licensed under BUSL-1.1.

use crate::RemoteError;
use crate::codec::ArgError;
use loom_codec::{CodecError, Value, decode, encode};

/// The v1 protocol identifier carried by every envelope.
pub const PROTOCOL_ID: &str = "loom.remote.v1";

/// Payload compression selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    /// No compression.
    #[default]
    None,
    /// Zstandard compression.
    Zstd,
}

impl Compression {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Compression::None => "none",
            Compression::Zstd => "zstd",
        }
    }

    /// Parse the wire spelling; unknown values are rejected.
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Compression::None),
            "zstd" => Some(Compression::Zstd),
            _ => None,
        }
    }
}

/// A unary or stream-opening request envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Client-minted request correlation id.
    pub request_id: Vec<u8>,
    /// The session this request runs under, or `None` for credential-free routes.
    pub session_id: Option<Vec<u8>>,
    /// The IDL interface name (for example `Kv`).
    pub interface: String,
    /// The IDL method name (for example `get`).
    pub method: String,
    /// The method argument tuple, one canonical CBOR value per argument.
    pub args: Vec<Value>,
    /// Client deadline in milliseconds; `0` means no client deadline.
    pub deadline_ms: u64,
    /// Idempotency key for a mutating method that is not naturally idempotent.
    pub idempotency_key: Option<Vec<u8>>,
    /// Non-authoritative principal hint; the server binds the authenticated principal itself.
    pub principal_hint: Option<String>,
    /// Requested payload compression.
    pub compression: Compression,
    /// Whether the caller opens a stream rather than a unary call.
    pub stream: bool,
}

impl Request {
    /// Encode to a canonical CBOR envelope map.
    ///
    /// # Errors
    /// Returns [`CodecError`] only if an argument carries a non-finite float.
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let entries = vec![
            (text("protocol"), text(PROTOCOL_ID)),
            (text("request_id"), Value::Bytes(self.request_id.clone())),
            (text("session_id"), opt_bytes(&self.session_id)),
            (text("interface"), text(&self.interface)),
            (text("method"), text(&self.method)),
            (text("args"), Value::Array(self.args.clone())),
            (text("deadline_ms"), Value::Uint(self.deadline_ms)),
            (text("idempotency_key"), opt_bytes(&self.idempotency_key)),
            (text("principal_hint"), opt_text(&self.principal_hint)),
            (text("compression"), text(self.compression.as_str())),
            (text("stream"), Value::Bool(self.stream)),
        ];
        encode(&Value::Map(entries))
    }

    /// Decode from a canonical CBOR envelope map.
    ///
    /// # Errors
    /// Returns [`ArgError`] for a non-map buffer, a wrong protocol id, or a mistyped field.
    pub fn decode(bytes: &[u8]) -> Result<Self, ArgError> {
        let map = as_map(&decode(bytes)?)?;
        require_protocol(&map)?;
        Ok(Self {
            request_id: bytes_field(&map, "request_id")?,
            session_id: opt_bytes_field(&map, "session_id")?,
            interface: text_field(&map, "interface")?,
            method: text_field(&map, "method")?,
            args: array_field(&map, "args")?,
            deadline_ms: u64_field(&map, "deadline_ms")?,
            idempotency_key: opt_bytes_field(&map, "idempotency_key")?,
            principal_hint: opt_text_field(&map, "principal_hint")?,
            compression: compression_field(&map)?,
            stream: bool_field(&map, "stream")?,
        })
    }
}

/// The body of a response: a successful return value or a stable error object.
#[derive(Debug, Clone, PartialEq)]
pub enum ResponsePayload {
    /// The method's canonical-CBOR return value.
    Ok(Value),
    /// A stable error object.
    Err(RemoteError),
}

/// A unary response envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    /// The correlation id echoed from the request.
    pub request_id: Vec<u8>,
    /// The session id echoed from the request, when present.
    pub session_id: Option<Vec<u8>>,
    /// The response body.
    pub payload: ResponsePayload,
}

impl Response {
    /// A successful response for `request_id`/`session_id` carrying `value`.
    pub fn ok(request_id: Vec<u8>, session_id: Option<Vec<u8>>, value: Value) -> Self {
        Self {
            request_id,
            session_id,
            payload: ResponsePayload::Ok(value),
        }
    }

    /// An error response for `request_id`/`session_id` carrying `error`.
    pub fn err(request_id: Vec<u8>, session_id: Option<Vec<u8>>, error: RemoteError) -> Self {
        Self {
            request_id,
            session_id,
            payload: ResponsePayload::Err(error),
        }
    }

    /// Encode to a canonical CBOR envelope map.
    ///
    /// # Errors
    /// Returns [`CodecError`] only if the value carries a non-finite float.
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut entries = vec![
            (text("protocol"), text(PROTOCOL_ID)),
            (text("request_id"), Value::Bytes(self.request_id.clone())),
            (text("session_id"), opt_bytes(&self.session_id)),
        ];
        match &self.payload {
            ResponsePayload::Ok(value) => {
                entries.push((text("ok"), Value::Bool(true)));
                entries.push((text("value"), value.clone()));
            }
            ResponsePayload::Err(error) => {
                entries.push((text("ok"), Value::Bool(false)));
                entries.push((text("error"), encode_error(error)));
            }
        }
        encode(&Value::Map(entries))
    }

    /// Decode from a canonical CBOR envelope map.
    ///
    /// # Errors
    /// Returns [`ArgError`] for a non-map buffer, a wrong protocol id, or a mistyped field.
    pub fn decode(bytes: &[u8]) -> Result<Self, ArgError> {
        let map = as_map(&decode(bytes)?)?;
        require_protocol(&map)?;
        let request_id = bytes_field(&map, "request_id")?;
        let session_id = opt_bytes_field(&map, "session_id")?;
        let payload = if bool_field(&map, "ok")? {
            ResponsePayload::Ok(
                field(&map, "value")
                    .ok_or(ArgError::TypeMismatch {
                        expected: "response value",
                    })?
                    .clone(),
            )
        } else {
            ResponsePayload::Err(decode_error(field(&map, "error").ok_or(
                ArgError::TypeMismatch {
                    expected: "response error",
                },
            )?)?)
        };
        Ok(Self {
            request_id,
            session_id,
            payload,
        })
    }
}

/// Encode a [`RemoteError`] as its error object.
pub(crate) fn encode_error(error: &RemoteError) -> Value {
    Value::Map(vec![
        (text("code"), text(&error.wire_code)),
        (text("message"), text(&error.message)),
        (text("retry"), text(error.retry.as_str())),
        (
            text("retry_after_ms"),
            match error.retry_after_ms {
                Some(ms) => Value::Uint(ms),
                None => Value::Null,
            },
        ),
        (
            text("details"),
            match &error.details {
                Some(bytes) => Value::Bytes(bytes.clone()),
                None => Value::Null,
            },
        ),
    ])
}

/// Decode an error object into a [`RemoteError`], preserving an unrecognized code.
pub(crate) fn decode_error(value: &Value) -> Result<RemoteError, ArgError> {
    let map = as_map(value)?;
    let wire_code = text_field(&map, "code")?;
    let message = text_field(&map, "message")?;
    let retry = crate::RetryAdvice::from_wire(&text_field(&map, "retry")?).ok_or(
        ArgError::TypeMismatch {
            expected: "retry advice",
        },
    )?;
    let retry_after_ms = opt_u64_field(&map, "retry_after_ms")?;
    let details = opt_bytes_field(&map, "details")?;
    Ok(RemoteError::from_wire(
        wire_code,
        message,
        retry,
        retry_after_ms,
        details,
    ))
}

// ---- shared map helpers --------------------------------------------------------------------------

pub(crate) fn text(value: &str) -> Value {
    Value::Text(value.to_string())
}

fn opt_bytes(value: &Option<Vec<u8>>) -> Value {
    match value {
        Some(bytes) => Value::Bytes(bytes.clone()),
        None => Value::Null,
    }
}

fn opt_text(value: &Option<String>) -> Value {
    match value {
        Some(text) => Value::Text(text.clone()),
        None => Value::Null,
    }
}

pub(crate) fn as_map(value: &Value) -> Result<Vec<(Value, Value)>, ArgError> {
    match value {
        Value::Map(entries) => Ok(entries.clone()),
        _ => Err(ArgError::TypeMismatch {
            expected: "envelope map",
        }),
    }
}

pub(crate) fn field<'a>(map: &'a [(Value, Value)], key: &str) -> Option<&'a Value> {
    map.iter().find_map(|(k, v)| match k {
        Value::Text(name) if name == key => Some(v),
        _ => None,
    })
}

fn require_protocol(map: &[(Value, Value)]) -> Result<(), ArgError> {
    match field(map, "protocol") {
        Some(Value::Text(id)) if id == PROTOCOL_ID => Ok(()),
        _ => Err(ArgError::TypeMismatch {
            expected: "loom.remote.v1 protocol",
        }),
    }
}

pub(crate) fn text_field(map: &[(Value, Value)], key: &str) -> Result<String, ArgError> {
    match field(map, key) {
        Some(Value::Text(text)) => Ok(text.clone()),
        _ => Err(ArgError::TypeMismatch { expected: "text" }),
    }
}

fn bytes_field(map: &[(Value, Value)], key: &str) -> Result<Vec<u8>, ArgError> {
    match field(map, key) {
        Some(Value::Bytes(bytes)) => Ok(bytes.clone()),
        _ => Err(ArgError::TypeMismatch { expected: "bytes" }),
    }
}

fn opt_bytes_field(map: &[(Value, Value)], key: &str) -> Result<Option<Vec<u8>>, ArgError> {
    match field(map, key) {
        Some(Value::Bytes(bytes)) => Ok(Some(bytes.clone())),
        Some(Value::Null) | None => Ok(None),
        _ => Err(ArgError::TypeMismatch {
            expected: "optional bytes",
        }),
    }
}

fn opt_text_field(map: &[(Value, Value)], key: &str) -> Result<Option<String>, ArgError> {
    match field(map, key) {
        Some(Value::Text(text)) => Ok(Some(text.clone())),
        Some(Value::Null) | None => Ok(None),
        _ => Err(ArgError::TypeMismatch {
            expected: "optional text",
        }),
    }
}

pub(crate) fn u64_field(map: &[(Value, Value)], key: &str) -> Result<u64, ArgError> {
    match field(map, key) {
        Some(Value::Uint(n)) => Ok(*n),
        _ => Err(ArgError::TypeMismatch { expected: "u64" }),
    }
}

fn opt_u64_field(map: &[(Value, Value)], key: &str) -> Result<Option<u64>, ArgError> {
    match field(map, key) {
        Some(Value::Uint(n)) => Ok(Some(*n)),
        Some(Value::Null) | None => Ok(None),
        _ => Err(ArgError::TypeMismatch {
            expected: "optional u64",
        }),
    }
}

pub(crate) fn bool_field(map: &[(Value, Value)], key: &str) -> Result<bool, ArgError> {
    match field(map, key) {
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err(ArgError::TypeMismatch { expected: "bool" }),
    }
}

fn array_field(map: &[(Value, Value)], key: &str) -> Result<Vec<Value>, ArgError> {
    match field(map, key) {
        Some(Value::Array(items)) => Ok(items.clone()),
        _ => Err(ArgError::TypeMismatch { expected: "array" }),
    }
}

fn compression_field(map: &[(Value, Value)]) -> Result<Compression, ArgError> {
    Compression::from_wire(&text_field(map, "compression")?).ok_or(ArgError::TypeMismatch {
        expected: "compression selector",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RetryAdvice;

    #[test]
    fn request_round_trips() {
        let request = Request {
            request_id: vec![1, 2, 3],
            session_id: Some(vec![9, 9]),
            interface: "Kv".to_string(),
            method: "get".to_string(),
            args: vec![Value::Text("ns".to_string()), Value::Uint(7)],
            deadline_ms: 5000,
            idempotency_key: None,
            principal_hint: Some("svc".to_string()),
            compression: Compression::Zstd,
            stream: false,
        };
        let decoded = Request::decode(&request.encode().unwrap()).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn response_ok_round_trips() {
        let response = Response::ok(vec![4, 5], None, Value::Bool(true));
        let decoded = Response::decode(&response.encode().unwrap()).unwrap();
        assert_eq!(decoded, response);
    }

    #[test]
    fn response_error_round_trips_and_preserves_code() {
        let error = RemoteError::from_wire(
            "PERMISSION_DENIED",
            "no access",
            RetryAdvice::Never,
            None,
            None,
        );
        let response = Response::err(vec![7], Some(vec![1]), error.clone());
        let decoded = Response::decode(&response.encode().unwrap()).unwrap();
        match decoded.payload {
            ResponsePayload::Err(got) => assert_eq!(got, error),
            other => panic!("expected error payload, got {other:?}"),
        }
    }

    #[test]
    fn decode_rejects_wrong_protocol() {
        let bytes = encode(&Value::Map(vec![(
            text("protocol"),
            text("other.protocol"),
        )]))
        .unwrap();
        assert!(Request::decode(&bytes).is_err());
    }
}
