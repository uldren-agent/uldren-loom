//! Carrier session-open handshake.
//!
//! A remote client cannot dispatch any method until it holds a server-minted session id, but the IDL
//! surface has no session-open method (session lifecycle is owned by the runtime, not the engine trait).
//! This module defines a small, carrier-level session-open request/reply carried over a dedicated HTTP
//! route, reusing the canonical [`Request`]/[`Response`] envelopes so no new wire encoding or IDL method
//! is introduced. The client posts a [`SessionAuth`] to the session route; the server opens a runtime
//! session with the mapped auth and returns the opaque session id.
//!
//! Licensed under BUSL-1.1.

use crate::RemoteError;
use crate::codec::ArgError;
use crate::envelope::{Compression, Request, Response, ResponsePayload};
use loom_codec::Value;

/// The carrier-internal interface/method names used to frame a session-open request in a [`Request`]
/// envelope. These are not IDL methods and never reach the generated dispatch; they only tag the
/// dedicated session route's request body.
const SESSION_INTERFACE: &str = "Session";
const SESSION_METHOD: &str = "open";

/// How a client authenticates the session it opens. Mirrors the runtime's `RemoteAuth` semantics without
/// depending on the hosted crate; the server maps this onto its `RemoteAuth`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAuth {
    /// No authentication; the session runs in the store's unauthenticated-root mode.
    Unauthenticated,
    /// Authenticate `principal` (16 opaque id bytes) by passphrase.
    Passphrase {
        /// The principal id bytes (a UUID's 16 bytes).
        principal: [u8; 16],
        /// The passphrase bytes.
        passphrase: Vec<u8>,
    },
}

/// The server's reply to a session-open: the opaque session id and lease, or a stable error.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionOpenReply {
    /// A session was opened.
    Ok {
        /// The opaque server-minted session id.
        session_id: Vec<u8>,
        /// The wall-clock millisecond lease expiry.
        lease_expires_ms: u64,
    },
    /// The session could not be opened (auth failure, draining, store error).
    Err(RemoteError),
}

/// The dedicated session-open route path for a given call path: the call path's final `call` segment
/// replaced by `session` (or `<call_path>/session` when it does not end in `/call`). Both the client
/// transport and the server service derive the route the same way, so they always agree.
pub fn session_route(call_path: &str) -> String {
    match call_path.strip_suffix("/call") {
        Some(prefix) => format!("{prefix}/session"),
        None => format!("{}/session", call_path.trim_end_matches('/')),
    }
}

/// Encode `auth` as a canonical session-open request body (a [`Request`] envelope tagged for the session
/// route).
pub fn open_request_bytes(auth: &SessionAuth) -> Vec<u8> {
    let request = Request {
        request_id: Vec::new(),
        session_id: None,
        interface: SESSION_INTERFACE.to_string(),
        method: SESSION_METHOD.to_string(),
        args: vec![auth_to_value(auth)],
        deadline_ms: 0,
        idempotency_key: None,
        principal_hint: None,
        compression: Compression::None,
        stream: false,
    };
    request.encode().unwrap_or_default()
}

/// Decode a session-open request body into its [`SessionAuth`].
///
/// # Errors
/// Returns [`ArgError`] for a malformed envelope, a non-session request, or a malformed auth value.
pub fn parse_open_request(bytes: &[u8]) -> Result<SessionAuth, ArgError> {
    let request = Request::decode(bytes)?;
    if request.interface != SESSION_INTERFACE || request.method != SESSION_METHOD {
        return Err(ArgError::TypeMismatch {
            expected: "session-open request",
        });
    }
    let auth_value = request.args.first().ok_or(ArgError::TypeMismatch {
        expected: "session auth argument",
    })?;
    auth_from_value(auth_value)
}

/// Encode a [`SessionOpenReply`] as a canonical response body (a [`Response`] envelope).
pub fn open_reply_bytes(reply: &SessionOpenReply) -> Vec<u8> {
    let response = match reply {
        SessionOpenReply::Ok {
            session_id,
            lease_expires_ms,
        } => Response::ok(
            Vec::new(),
            None,
            Value::Array(vec![
                Value::Bytes(session_id.clone()),
                Value::Uint(*lease_expires_ms),
            ]),
        ),
        SessionOpenReply::Err(error) => Response::err(Vec::new(), None, error.clone()),
    };
    response.encode().unwrap_or_default()
}

/// Decode a session-open reply body.
///
/// # Errors
/// Returns [`ArgError`] for a malformed envelope or a malformed success value.
pub fn parse_open_reply(bytes: &[u8]) -> Result<SessionOpenReply, ArgError> {
    let response = Response::decode(bytes)?;
    match response.payload {
        ResponsePayload::Ok(Value::Array(items)) if items.len() == 2 => {
            let session_id = match &items[0] {
                Value::Bytes(bytes) => bytes.clone(),
                _ => {
                    return Err(ArgError::TypeMismatch {
                        expected: "session id bytes",
                    });
                }
            };
            let lease_expires_ms = match &items[1] {
                Value::Uint(value) => *value,
                _ => {
                    return Err(ArgError::TypeMismatch {
                        expected: "lease expiry uint",
                    });
                }
            };
            Ok(SessionOpenReply::Ok {
                session_id,
                lease_expires_ms,
            })
        }
        ResponsePayload::Ok(_) => Err(ArgError::TypeMismatch {
            expected: "session-open success value",
        }),
        ResponsePayload::Err(error) => Ok(SessionOpenReply::Err(error)),
    }
}

fn auth_to_value(auth: &SessionAuth) -> Value {
    match auth {
        SessionAuth::Unauthenticated => Value::Null,
        SessionAuth::Passphrase {
            principal,
            passphrase,
        } => Value::Array(vec![
            Value::Bytes(principal.to_vec()),
            Value::Bytes(passphrase.clone()),
        ]),
    }
}

fn auth_from_value(value: &Value) -> Result<SessionAuth, ArgError> {
    match value {
        Value::Null => Ok(SessionAuth::Unauthenticated),
        Value::Array(items) if items.len() == 2 => {
            let principal_bytes = match &items[0] {
                Value::Bytes(bytes) => bytes,
                _ => {
                    return Err(ArgError::TypeMismatch {
                        expected: "principal bytes",
                    });
                }
            };
            let principal: [u8; 16] =
                principal_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| ArgError::TypeMismatch {
                        expected: "16-byte principal id",
                    })?;
            let passphrase = match &items[1] {
                Value::Bytes(bytes) => bytes.clone(),
                _ => {
                    return Err(ArgError::TypeMismatch {
                        expected: "passphrase bytes",
                    });
                }
            };
            Ok(SessionAuth::Passphrase {
                principal,
                passphrase,
            })
        }
        _ => Err(ArgError::TypeMismatch {
            expected: "session auth (null or [principal, passphrase])",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RetryAdvice;

    #[test]
    fn session_route_derives_from_call_path() {
        assert_eq!(session_route("/apps/loom/v1/call"), "/apps/loom/v1/session");
        assert_eq!(session_route("/x/"), "/x/session");
        assert_eq!(session_route("/x"), "/x/session");
    }

    #[test]
    fn unauthenticated_request_round_trips() {
        let bytes = open_request_bytes(&SessionAuth::Unauthenticated);
        assert_eq!(
            parse_open_request(&bytes).unwrap(),
            SessionAuth::Unauthenticated
        );
    }

    #[test]
    fn passphrase_request_round_trips() {
        let auth = SessionAuth::Passphrase {
            principal: [7u8; 16],
            passphrase: b"s3cret".to_vec(),
        };
        let bytes = open_request_bytes(&auth);
        assert_eq!(parse_open_request(&bytes).unwrap(), auth);
    }

    #[test]
    fn ok_reply_round_trips() {
        let reply = SessionOpenReply::Ok {
            session_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
            lease_expires_ms: 70_000,
        };
        let bytes = open_reply_bytes(&reply);
        assert_eq!(parse_open_reply(&bytes).unwrap(), reply);
    }

    #[test]
    fn err_reply_round_trips() {
        let error = RemoteError::from_wire(
            "PERMISSION_DENIED",
            "bad passphrase",
            RetryAdvice::Never,
            None,
            None,
        );
        let bytes = open_reply_bytes(&SessionOpenReply::Err(error.clone()));
        match parse_open_reply(&bytes).unwrap() {
            SessionOpenReply::Err(got) => assert_eq!(got.wire_code, error.wire_code),
            other => panic!("expected error reply, got {other:?}"),
        }
    }
}
