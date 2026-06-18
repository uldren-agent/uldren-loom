//! Client-side HTTP carrier mapping (the client half of the HTTP/2-over-TLS carrier).
//!
//! These helpers translate a protocol envelope into the HTTP request parts a carrier sends, and map an
//! HTTP status and body back into envelope bytes or a stable transport error. They are transport-agnostic
//! and engine-free: a concrete hyper/TLS `Transport` builds its request from [`call_request`] /
//! [`discovery_request`] and feeds the response through [`parse_response`]. Application errors ride the
//! envelope at HTTP 200; only transport-level failures map to a non-200 status here.
//!
//! Licensed under BUSL-1.1.

use loom_codec::{Value, decode};
use loom_types::{Code, LoomError};

/// The canonical CBOR content type the carrier uses.
pub const CBOR_CONTENT_TYPE: &str = "application/cbor";

/// The HTTP request a carrier should send for one protocol operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequestParts {
    /// The HTTP method.
    pub method: &'static str,
    /// The absolute request path.
    pub path: String,
    /// The request body (empty for discovery GETs).
    pub body: Vec<u8>,
    /// The body content type.
    pub content_type: &'static str,
}

/// The HTTP request parts for a discovery GET at `path`.
pub fn discovery_request(path: &str) -> HttpRequestParts {
    HttpRequestParts {
        method: "GET",
        path: path.to_string(),
        body: Vec::new(),
        content_type: CBOR_CONTENT_TYPE,
    }
}

/// The HTTP request parts for a unary call POST of `envelope` to `call_path`.
pub fn call_request(call_path: &str, envelope: Vec<u8>) -> HttpRequestParts {
    HttpRequestParts {
        method: "POST",
        path: call_path.to_string(),
        body: envelope,
        content_type: CBOR_CONTENT_TYPE,
    }
}

/// Map an HTTP `status` and `body` back to envelope/document bytes, or a stable transport error. A `200`
/// yields the body (the response or discovery envelope, whose own `ok` flag carries application success);
/// other statuses map to the closest stable [`Code`].
///
/// # Errors
/// Returns [`LoomError`] for any non-200 status.
pub fn parse_response(status: u16, body: Vec<u8>) -> Result<Vec<u8>, LoomError> {
    match status {
        200 => Ok(body),
        400 => Err(LoomError::new(
            Code::InvalidArgument,
            "remote rejected the request envelope",
        )),
        401 | 403 => Err(LoomError::new(
            Code::PermissionDenied,
            "remote denied the request",
        )),
        404 => Err(LoomError::new(Code::NotFound, "remote route not found")),
        429 => Err(LoomError::new(
            Code::ResourceExhausted,
            "remote is rate limiting",
        )),
        500..=599 => Err(LoomError::new(
            Code::Internal,
            format!("remote server error (status {status})"),
        )),
        other => Err(LoomError::new(
            Code::Io,
            format!("unexpected remote status {other}"),
        )),
    }
}

/// Map a streaming HTTP response into the ordered encoded frames the server emitted. A `200` body is a
/// canonical CBOR array of frame byte-strings (the form the server carrier produces); other statuses map
/// to a stable transport error.
///
/// # Errors
/// Returns [`LoomError`] for a non-200 status or a body that is not a CBOR array of byte-strings.
pub fn parse_stream_response(status: u16, body: Vec<u8>) -> Result<Vec<Vec<u8>>, LoomError> {
    let body = parse_response(status, body)?;
    match decode(&body)
        .map_err(|err| LoomError::new(Code::CorruptObject, format!("stream body decode: {err}")))?
    {
        Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                Value::Bytes(bytes) => Ok(bytes),
                _ => Err(LoomError::new(
                    Code::CorruptObject,
                    "stream frame must be a byte string",
                )),
            })
            .collect(),
        _ => Err(LoomError::new(
            Code::CorruptObject,
            "stream body must be a frame array",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_parts_are_shaped_per_operation() {
        let disco = discovery_request("/.well-known/loom");
        assert_eq!(disco.method, "GET");
        assert!(disco.body.is_empty());

        let call = call_request("/v1/call", vec![1, 2, 3]);
        assert_eq!(call.method, "POST");
        assert_eq!(call.path, "/v1/call");
        assert_eq!(call.body, vec![1, 2, 3]);
        assert_eq!(call.content_type, CBOR_CONTENT_TYPE);
    }

    #[test]
    fn status_maps_to_stable_codes() {
        assert_eq!(parse_response(200, vec![9]).unwrap(), vec![9]);
        assert_eq!(
            parse_response(400, vec![]).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            parse_response(403, vec![]).unwrap_err().code,
            Code::PermissionDenied
        );
        assert_eq!(
            parse_response(404, vec![]).unwrap_err().code,
            Code::NotFound
        );
        assert_eq!(
            parse_response(503, vec![]).unwrap_err().code,
            Code::Internal
        );
        assert_eq!(parse_response(418, vec![]).unwrap_err().code, Code::Io);
    }
}
