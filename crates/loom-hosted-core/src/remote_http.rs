//! HTTP carrier semantics for the remote runtime (the server side of the HTTP/2-over-TLS carrier).
//!
//! [`RemoteHttpService`] maps an HTTP method, path, and body onto a [`RemoteRuntime`] discovery lookup or
//! unary dispatch, independent of the socket. Application errors ride the response envelope at HTTP 200
//!; only transport-level failures (a malformed envelope, an unknown route) map to a non-200
//! status. The concrete hyper listener and TLS terminator are a thin adapter that owns the socket and
//! calls [`RemoteHttpService::handle`]; this module holds all the routing and status semantics so they
//! are testable without a network.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use crate::remote::{RemoteRuntime, ServerFrameStream};
use loom_codec::{Value, encode};
use loom_remote_protocol::envelope::Request;
use std::sync::Arc;

/// The canonical CBOR content type the carrier uses for envelopes and documents.
pub const CBOR_CONTENT_TYPE: &str = "application/cbor";

/// A carrier-independent HTTP response: status, content type, and body bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The response content type.
    pub content_type: &'static str,
    /// The response body.
    pub body: Vec<u8>,
}

impl HttpResponse {
    fn ok(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type: CBOR_CONTENT_TYPE,
            body,
        }
    }

    fn status_only(status: u16) -> Self {
        Self {
            status,
            content_type: CBOR_CONTENT_TYPE,
            body: Vec::new(),
        }
    }
}

/// The HTTP request router for one served remote endpoint. It owns a shared [`RemoteRuntime`] and the
/// configured call path; the socket/TLS listener adapts raw HTTP onto [`RemoteHttpService::handle`].
pub struct RemoteHttpService {
    runtime: Arc<RemoteRuntime>,
    call_path: String,
    session_path: String,
    health_path: String,
}

impl RemoteHttpService {
    /// Build a service over `runtime`, routing unary calls at `call_path`, session-open at the derived
    /// session route (`call_path`'s final `call` segment replaced by `session`), and health at `/healthz`.
    pub fn new(runtime: Arc<RemoteRuntime>, call_path: impl Into<String>) -> Self {
        let call_path = call_path.into();
        let session_path = loom_remote_protocol::session::session_route(&call_path);
        Self {
            runtime,
            call_path,
            session_path,
            health_path: "/healthz".to_string(),
        }
    }

    /// The shared runtime this service dispatches into.
    pub fn runtime(&self) -> &Arc<RemoteRuntime> {
        &self.runtime
    }

    /// Route one HTTP request. `GET` serves discovery (credential-free) and health; `POST` at the call
    /// path decodes a request envelope and dispatches it. A malformed envelope is `400`, an unknown route
    /// is `404`, and an envelope-encode failure is `500`; every application outcome (including engine
    /// errors) is `200` with the outcome in the response envelope.
    pub fn handle(&self, method: &str, path: &str, body: &[u8]) -> HttpResponse {
        match method {
            "GET" => {
                if path == self.health_path {
                    return self.health();
                }
                match self.runtime.discovery_response(path) {
                    Some(document) => HttpResponse::ok(document),
                    None => HttpResponse::status_only(404),
                }
            }
            "POST" if path == self.session_path => self.open_session_route(body),
            "POST" if path == self.call_path => match Request::decode(body) {
                Ok(request) if request.stream => {
                    HttpResponse::ok(encode_frames(&self.runtime.dispatch_stream(&request)))
                }
                Ok(request) => match self.runtime.dispatch(&request).encode() {
                    Ok(bytes) => HttpResponse::ok(bytes),
                    Err(_) => HttpResponse::status_only(500),
                },
                Err(_) => HttpResponse::status_only(400),
            },
            _ => HttpResponse::status_only(404),
        }
    }

    /// Handle a session-open request at the session route: decode the auth, open a runtime session on a
    /// freshly registered connection, and return the canonical session-open reply (an opaque session id
    /// and lease, or a stable error). A malformed body is a transport-level `400`; an auth/runtime failure
    /// rides the reply envelope at `200`, like the call route's application errors.
    fn open_session_route(&self, body: &[u8]) -> HttpResponse {
        use loom_remote_protocol::session::{
            SessionOpenReply, open_reply_bytes, parse_open_request,
        };
        let auth = match parse_open_request(body) {
            Ok(auth) => auth,
            Err(_) => return HttpResponse::status_only(400),
        };
        let connection = self.runtime.register_connection("remote-http-session");
        let reply = match self
            .runtime
            .open_session(connection, map_session_auth(auth))
        {
            Ok(session) => SessionOpenReply::Ok {
                session_id: session.id,
                lease_expires_ms: session.lease_expires_ms,
            },
            Err(err) => {
                SessionOpenReply::Err(loom_remote_protocol::RemoteError::from_loom_error(&err))
            }
        };
        HttpResponse::ok(open_reply_bytes(&reply))
    }

    /// Open an incremental server frame stream when `(method, path, body)` is a streaming call at the call
    /// path (a decoded request envelope with `stream = true`); otherwise `None`, so the caller serves the
    /// request through the unary [`RemoteHttpService::handle`] path. The concrete carrier uses this to
    /// stream frames over the response body instead of buffering them.
    pub fn open_stream(&self, method: &str, path: &str, body: &[u8]) -> Option<ServerFrameStream> {
        if method == "POST"
            && path == self.call_path
            && let Ok(request) = Request::decode(body)
            && request.stream
        {
            return Some(self.runtime.open_frame_stream(&request));
        }
        None
    }

    fn health(&self) -> HttpResponse {
        let (draining, sessions) = self.runtime.health();
        let document = encode(&Value::Map(vec![
            (Value::Text("draining".to_string()), Value::Bool(draining)),
            (
                Value::Text("sessions".to_string()),
                Value::Uint(sessions as u64),
            ),
        ]))
        .unwrap_or_default();
        HttpResponse::ok(document)
    }
}

/// Map a protocol [`SessionAuth`](loom_remote_protocol::session::SessionAuth) onto the runtime's
/// `RemoteAuth`, reusing the existing session/auth semantics (no faked auth).
fn map_session_auth(auth: loom_remote_protocol::session::SessionAuth) -> crate::remote::RemoteAuth {
    use loom_remote_protocol::session::SessionAuth;
    match auth {
        SessionAuth::Unauthenticated => crate::remote::RemoteAuth::Unauthenticated,
        SessionAuth::Passphrase {
            principal,
            passphrase,
        } => crate::remote::RemoteAuth::Passphrase {
            principal: loom_core::WorkspaceId::from_bytes(principal),
            passphrase,
        },
    }
}

/// Encode a batch of stream frames as one HTTP body: a canonical CBOR array of frame byte-strings. The
/// client decodes it back into the frame sequence.
fn encode_frames(frames: &[Vec<u8>]) -> Vec<u8> {
    let array = Value::Array(frames.iter().map(|f| Value::Bytes(f.clone())).collect());
    encode(&array).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::{RemoteAuth, RemoteAuthMode, RemoteServerConfig, RemoteTlsTrust};
    use loom_client::LocalLoomClient;
    use loom_codec::Value;
    use loom_remote_protocol::discovery::{Discovery, DiscoveryMode, DiscoveryRoutes};
    use loom_remote_protocol::envelope::{Compression, Request, Response, ResponsePayload};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_store(tag: &str) -> std::path::PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-remote-http-{tag}-{}-{}.loom",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::remove_dir_all(&path).ok();
        LocalLoomClient::new(&path).create().expect("create store");
        path
    }

    fn config() -> RemoteServerConfig {
        RemoteServerConfig {
            service_root: "https://remote.host/apps/loom".to_string(),
            call_endpoint: "https://remote.host/apps/loom/v1/call".to_string(),
            auth_modes: vec![RemoteAuthMode::Interactive],
            tls: vec![RemoteTlsTrust::System],
            discovery: DiscoveryRoutes {
                mode: DiscoveryMode::Default,
                service_root_path: "/apps/loom".to_string(),
                custom_path: None,
            },
            session_lease_ms: 60_000,
        }
    }

    fn service(path: &std::path::Path) -> (RemoteHttpService, Vec<u8>) {
        let runtime = Arc::new(RemoteRuntime::start(path, config()).expect("start"));
        let connection = runtime.register_connection("http");
        let session = runtime
            .open_session(connection, RemoteAuth::Unauthenticated)
            .expect("session");
        (
            RemoteHttpService::new(runtime, "/apps/loom/v1/call"),
            session.id,
        )
    }

    #[test]
    fn get_serves_discovery_and_health_and_404s_unknown() {
        let path = temp_store("disco");
        let (svc, _) = service(&path);
        let disco = svc.handle("GET", "/apps/loom/.well-known/loom", &[]);
        assert_eq!(disco.status, 200);
        assert!(Discovery::decode(&disco.body).unwrap().is_compatible(1, 1));

        let health = svc.handle("GET", "/healthz", &[]);
        assert_eq!(health.status, 200);

        assert_eq!(svc.handle("GET", "/nope", &[]).status, 404);
        svc.runtime().shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    #[test]
    fn post_dispatches_a_unary_call_and_rejects_a_bad_envelope() {
        let path = temp_store("call");
        let (svc, session) = service(&path);

        let request = Request {
            request_id: vec![1],
            session_id: Some(session),
            interface: "Store".to_string(),
            method: "version".to_string(),
            args: Vec::new(),
            deadline_ms: 0,
            idempotency_key: None,
            principal_hint: None,
            compression: Compression::None,
            stream: false,
        };
        let response = svc.handle("POST", "/apps/loom/v1/call", &request.encode().unwrap());
        assert_eq!(response.status, 200);
        match Response::decode(&response.body).unwrap().payload {
            ResponsePayload::Ok(Value::Text(_)) => {}
            other => panic!("expected version text, got {other:?}"),
        }

        // A body that is not a request envelope is a transport-level 400.
        assert_eq!(
            svc.handle("POST", "/apps/loom/v1/call", b"not-cbor").status,
            400
        );
        // The wrong path is a 404.
        assert_eq!(svc.handle("POST", "/elsewhere", &[]).status, 404);
        svc.runtime().shutdown();
        std::fs::remove_dir_all(&path).ok();
    }

    // Unsupported HTTP policy: only `GET` (discovery/health) and `POST` (session open / call) are served;
    // any other method, and any method/path mismatch, is rejected rather than dispatched. This is the
    // transport-policy guard complementing the bad-envelope (400) case above.
    #[test]
    fn unsupported_http_methods_and_method_path_mismatch_are_rejected() {
        let path = temp_store("http-policy");
        let (svc, _session) = service(&path);
        let call = "/apps/loom/v1/call";

        // Unsupported HTTP methods on the call path never dispatch a call.
        for method in ["PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE"] {
            assert_eq!(
                svc.handle(method, call, &[]).status,
                404,
                "{method} on the call path is rejected"
            );
        }
        // Method/path mismatch: `GET` on the call path is not a dispatch (GET only serves discovery/health),
        // and `POST` to the GET-only discovery route is rejected.
        assert_eq!(
            svc.handle("GET", call, &[]).status,
            404,
            "GET on the call path is not a dispatch"
        );
        assert_eq!(
            svc.handle("POST", "/apps/loom/.well-known/loom", &[])
                .status,
            404,
            "POST to the discovery route is rejected"
        );

        svc.runtime().shutdown();
        std::fs::remove_dir_all(&path).ok();
    }
}
