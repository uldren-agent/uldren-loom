//! Client streaming, remote handles, tasks, and result views.
//!
//! A [`RemoteStream`] consumes the ordered frames a server emits for a row, watch, byte-chunk, or task
//! stream, surfacing items until a terminal `complete`, `error`, or `cancel` frame. A
//! [`HandleTracker`] owns the client-side view of remote handle ids and frees them. Task, batch, and
//! result-view access ride the same unary `call` seam. Result views decode a canonical-CBOR buffer the
//! client already holds, so they never ticket a round trip.
//!
//! Licensed under BUSL-1.1.

use crate::client::RemoteLoomClient;
use crate::transport::{FrameSource, Transport};
use loom_codec::Value;
use loom_remote_protocol::codec::RemoteHandleId;
use loom_remote_protocol::frame::Frame;
use loom_types::{Code, LoomError};
use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A client-side view of a server stream: it pulls encoded frames from an incremental [`FrameSource`] and
/// surfaces items until a terminal `complete`/`cancel` (end) or `error` frame. Because it pulls one frame
/// at a time, the client never buffers the whole (possibly unbounded) stream, and pulling slowly bounds
/// the memory the pipeline holds; dropping it closes the underlying transport stream.
pub struct RemoteStream {
    source: FrameSource,
    closed: bool,
    trailer: Option<Vec<u8>>,
}

impl RemoteStream {
    /// Build a stream over an incremental frame source.
    pub fn from_source(source: FrameSource) -> Self {
        Self {
            source,
            closed: false,
            trailer: None,
        }
    }

    /// Build a stream from a fixed set of already-collected encoded frames (the in-process / loopback
    /// path, where all frames are produced synchronously).
    pub fn from_encoded(encoded: Vec<Vec<u8>>) -> Self {
        Self::from_source(FrameSource::from_frames(encoded))
    }

    /// Poll for the next item, consuming and interpreting control frames from the source. `Ready(None)`
    /// at normal end of stream; `Ready(Some(Err(..)))` for a terminal `error` frame or a transport
    /// failure; `Pending` when the source has no frame ready yet.
    fn poll_item(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Vec<u8>, LoomError>>> {
        if self.closed {
            return Poll::Ready(None);
        }
        loop {
            match self.source.poll_frame(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    self.closed = true;
                    return Poll::Ready(None);
                }
                Poll::Ready(Some(Err(err))) => {
                    self.closed = true;
                    return Poll::Ready(Some(Err(err)));
                }
                Poll::Ready(Some(Ok(bytes))) => {
                    let frame = match Frame::decode(&bytes) {
                        Ok(frame) => frame,
                        Err(err) => {
                            self.closed = true;
                            return Poll::Ready(Some(Err(LoomError::new(
                                Code::CorruptObject,
                                format!("frame decode: {err}"),
                            ))));
                        }
                    };
                    match frame {
                        Frame::Item(bytes) => return Poll::Ready(Some(Ok(bytes))),
                        Frame::Trailer(bytes) => self.trailer = Some(bytes),
                        Frame::Open { .. } | Frame::Credit(_) => {}
                        Frame::Complete | Frame::Cancel => {
                            self.closed = true;
                            return Poll::Ready(None);
                        }
                        Frame::Error(err) => {
                            self.closed = true;
                            return Poll::Ready(Some(Err(err.to_loom_error())));
                        }
                    }
                }
            }
        }
    }

    /// Pull the next item, awaiting the source. Returns `Ok(None)` at normal end of stream and an error at
    /// a terminal error frame or a mid-stream transport failure.
    ///
    /// # Errors
    /// Returns [`LoomError`] carried by a terminal `error` frame or a transport failure.
    pub fn next_item(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send + '_ {
        std::future::poll_fn(move |cx| self.poll_item(cx).map(|item| item.transpose()))
    }

    /// The trailer metadata, once observed.
    pub fn trailer(&self) -> Option<&[u8]> {
        self.trailer.as_deref()
    }

    /// Whether the stream is closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

impl futures_core::Stream for RemoteStream {
    type Item = Result<Vec<u8>, LoomError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().poll_item(cx)
    }
}

/// The client-side registry of open remote handles (sessions, sql sessions, iterators, tasks, files).
#[derive(Default)]
pub struct HandleTracker {
    handles: HashMap<Vec<u8>, RemoteHandleId>,
}

impl HandleTracker {
    /// A new empty tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an open handle, keyed by its server-minted id bytes.
    pub fn register(&mut self, handle: RemoteHandleId) {
        self.handles.insert(handle.id.clone(), handle);
    }

    /// Look up an open handle by id.
    pub fn get(&self, id: &[u8]) -> Option<&RemoteHandleId> {
        self.handles.get(id)
    }

    /// Free a handle locally (the free call itself is local). Returns whether one was open.
    pub fn free(&mut self, id: &[u8]) -> bool {
        self.handles.remove(id).is_some()
    }

    /// The number of open handles.
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// Whether no handles are open.
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }
}

impl<T: Transport> RemoteLoomClient<T> {
    /// Open a server stream for `interface`/`method` with `args`, collecting the emitted frames into a
    /// [`RemoteStream`]. Used for row, watch, byte-chunk, and task streams.
    ///
    /// # Errors
    /// Returns [`LoomError`] on an encode/transport failure or a malformed frame.
    pub async fn open_stream(
        &self,
        interface: &str,
        method: &str,
        args: Vec<Value>,
    ) -> Result<RemoteStream, LoomError> {
        let session_id = self.session_id();
        let request = loom_remote_protocol::envelope::Request {
            request_id: self.next_request_id(),
            session_id,
            interface: interface.to_string(),
            method: method.to_string(),
            args,
            deadline_ms: 0,
            idempotency_key: None,
            principal_hint: None,
            compression: loom_remote_protocol::envelope::Compression::None,
            stream: true,
        };
        let bytes = request
            .encode()
            .map_err(|err| LoomError::new(Code::Internal, format!("encode request: {err}")))?;
        let source = self.connection().transport().open_stream(bytes).await?;
        Ok(RemoteStream::from_source(source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::RemoteLoomClient;
    use crate::connection::RemoteConnection;
    use crate::transport::loopback::Loopback;
    use loom_locator::{ContextResolver, Layer};
    use loom_remote_protocol::RetryAdvice;
    use loom_remote_protocol::discovery::{Discovery, DiscoveryMode};
    use loom_remote_protocol::{RemoteError, frame::Frame};

    fn block<F: std::future::Future>(f: F) -> F::Output {
        futures::executor::block_on(f)
    }

    fn handle(kind: &str, id: u8) -> RemoteHandleId {
        RemoteHandleId {
            kind: kind.to_string(),
            id: vec![id],
            generation: 1,
            owner_session: vec![0],
        }
    }

    #[test]
    fn stream_surfaces_items_then_completes() {
        let encoded = vec![
            Frame::Item(b"row-1".to_vec()).encode().unwrap(),
            Frame::Item(b"row-2".to_vec()).encode().unwrap(),
            Frame::Trailer(b"count=2".to_vec()).encode().unwrap(),
            Frame::Complete.encode().unwrap(),
        ];
        let mut stream = RemoteStream::from_encoded(encoded);
        assert_eq!(block(stream.next_item()).unwrap(), Some(b"row-1".to_vec()));
        assert_eq!(block(stream.next_item()).unwrap(), Some(b"row-2".to_vec()));
        assert_eq!(block(stream.next_item()).unwrap(), None);
        assert!(stream.is_closed());
        assert_eq!(stream.trailer(), Some(&b"count=2"[..]));
    }

    #[test]
    fn stream_surfaces_a_terminal_error() {
        let encoded = vec![
            Frame::Item(b"row-1".to_vec()).encode().unwrap(),
            Frame::Error(RemoteError::from_wire(
                "CURSOR_INVALID",
                "cursor expired",
                RetryAdvice::Never,
                None,
                None,
            ))
            .encode()
            .unwrap(),
        ];
        let mut stream = RemoteStream::from_encoded(encoded);
        assert_eq!(block(stream.next_item()).unwrap(), Some(b"row-1".to_vec()));
        let err = block(stream.next_item()).expect_err("terminal error");
        assert_eq!(err.code, Code::CursorInvalid);
        assert!(stream.is_closed());
    }

    #[test]
    fn handle_tracker_registers_and_frees() {
        let mut tracker = HandleTracker::new();
        tracker.register(handle("row_iter", 1));
        tracker.register(handle("task", 2));
        assert_eq!(tracker.len(), 2);
        assert!(tracker.get(&[1]).is_some());
        assert!(tracker.free(&[1]));
        assert!(!tracker.free(&[1]));
        assert_eq!(tracker.len(), 1);
    }

    fn connect_loopback(loopback: Loopback) -> RemoteLoomClient<Loopback> {
        let resolver = ContextResolver::from_layers(&[Layer::new(
            "test",
            "[contexts.prod]\ntarget = \"https://remote.host/apps/loom\"\n",
        )])
        .unwrap();
        let conn = block(RemoteConnection::connect(
            loopback,
            "prod",
            &resolver,
            DiscoveryMode::WellKnown,
        ))
        .expect("connect");
        RemoteLoomClient::new(conn)
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

    #[test]
    fn open_stream_collects_frames_from_the_transport() {
        let transport = Loopback::new(
            Box::new(|_| Ok(discovery_bytes())),
            Box::new(|_| Ok(Vec::new())),
            Box::new(|_bytes| {
                Ok(vec![
                    Frame::Item(b"x".to_vec()).encode().unwrap(),
                    Frame::Complete.encode().unwrap(),
                ])
            }),
        );
        let client = connect_loopback(transport);
        let mut stream = block(client.open_stream(
            "Sql",
            "sql_query",
            vec![Value::Bytes(vec![1]), Value::Text("SELECT 1".to_string())],
        ))
        .expect("open stream");
        assert_eq!(block(stream.next_item()).unwrap(), Some(b"x".to_vec()));
        assert_eq!(block(stream.next_item()).unwrap(), None);
    }
}
