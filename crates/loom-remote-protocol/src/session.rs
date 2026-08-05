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
use loom_codec::{Value, decode, encode};

/// The carrier-internal interface/method names used to frame a session-open request in a [`Request`]
/// envelope. These are not IDL methods and never reach the generated dispatch; they only tag the
/// dedicated session route's request body.
const SESSION_INTERFACE: &str = "Session";
const SESSION_METHOD: &str = "open";
const SESSION_RESUME_METHOD: &str = "resume";
const SESSION_CLOSE_METHOD: &str = "close";

/// Binding claims carried inside a coordinator-authenticated opaque credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCredentialClaims {
    pub session_id: Vec<u8>,
    pub resume_secret: [u8; 32],
    pub principal: Option<[u8; 16]>,
    pub store_identity: String,
    pub coordinator_identity: [u8; 32],
    pub authority_epoch: u64,
    pub protocol_profile: String,
    pub lease_expires_ms: u64,
}

/// A typed carrier lifecycle request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRequest {
    Create(SessionAuth),
    Resume {
        auth: SessionAuth,
        credential: Vec<u8>,
    },
    Close {
        auth: SessionAuth,
        credential: Vec<u8>,
    },
}

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
        /// Coordinator-issued credential used to resume this logical session.
        credential: Option<Vec<u8>>,
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
    request_bytes(SESSION_METHOD, vec![auth_to_value(auth)])
}

/// Encode a logical-session resume request.
pub fn resume_request_bytes(auth: &SessionAuth, credential: &[u8]) -> Vec<u8> {
    request_bytes(
        SESSION_RESUME_METHOD,
        vec![auth_to_value(auth), Value::Bytes(credential.to_vec())],
    )
}

/// Encode an explicit logical-session close request.
pub fn close_request_bytes(auth: &SessionAuth, credential: &[u8]) -> Vec<u8> {
    request_bytes(
        SESSION_CLOSE_METHOD,
        vec![auth_to_value(auth), Value::Bytes(credential.to_vec())],
    )
}

fn request_bytes(method: &str, args: Vec<Value>) -> Vec<u8> {
    let request = Request {
        request_id: Vec::new(),
        session_id: None,
        interface: SESSION_INTERFACE.to_string(),
        method: method.to_string(),
        args,
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
    match parse_session_request(bytes)? {
        SessionRequest::Create(auth) => Ok(auth),
        _ => Err(ArgError::TypeMismatch {
            expected: "session-open request",
        }),
    }
}

/// Decode a typed carrier lifecycle request.
pub fn parse_session_request(bytes: &[u8]) -> Result<SessionRequest, ArgError> {
    let request = Request::decode(bytes)?;
    if request.interface != SESSION_INTERFACE {
        return Err(ArgError::TypeMismatch {
            expected: "session lifecycle request",
        });
    }
    let auth_value = request.args.first().ok_or(ArgError::TypeMismatch {
        expected: "session auth argument",
    })?;
    let auth = auth_from_value(auth_value)?;
    match request.method.as_str() {
        SESSION_METHOD if request.args.len() == 1 => Ok(SessionRequest::Create(auth)),
        SESSION_RESUME_METHOD | SESSION_CLOSE_METHOD if request.args.len() == 2 => {
            let credential = match &request.args[1] {
                Value::Bytes(bytes) => bytes.clone(),
                _ => {
                    return Err(ArgError::TypeMismatch {
                        expected: "session credential bytes",
                    });
                }
            };
            if request.method == SESSION_RESUME_METHOD {
                Ok(SessionRequest::Resume { auth, credential })
            } else {
                Ok(SessionRequest::Close { auth, credential })
            }
        }
        _ => Err(ArgError::TypeMismatch {
            expected: "session lifecycle request",
        }),
    }
}

/// Encode a [`SessionOpenReply`] as a canonical response body (a [`Response`] envelope).
pub fn open_reply_bytes(reply: &SessionOpenReply) -> Vec<u8> {
    let response = match reply {
        SessionOpenReply::Ok {
            session_id,
            lease_expires_ms,
            credential,
        } => Response::ok(
            Vec::new(),
            None,
            Value::Array(vec![
                Value::Bytes(session_id.clone()),
                Value::Uint(*lease_expires_ms),
                credential
                    .as_ref()
                    .map_or(Value::Null, |bytes| Value::Bytes(bytes.clone())),
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
        ResponsePayload::Ok(Value::Array(items)) if items.len() == 2 || items.len() == 3 => {
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
            let credential = match items.get(2) {
                Some(Value::Bytes(bytes)) => Some(bytes.clone()),
                Some(Value::Null) | None => None,
                Some(_) => {
                    return Err(ArgError::TypeMismatch {
                        expected: "session credential bytes or null",
                    });
                }
            };
            Ok(SessionOpenReply::Ok {
                session_id,
                lease_expires_ms,
                credential,
            })
        }
        ResponsePayload::Ok(_) => Err(ArgError::TypeMismatch {
            expected: "session-open success value",
        }),
        ResponsePayload::Err(error) => Ok(SessionOpenReply::Err(error)),
    }
}

/// Issue an authenticated opaque credential for `claims`.
pub fn seal_credential(
    claims: &SessionCredentialClaims,
    signing_key: &[u8; 32],
) -> Result<Vec<u8>, loom_codec::CodecError> {
    let claims_value = claims_to_value(claims);
    let claims_bytes = encode(&claims_value)?;
    let mac = blake3::keyed_hash(signing_key, &claims_bytes);
    encode(&Value::Array(vec![
        claims_value,
        Value::Bytes(mac.as_bytes().to_vec()),
    ]))
}

/// Verify and decode a coordinator-authenticated opaque credential.
pub fn open_credential(
    credential: &[u8],
    signing_key: &[u8; 32],
) -> Result<SessionCredentialClaims, ArgError> {
    let value = decode(credential)?;
    let Value::Array(items) = value else {
        return Err(ArgError::TypeMismatch {
            expected: "session credential",
        });
    };
    if items.len() != 2 {
        return Err(ArgError::TypeMismatch {
            expected: "session credential",
        });
    }
    let claims_bytes = encode(&items[0])?;
    let mac = match &items[1] {
        Value::Bytes(bytes) if bytes.len() == 32 => bytes,
        _ => {
            return Err(ArgError::TypeMismatch {
                expected: "session credential authenticator",
            });
        }
    };
    if blake3::keyed_hash(signing_key, &claims_bytes).as_bytes() != mac.as_slice() {
        return Err(ArgError::TypeMismatch {
            expected: "valid session credential authenticator",
        });
    }
    let claims = claims_from_value(&items[0])?;
    if seal_credential(&claims, signing_key)? != credential {
        return Err(ArgError::TypeMismatch {
            expected: "canonical session credential",
        });
    }
    Ok(claims)
}

fn claims_to_value(claims: &SessionCredentialClaims) -> Value {
    Value::Array(vec![
        Value::Bytes(claims.session_id.clone()),
        Value::Bytes(claims.resume_secret.to_vec()),
        claims
            .principal
            .map_or(Value::Null, |principal| Value::Bytes(principal.to_vec())),
        Value::Text(claims.store_identity.clone()),
        Value::Bytes(claims.coordinator_identity.to_vec()),
        Value::Uint(claims.authority_epoch),
        Value::Text(claims.protocol_profile.clone()),
        Value::Uint(claims.lease_expires_ms),
    ])
}

fn claims_from_value(value: &Value) -> Result<SessionCredentialClaims, ArgError> {
    let Value::Array(items) = value else {
        return Err(ArgError::TypeMismatch {
            expected: "session credential claims",
        });
    };
    if items.len() != 8 {
        return Err(ArgError::TypeMismatch {
            expected: "session credential claims",
        });
    }
    let bytes = |index: usize, expected| match &items[index] {
        Value::Bytes(bytes) => Ok(bytes.clone()),
        _ => Err(ArgError::TypeMismatch { expected }),
    };
    let session_id = bytes(0, "session id bytes")?;
    let resume_secret: [u8; 32] =
        bytes(1, "32-byte resume secret")?
            .try_into()
            .map_err(|_| ArgError::TypeMismatch {
                expected: "32-byte resume secret",
            })?;
    let principal = match &items[2] {
        Value::Null => None,
        Value::Bytes(bytes) => {
            Some(
                bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| ArgError::TypeMismatch {
                        expected: "16-byte principal id",
                    })?,
            )
        }
        _ => {
            return Err(ArgError::TypeMismatch {
                expected: "optional principal id",
            });
        }
    };
    let text = |index: usize, expected| match &items[index] {
        Value::Text(value) => Ok(value.clone()),
        _ => Err(ArgError::TypeMismatch { expected }),
    };
    let coordinator_identity: [u8; 32] = bytes(4, "32-byte coordinator identity")?
        .try_into()
        .map_err(|_| ArgError::TypeMismatch {
            expected: "32-byte coordinator identity",
        })?;
    let uint = |index: usize, expected| match items[index] {
        Value::Uint(value) => Ok(value),
        _ => Err(ArgError::TypeMismatch { expected }),
    };
    Ok(SessionCredentialClaims {
        session_id,
        resume_secret,
        principal,
        store_identity: text(3, "store identity text")?,
        coordinator_identity,
        authority_epoch: uint(5, "authority epoch")?,
        protocol_profile: text(6, "protocol profile text")?,
        lease_expires_ms: uint(7, "lease expiry")?,
    })
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
            credential: Some(vec![9, 8, 7]),
        };
        let bytes = open_reply_bytes(&reply);
        assert_eq!(parse_open_reply(&bytes).unwrap(), reply);
    }

    #[test]
    fn malformed_logical_credential_reply_is_rejected() {
        let bytes = Response::ok(
            Vec::new(),
            None,
            Value::Array(vec![
                Value::Bytes(vec![1]),
                Value::Uint(2),
                Value::Bool(true),
            ]),
        )
        .encode()
        .unwrap();
        assert!(parse_open_reply(&bytes).is_err());
    }

    #[test]
    fn resume_and_close_requests_round_trip() {
        let auth = SessionAuth::Unauthenticated;
        let credential = vec![1, 2, 3];
        assert_eq!(
            parse_session_request(&resume_request_bytes(&auth, &credential)).unwrap(),
            SessionRequest::Resume {
                auth: auth.clone(),
                credential: credential.clone(),
            }
        );
        assert_eq!(
            parse_session_request(&close_request_bytes(&auth, &credential)).unwrap(),
            SessionRequest::Close { auth, credential }
        );
    }

    #[test]
    fn credential_is_canonical_and_authenticated() {
        let claims = SessionCredentialClaims {
            session_id: vec![1, 2, 3],
            resume_secret: [4; 32],
            principal: Some([5; 16]),
            store_identity: "store".to_string(),
            coordinator_identity: [6; 32],
            authority_epoch: 1,
            protocol_profile: crate::envelope::PROTOCOL_ID.to_string(),
            lease_expires_ms: 42,
        };
        let key = [7; 32];
        let encoded = seal_credential(&claims, &key).unwrap();
        assert_eq!(open_credential(&encoded, &key).unwrap(), claims);
        let mut damaged = encoded;
        let last = damaged.len() - 1;
        damaged[last] ^= 1;
        assert!(open_credential(&damaged, &key).is_err());
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
