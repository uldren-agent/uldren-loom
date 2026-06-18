//! Concrete HTTP/2-over-TLS carrier for the remote runtime.
//!
//! [`RemoteHttpServer`] binds a real TCP socket, terminates TLS with a caller-supplied rustls
//! `ServerConfig`, and serves HTTP/2 requests by routing each into [`RemoteHttpService::handle`]. This is
//! the thin socket adapter over the transport-agnostic router: all routing and status
//! semantics live in `RemoteHttpService`, so this module only owns the accept loop and body plumbing. The
//! v1 carrier is HTTP/2 over TLS.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use crate::remote::ServerFrameStream;
use crate::remote_http::{CBOR_CONTENT_TYPE, RemoteHttpService};
use http_body_util::combinators::UnsyncBoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Bytes, Frame as BodyFrame, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;

/// A streaming HTTP body that pushes length-delimited protocol frames from a [`ServerFrameStream`]. Each
/// body chunk is `u32` big-endian length followed by one encoded frame. hyper polls this only when the
/// connection's flow-control window allows a write, so a slow or stalled reader bounds how far the server
/// runs ahead of it (bounded memory), and a client reset drops the body - and with it the source -
/// stopping production.
struct FrameBody {
    stream: ServerFrameStream,
    done: bool,
}

impl Body for FrameBody {
    type Data = Bytes;
    type Error = io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<BodyFrame<Bytes>, io::Error>>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        match this.stream.next_frame() {
            Some(frame) => {
                let mut chunk = Vec::with_capacity(4 + frame.len());
                chunk.extend_from_slice(&(frame.len() as u32).to_be_bytes());
                chunk.extend_from_slice(&frame);
                Poll::Ready(Some(Ok(BodyFrame::data(Bytes::from(chunk)))))
            }
            None => {
                this.done = true;
                Poll::Ready(None)
            }
        }
    }
}

/// A running HTTP/2-over-TLS listener that serves one remote endpoint over a [`RemoteHttpService`].
pub struct RemoteHttpServer {
    local_addr: SocketAddr,
    accept_task: tokio::task::JoinHandle<()>,
}

impl RemoteHttpServer {
    /// Bind `addr`, terminate TLS with `tls`, and serve HTTP/2 into `service`. Returns once the socket is
    /// bound; connections are accepted on the current Tokio runtime.
    ///
    /// # Errors
    /// Returns an [`io::Error`] when the address cannot be bound.
    pub async fn bind(
        addr: SocketAddr,
        tls: impl Into<Arc<ServerConfig>>,
        service: Arc<RemoteHttpService>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let acceptor = TlsAcceptor::from(tls.into());
        let accept_task = tokio::spawn(async move {
            loop {
                let Ok((stream, _peer)) = listener.accept().await else {
                    continue;
                };
                let acceptor = acceptor.clone();
                let service = service.clone();
                tokio::spawn(async move {
                    let Ok(tls_stream) = acceptor.accept(stream).await else {
                        return;
                    };
                    let io = TokioIo::new(tls_stream);
                    let handler = service_fn(move |req: Request<Incoming>| {
                        let service = service.clone();
                        async move { serve(service, req).await }
                    });
                    let _ = auto::Builder::new(TokioExecutor::new())
                        .serve_connection(io, handler)
                        .await;
                });
            }
        });
        Ok(Self {
            local_addr,
            accept_task,
        })
    }

    /// The bound local address (with the resolved port when bound to port 0).
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Stop accepting connections and release the socket.
    pub fn shutdown(self) {
        self.accept_task.abort();
    }
}

async fn serve(
    service: Arc<RemoteHttpService>,
    req: Request<Incoming>,
) -> Result<Response<UnsyncBoxBody<Bytes, io::Error>>, hyper::Error> {
    let method = req.method().as_str().to_string();
    let path = req.uri().path().to_string();
    let body = req.into_body().collect().await?.to_bytes();

    // A streaming call gets a length-delimited frame body pushed incrementally; every other request is a
    // buffered unary response.
    if let Some(stream) = service.open_stream(&method, &path, &body) {
        let body = FrameBody {
            stream,
            done: false,
        }
        .boxed_unsync();
        let response = Response::builder()
            .status(200)
            .header("content-type", CBOR_CONTENT_TYPE)
            .body(body)
            .expect("a valid response is always buildable");
        return Ok(response);
    }

    let result = service.handle(&method, &path, &body);
    let body = Full::new(Bytes::from(result.body))
        .map_err(|never: Infallible| match never {})
        .boxed_unsync();
    let response = Response::builder()
        .status(result.status)
        .header("content-type", result.content_type)
        .body(body)
        .expect("a valid response is always buildable");
    Ok(response)
}
