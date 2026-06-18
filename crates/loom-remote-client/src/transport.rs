//! The pluggable carrier seam for the Loom Remote Protocol.
//!
//! The core client is transport-agnostic: it builds and parses canonical-CBOR envelopes and
//! never touches sockets. A concrete HTTP/2-over-TLS carrier implements [`Transport`] with async network
//! I/O; unit tests use an in-memory loopback that routes to a server runtime. The trait is async-first,
//! using return-position `impl Future` so no `async_trait` dependency is needed.
//!
//! Licensed under BUSL-1.1.

use loom_types::LoomError;
use std::collections::VecDeque;
use std::future::Future;
use std::task::{Context, Poll};

/// A pull-based source of encoded protocol frames delivered incrementally. Implementors apply their own
/// flow control (a bounded channel over the wire, an in-memory queue in process), so a consumer that
/// pulls slowly bounds the memory the whole pipeline holds. `Send` so a stream can move across tasks.
pub trait FrameStreamSource: Send {
    /// Poll for the next encoded frame: `Ready(Some(Ok(frame)))` for a frame, `Ready(Some(Err(..)))` for
    /// a transport failure mid-stream, `Ready(None)` at end of stream, `Pending` when no frame is ready.
    fn poll_frame(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Vec<u8>, LoomError>>>;
}

/// An incremental source of the encoded frames a server emits for one stream. Returned by
/// [`Transport::open_stream`]; the consumer pulls frames with [`FrameSource::next_frame`] as it advances,
/// so the carrier never has to buffer the whole (possibly unbounded) stream. Dropping the source closes
/// the underlying stream.
pub struct FrameSource {
    inner: Box<dyn FrameStreamSource>,
}

impl FrameSource {
    /// Build a source from any [`FrameStreamSource`] (a bounded wire reader, a queue, a producer).
    pub fn new(inner: impl FrameStreamSource + 'static) -> Self {
        Self {
            inner: Box::new(inner),
        }
    }

    /// A source that yields already-collected `frames` in order, then ends. Used by the in-process
    /// carrier bridge and the loopback transport, where all frames are produced synchronously.
    pub fn from_frames(frames: Vec<Vec<u8>>) -> Self {
        Self::new(BufferedFrames {
            frames: frames.into(),
        })
    }

    /// Pull the next encoded frame, or `None` at end of stream.
    ///
    /// # Errors
    /// Returns [`LoomError`] for a transport failure mid-stream.
    pub fn next_frame(
        &mut self,
    ) -> impl Future<Output = Option<Result<Vec<u8>, LoomError>>> + Send + '_ {
        std::future::poll_fn(move |cx| self.inner.poll_frame(cx))
    }

    /// Poll for the next encoded frame (the polling primitive behind [`FrameSource::next_frame`], used by
    /// stream adapters that drive the source from their own `poll_next`).
    pub fn poll_frame(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Vec<u8>, LoomError>>> {
        self.inner.poll_frame(cx)
    }
}

impl std::fmt::Debug for FrameSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FrameSource")
    }
}

/// A synchronous source over a fixed queue of frames.
struct BufferedFrames {
    frames: VecDeque<Vec<u8>>,
}

impl FrameStreamSource for BufferedFrames {
    fn poll_frame(&mut self, _cx: &mut Context<'_>) -> Poll<Option<Result<Vec<u8>, LoomError>>> {
        Poll::Ready(self.frames.pop_front().map(Ok))
    }
}

impl<T: FrameStreamSource + ?Sized> FrameStreamSource for Box<T> {
    fn poll_frame(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Vec<u8>, LoomError>>> {
        (**self).poll_frame(cx)
    }
}

/// A carrier for remote protocol traffic. Every method that crosses the network is async.
pub trait Transport {
    /// Fetch the discovery document bytes at `path` (a credential-free GET).
    ///
    /// # Errors
    /// Returns [`LoomError`] when the route is unavailable or the fetch fails.
    fn discover(&self, path: &str) -> impl Future<Output = Result<Vec<u8>, LoomError>> + Send;

    /// Send one unary call envelope and return the response envelope bytes.
    ///
    /// # Errors
    /// Returns [`LoomError`] on a transport failure before a response envelope is produced.
    fn call(&self, request: Vec<u8>) -> impl Future<Output = Result<Vec<u8>, LoomError>> + Send;

    /// Open a stream for a request envelope and return an incremental [`FrameSource`] over the frames the
    /// server emits. The source is pulled frame-by-frame, so an unbounded stream never has to be buffered
    /// whole; dropping it closes the underlying stream.
    ///
    /// # Errors
    /// Returns [`LoomError`] on a transport failure before the stream opens.
    fn open_stream(
        &self,
        request: Vec<u8>,
    ) -> impl Future<Output = Result<FrameSource, LoomError>> + Send;

    /// Post a session-open request to the carrier's dedicated session route and return the reply bytes.
    /// The client uses this to obtain a server-minted session id before any dispatch; the reply is a
    /// canonical session-open envelope decoded by the caller.
    ///
    /// # Errors
    /// Returns [`LoomError`] on a transport failure before a reply is produced.
    fn open_session(
        &self,
        request: Vec<u8>,
    ) -> impl Future<Output = Result<Vec<u8>, LoomError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffered_frame_source_yields_in_order_then_ends() {
        let mut source = FrameSource::from_frames(vec![b"a".to_vec(), b"b".to_vec()]);
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(waker);
        match source.poll_frame(&mut cx) {
            Poll::Ready(Some(Ok(frame))) => assert_eq!(frame, b"a"),
            other => panic!("expected first frame, got {other:?}"),
        }
        match source.poll_frame(&mut cx) {
            Poll::Ready(Some(Ok(frame))) => assert_eq!(frame, b"b"),
            other => panic!("expected second frame, got {other:?}"),
        }
        assert!(
            matches!(source.poll_frame(&mut cx), Poll::Ready(None)),
            "the source ends after its frames"
        );
    }
}

#[cfg(test)]
pub(crate) mod loopback {
    use super::*;

    type DiscoverFn = Box<dyn Fn(&str) -> Result<Vec<u8>, LoomError> + Send + Sync>;
    type CallFn = Box<dyn Fn(Vec<u8>) -> Result<Vec<u8>, LoomError> + Send + Sync>;
    type StreamFn = Box<dyn Fn(Vec<u8>) -> Result<Vec<Vec<u8>>, LoomError> + Send + Sync>;

    /// An in-memory transport that routes protocol traffic to caller-supplied handlers. The handlers run
    /// synchronously and the trait methods wrap their results in ready futures, so tests exercise the full
    /// envelope round trip without a socket.
    pub(crate) struct Loopback {
        discover: DiscoverFn,
        call: CallFn,
        stream: StreamFn,
    }

    impl Loopback {
        pub(crate) fn new(discover: DiscoverFn, call: CallFn, stream: StreamFn) -> Self {
            Self {
                discover,
                call,
                stream,
            }
        }

        /// A loopback whose stream route is unused (returns an empty frame list).
        pub(crate) fn unary(discover: DiscoverFn, call: CallFn) -> Self {
            Self::new(discover, call, Box::new(|_| Ok(Vec::new())))
        }
    }

    impl Transport for Loopback {
        fn discover(&self, path: &str) -> impl Future<Output = Result<Vec<u8>, LoomError>> + Send {
            let result = (self.discover)(path);
            async move { result }
        }

        fn call(
            &self,
            request: Vec<u8>,
        ) -> impl Future<Output = Result<Vec<u8>, LoomError>> + Send {
            let result = (self.call)(request);
            async move { result }
        }

        fn open_stream(
            &self,
            request: Vec<u8>,
        ) -> impl Future<Output = Result<FrameSource, LoomError>> + Send {
            let result = (self.stream)(request).map(FrameSource::from_frames);
            async move { result }
        }

        async fn open_session(&self, _request: Vec<u8>) -> Result<Vec<u8>, LoomError> {
            Err(LoomError::new(
                loom_types::Code::Unsupported,
                "loopback transport has no session route",
            ))
        }
    }
}
