//! The `RemoteLoomClient` unary call path.
//!
//! `RemoteLoomClient` builds canonical-CBOR request envelopes (deadline, idempotency key, compression,
//! and bound session metadata), sends them over the transport, and maps the response envelope back to a
//! typed value or a stable [`LoomError`]. Representative typed methods mirror the interfaces
//! the server runtime dispatches, giving local-vs-remote parity. The generated per-interface trait impls
//! layer on this same `call` seam.
//!
//! Licensed under BUSL-1.1.

use crate::connection::RemoteConnection;
use crate::transport::Transport;
use loom_codec::Value;
use loom_remote_protocol::envelope::{Compression, Request, Response, ResponsePayload};
use loom_result::result_view::ResultPayload;
use loom_types::{Code, LoomError};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-call options carried in the request envelope.
#[derive(Debug, Clone)]
pub struct CallOptions {
    /// Client deadline in milliseconds; `0` means no client deadline.
    pub deadline_ms: u64,
    /// Idempotency key for a mutating method that is not naturally idempotent.
    pub idempotency_key: Option<Vec<u8>>,
    /// Requested payload compression.
    pub compression: Compression,
}

impl Default for CallOptions {
    fn default() -> Self {
        Self {
            deadline_ms: 0,
            idempotency_key: None,
            compression: Compression::None,
        }
    }
}

/// A client bound to one discovered remote endpoint.
pub struct RemoteLoomClient<T: Transport> {
    conn: RemoteConnection<T>,
    session: Mutex<Option<Vec<u8>>>,
    next_request_id: AtomicU64,
    /// Client-local decoded result views keyed by minted handle id, for the local `ResultViews` methods:
    /// they read a buffer the client already holds and never round trip.
    pub(crate) views: Mutex<HashMap<Vec<u8>, ResultPayload>>,
    /// The next result-view handle id.
    pub(crate) next_view: AtomicU64,
    /// The most recent decode failure, for `Diagnostics.last_error`.
    pub(crate) last_error: Mutex<Vec<u8>>,
    /// Monotonic source of auto-generated idempotency keys for the §12 `key`-classified methods. Each keyed
    /// invocation mints a fresh key so a transport-level duplicate delivery cannot double-apply the call.
    next_idempotency: AtomicU64,
}

impl<T: Transport> RemoteLoomClient<T> {
    /// Wrap a discovered connection.
    pub fn new(conn: RemoteConnection<T>) -> Self {
        Self {
            conn,
            session: Mutex::new(None),
            next_request_id: AtomicU64::new(1),
            views: Mutex::new(HashMap::new()),
            next_view: AtomicU64::new(1),
            last_error: Mutex::new(Vec::new()),
            next_idempotency: AtomicU64::new(1),
        }
    }

    /// Per-call options carrying a fresh auto-generated idempotency key, used by the generated client for
    /// the §12 `key`-classified methods. Each logical invocation mints a distinct monotonically increasing
    /// key, so a transport-level duplicate delivery of that one call replays the same key (and the server
    /// dedups it), while a fresh application call mints a new key. Callers wanting durable app-level retry
    /// semantics instead pass their own key through [`RemoteLoomClient::call`] with a populated
    /// [`CallOptions::idempotency_key`].
    pub(crate) fn idempotency_options(&self) -> CallOptions {
        CallOptions {
            idempotency_key: Some(
                self.next_idempotency
                    .fetch_add(1, Ordering::Relaxed)
                    .to_be_bytes()
                    .to_vec(),
            ),
            ..CallOptions::default()
        }
    }

    /// Bind the session id every subsequent call carries.
    pub fn bind_session(&self, id: Vec<u8>) {
        *self.session.lock().expect("session") = Some(id);
    }

    /// Open a session over the carrier's dedicated session route with `auth`, bind the server-minted
    /// session id for subsequent calls, and return it (opaque bytes). This is how a real remote client
    /// obtains a session without server-side access; it does not touch the IDL surface.
    ///
    /// # Errors
    /// Returns [`LoomError`] on a transport failure, a malformed reply, or a server-side open failure
    /// (auth rejected, runtime draining).
    pub async fn open_session(
        &self,
        auth: loom_remote_protocol::session::SessionAuth,
    ) -> Result<Vec<u8>, LoomError> {
        use loom_remote_protocol::session::{
            SessionOpenReply, open_request_bytes, parse_open_reply,
        };
        let body = open_request_bytes(&auth);
        let reply_bytes = self.conn.transport().open_session(body).await?;
        match parse_open_reply(&reply_bytes)
            .map_err(|err| LoomError::new(Code::CorruptObject, format!("session reply: {err}")))?
        {
            SessionOpenReply::Ok { session_id, .. } => {
                self.bind_session(session_id.clone());
                Ok(session_id)
            }
            SessionOpenReply::Err(err) => Err(err.to_loom_error()),
        }
    }

    /// Clear the bound session.
    pub fn clear_session(&self) {
        *self.session.lock().expect("session") = None;
    }

    /// The discovered connection.
    pub fn connection(&self) -> &RemoteConnection<T> {
        &self.conn
    }

    /// The currently bound session id, if any (used by the streaming path).
    pub(crate) fn session_id(&self) -> Option<Vec<u8>> {
        self.session.lock().expect("session").clone()
    }

    /// Mint the next request correlation id (used by the streaming path).
    pub(crate) fn next_request_id(&self) -> Vec<u8> {
        self.next_request_id
            .fetch_add(1, Ordering::Relaxed)
            .to_be_bytes()
            .to_vec()
    }

    /// Send a unary call and return the decoded result value, mapping a wire error to [`LoomError`].
    ///
    /// # Errors
    /// Returns [`LoomError`] on an encode/transport/decode failure, a request-id mismatch, or a server
    /// error envelope.
    pub async fn call(
        &self,
        interface: &str,
        method: &str,
        args: Vec<Value>,
        options: &CallOptions,
    ) -> Result<Value, LoomError> {
        let request_id = self
            .next_request_id
            .fetch_add(1, Ordering::Relaxed)
            .to_be_bytes()
            .to_vec();
        let session_id = self.session.lock().expect("session").clone();
        let request = Request {
            request_id: request_id.clone(),
            session_id,
            interface: interface.to_string(),
            method: method.to_string(),
            args,
            deadline_ms: options.deadline_ms,
            idempotency_key: options.idempotency_key.clone(),
            principal_hint: None,
            compression: options.compression,
            stream: false,
        };
        let bytes = request
            .encode()
            .map_err(|err| LoomError::new(Code::Internal, format!("encode request: {err}")))?;
        let response_bytes = self.conn.transport().call(bytes).await?;
        let response = Response::decode(&response_bytes).map_err(|err| {
            LoomError::new(Code::CorruptObject, format!("decode response: {err}"))
        })?;
        if response.request_id != request_id {
            return Err(LoomError::new(
                Code::Internal,
                "response request id does not match the request",
            ));
        }
        match response.payload {
            ResponsePayload::Ok(value) => Ok(value),
            ResponsePayload::Err(remote) => Err(remote.to_loom_error()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::loopback::Loopback;
    use loom_locator::{ContextResolver, Layer};
    use loom_remote_protocol::api_types::{HandleId, LoomSession, ResultView};
    use loom_remote_protocol::discovery::{Discovery, DiscoveryMode};
    use loom_remote_protocol::generated_api::{Exec, Kv, ResultViews, Store, Workspaces};
    use loom_remote_protocol::{RemoteError, RetryAdvice};
    use loom_types::tabular::{Value as CellValue, cell_from, cell_value};

    type RecordedIdempotency = (String, Option<Vec<u8>>);
    type RecordedIdempotencies = std::sync::Arc<Mutex<Vec<RecordedIdempotency>>>;

    fn block<F: std::future::Future>(f: F) -> F::Output {
        futures::executor::block_on(f)
    }

    fn resolver() -> ContextResolver {
        ContextResolver::from_layers(&[Layer::new(
            "test",
            "[contexts.prod]\ntarget = \"https://remote.host/apps/loom\"\n",
        )])
        .unwrap()
    }

    fn discovery_bytes() -> Vec<u8> {
        Discovery::v1(
            "https://remote.host/apps/loom",
            "https://h/call",
            vec![],
            vec![],
        )
        .encode()
        .unwrap()
    }

    fn connect(transport: Loopback) -> RemoteLoomClient<Loopback> {
        let conn = block(RemoteConnection::connect(
            transport,
            "prod",
            &resolver(),
            DiscoveryMode::WellKnown,
        ))
        .expect("connect");
        RemoteLoomClient::new(conn)
    }

    fn handle() -> LoomSession {
        LoomSession(HandleId {
            kind: "session".to_string(),
            id: vec![1],
            generation: 1,
            owner_session: Vec::new(),
        })
    }

    #[test]
    fn generated_unary_stubs_encode_requests_and_decode_replies() {
        // The loopback validates the request the generated stub produced and returns a crafted reply, so
        // this exercises argument encoding and reply decoding in isolation from the server dispatch.
        let transport = Loopback::unary(
            Box::new(|_| Ok(discovery_bytes())),
            Box::new(|bytes| {
                let request = Request::decode(&bytes).unwrap();
                let value = match (request.interface.as_str(), request.method.as_str()) {
                    ("Store", "version") => {
                        assert!(request.args.is_empty());
                        Value::Text("9.9.9".to_string())
                    }
                    ("Kv", "put") => {
                        // handle, workspace, collection, key, value.
                        assert_eq!(request.args.len(), 5);
                        assert_eq!(request.args[1], Value::Text("app".to_string()));
                        assert_eq!(request.args[3], Value::Bytes(b"k".to_vec()));
                        Value::Null
                    }
                    ("Kv", "get") => {
                        assert_eq!(request.args.len(), 4);
                        Value::Bytes(b"v".to_vec())
                    }
                    ("Workspaces", "workspace_list") => {
                        Value::Array(vec![Value::Bytes(b"ns1".to_vec())])
                    }
                    other => panic!("unexpected call {other:?}"),
                };
                Response::ok(request.request_id, request.session_id, value)
                    .encode()
                    .map_err(|e| LoomError::new(Code::Internal, format!("{e}")))
            }),
        );
        let client = connect(transport);

        assert_eq!(block(client.version()).expect("version"), "9.9.9");
        block(client.put(
            handle(),
            "app".to_string(),
            "c".to_string(),
            b"k".to_vec(),
            b"v".to_vec(),
        ))
        .expect("put");
        assert_eq!(
            block(client.get(handle(), "app".to_string(), "c".to_string(), b"k".to_vec()))
                .expect("get"),
            Some(b"v".to_vec())
        );
        assert_eq!(
            block(client.workspace_list(handle())).expect("list"),
            vec![b"ns1".to_vec()]
        );
    }

    #[test]
    fn a_server_error_maps_back_to_a_stable_code() {
        let transport = Loopback::unary(
            Box::new(|_| Ok(discovery_bytes())),
            Box::new(|bytes| {
                let request = Request::decode(&bytes).unwrap();
                let err = RemoteError::from_wire(
                    "PERMISSION_DENIED",
                    "denied by policy",
                    RetryAdvice::Never,
                    None,
                    None,
                );
                Response::err(request.request_id, request.session_id, err)
                    .encode()
                    .map_err(|e| LoomError::new(Code::Internal, format!("{e}")))
            }),
        );
        let client = connect(transport);
        let err = block(client.version()).expect_err("mapped error");
        assert_eq!(err.code, Code::PermissionDenied);
        assert_eq!(err.message, "denied by policy");
    }

    #[test]
    fn local_result_view_methods_never_round_trip() {
        // A loopback that panics on any call proves the `Lo` ResultViews methods stay local.
        let transport = Loopback::unary(
            Box::new(|_| Ok(discovery_bytes())),
            Box::new(|_| panic!("a local (Lo) method must not ticket a round trip")),
        );
        let client = connect(transport);

        let row = loom_codec::encode(&Value::Array(vec![
            cell_value(&CellValue::Int(1)),
            cell_value(&CellValue::Text("a".to_string())),
        ]))
        .unwrap();
        let view: ResultView = client.row_open(row).expect("row_open");
        assert_eq!(client.result_len(view.clone()).expect("len"), 1);
        assert_eq!(
            client.result_row_len(view.clone(), 0, 0).expect("row_len"),
            2
        );
        let cell = client.result_cell(view.clone(), 0, 0, 0).expect("cell");
        assert!(matches!(
            cell_from(loom_codec::decode(&cell).unwrap()).unwrap(),
            CellValue::Int(1)
        ));

        // blob_digest is also local and stable.
        assert!(
            !client
                .blob_digest(b"abc".to_vec())
                .expect("digest")
                .0
                .is_empty()
        );

        // Closing the view frees it; a later access is NOT_FOUND.
        client.result_close(view.clone()).expect("close");
        assert_eq!(
            client.result_len(view).expect_err("closed").code,
            Code::NotFound
        );
    }

    #[test]
    fn generated_key_methods_auto_attach_an_idempotency_key() {
        // The loopback records, per method, whether the dispatched request carried a non-empty idempotency
        // key. A §12 `key`-classified method (Exec.exec_cbor) must auto-attach one; a naturally idempotent
        // method (Kv.put) must not. Each keyed call also mints a distinct key.
        let seen: RecordedIdempotencies = Default::default();
        let sink = seen.clone();
        let transport = Loopback::unary(
            Box::new(|_| Ok(discovery_bytes())),
            Box::new(move |bytes| {
                let request = Request::decode(&bytes).unwrap();
                sink.lock().unwrap().push((
                    request.method.clone(),
                    request.idempotency_key.clone().filter(|k| !k.is_empty()),
                ));
                let value = match request.method.as_str() {
                    "exec_cbor" => Value::Bytes(Vec::new()),
                    _ => Value::Null,
                };
                Response::ok(request.request_id, request.session_id, value)
                    .encode()
                    .map_err(|e| LoomError::new(Code::Internal, format!("{e}")))
            }),
        );
        let client = connect(transport);

        block(client.exec_cbor(handle(), b"req-1".to_vec())).expect("exec 1");
        block(client.exec_cbor(handle(), b"req-2".to_vec())).expect("exec 2");
        block(client.put(
            handle(),
            "app".to_string(),
            "c".to_string(),
            b"k".to_vec(),
            b"v".to_vec(),
        ))
        .expect("kv put");

        let recorded = seen.lock().unwrap().clone();
        let exec_keys: Vec<&Vec<u8>> = recorded
            .iter()
            .filter(|(m, _)| m == "exec_cbor")
            .filter_map(|(_, k)| k.as_ref())
            .collect();
        assert_eq!(exec_keys.len(), 2, "both exec_cbor calls carried a key");
        assert_ne!(
            exec_keys[0], exec_keys[1],
            "each keyed call mints a fresh key"
        );

        let put_key = recorded
            .iter()
            .find(|(m, _)| m == "put")
            .map(|(_, k)| k.clone())
            .expect("put was dispatched");
        assert!(
            put_key.is_none(),
            "Kv.put must not attach an idempotency key"
        );
    }

    #[test]
    fn document_text_binary_generated_stubs_round_trip() {
        // `Document` is scoped locally so it does not make the `Kv` `get`/`put` calls in the other
        // tests ambiguous (both interfaces define `get`/`put`).
        use loom_remote_protocol::generated_api::Document;
        // The loopback asserts the request each generated Document stub produced and returns crafted
        // replies, exercising argument encoding and reply decoding for the text/binary contract.
        let transport = Loopback::unary(
            Box::new(|_| Ok(discovery_bytes())),
            Box::new(|bytes| {
                let request = Request::decode(&bytes).unwrap();
                let value = match (request.interface.as_str(), request.method.as_str()) {
                    ("Document", "put_text") => {
                        // handle, workspace, collection, id, text, expected_entity_tag
                        assert_eq!(request.args.len(), 6);
                        assert_eq!(request.args[4], Value::Text("hello".to_string()));
                        Value::Bytes(
                            loom_codec::encode(&Value::Array(vec![
                                Value::Text("blake3:abc".to_string()),
                                Value::Text("entity-tag:abc".to_string()),
                            ]))
                            .unwrap(),
                        )
                    }
                    ("Document", "get_text") => Value::Bytes(
                        loom_codec::encode(&Value::Array(vec![
                            Value::Text("hello".to_string()),
                            Value::Text("blake3:abc".to_string()),
                            Value::Text("entity-tag:abc".to_string()),
                        ]))
                        .unwrap(),
                    ),
                    ("Document", "put_binary") => {
                        assert_eq!(request.args.len(), 6);
                        assert_eq!(request.args[4], Value::Bytes(vec![1, 2, 3]));
                        Value::Bytes(
                            loom_codec::encode(&Value::Array(vec![
                                Value::Text("blake3:def".to_string()),
                                Value::Text("entity-tag:def".to_string()),
                            ]))
                            .unwrap(),
                        )
                    }
                    ("Document", "get_binary") => Value::Bytes(
                        loom_codec::encode(&Value::Array(vec![
                            Value::Bytes(vec![1, 2, 3]),
                            Value::Text("blake3:def".to_string()),
                            Value::Text("entity-tag:def".to_string()),
                        ]))
                        .unwrap(),
                    ),
                    ("Document", "list_binary") => {
                        assert_eq!(request.args.len(), 3);
                        Value::Bytes(vec![0x80])
                    }
                    other => panic!("unexpected call {other:?}"),
                };
                Response::ok(request.request_id, request.session_id, value)
                    .encode()
                    .map_err(|e| LoomError::new(Code::Internal, format!("{e}")))
            }),
        );
        let client = connect(transport);

        let d = block(client.put_text(
            handle(),
            "app".to_string(),
            "c".to_string(),
            "d1".to_string(),
            "hello".to_string(),
            None,
        ))
        .expect("put_text");
        assert_eq!(
            loom_codec::decode(&d).unwrap(),
            Value::Array(vec![
                Value::Text("blake3:abc".to_string()),
                Value::Text("entity-tag:abc".to_string())
            ])
        );

        let text = block(client.get_text(
            handle(),
            "app".to_string(),
            "c".to_string(),
            "d1".to_string(),
        ))
        .expect("get_text")
        .expect("present");
        assert_eq!(
            loom_codec::decode(&text).unwrap(),
            Value::Array(vec![
                Value::Text("hello".to_string()),
                Value::Text("blake3:abc".to_string()),
                Value::Text("entity-tag:abc".to_string())
            ])
        );

        let db = block(client.put_binary(
            handle(),
            "app".to_string(),
            "c".to_string(),
            "d2".to_string(),
            vec![1, 2, 3],
            None,
        ))
        .expect("put_binary");
        assert_eq!(
            loom_codec::decode(&db).unwrap(),
            Value::Array(vec![
                Value::Text("blake3:def".to_string()),
                Value::Text("entity-tag:def".to_string())
            ])
        );

        let bin = block(client.get_binary(
            handle(),
            "app".to_string(),
            "c".to_string(),
            "d2".to_string(),
        ))
        .expect("get_binary")
        .expect("present");
        assert_eq!(
            loom_codec::decode(&bin).unwrap(),
            Value::Array(vec![
                Value::Bytes(vec![1, 2, 3]),
                Value::Text("blake3:def".to_string()),
                Value::Text("entity-tag:def".to_string())
            ])
        );

        let listed = block(client.list_binary(handle(), "app".to_string(), "c".to_string()))
            .expect("list_binary");
        assert_eq!(listed, vec![0x80]);
    }
}
