//! Concrete HTTP/2-over-TLS client transport.
//!
//! [`Http2TlsTransport`] is the production [`Transport`](crate::transport::Transport): it connects to one
//! endpoint over TCP, terminates TLS with a caller-supplied rustls `ClientConfig`, performs an HTTP/2
//! handshake, and multiplexes discovery, unary, and stream requests over the single connection. Response
//! status is mapped to envelope bytes or a stable error by the shared client HTTP mapping
//! (`crate::http`).
//!
//! Licensed under BUSL-1.1.

use crate::http::{call_request, discovery_request, parse_response};
use crate::transport::{FrameSource, FrameStreamSource, Transport};
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper::body::{Bytes, Incoming};
use hyper_util::rt::{TokioExecutor, TokioIo};
use loom_types::{Code, LoomError};
use std::net::SocketAddr;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;

type Sender = hyper::client::conn::http2::SendRequest<Full<Bytes>>;

/// The number of frames the client buffers ahead of the consumer: the credit window. When the consumer
/// falls this far behind, the reader task stops pulling the response body, which closes the HTTP/2
/// flow-control window and backpressures the server - so the whole pipeline holds a bounded number of
/// frames regardless of stream length.
const STREAM_CREDIT: usize = 16;

/// A [`FrameStreamSource`] fed by the background reader task over a bounded channel. Polling it yields the
/// next decoded frame; dropping it drops the receiver, which ends the reader task and drops the HTTP/2
/// response (resetting the server stream).
struct ChannelSource {
    rx: mpsc::Receiver<Result<Vec<u8>, LoomError>>,
}

impl FrameStreamSource for ChannelSource {
    fn poll_frame(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Vec<u8>, LoomError>>> {
        self.rx.poll_recv(cx)
    }
}

/// Split off one length-delimited frame (`u32` big-endian length then that many bytes) from the front of
/// `buf`, returning it and removing its bytes; `None` when a whole frame is not yet buffered.
fn take_frame(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    if buf.len() < 4 {
        return None;
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if buf.len() < 4 + len {
        return None;
    }
    let frame = buf[4..4 + len].to_vec();
    buf.drain(..4 + len);
    Some(frame)
}

/// Read length-delimited frames off the response body and forward them to `tx`, one at a time. A full
/// channel parks the send (backpressure); a dropped receiver ends the task (dropping the body resets the
/// stream); end of body flushes any buffered frames and closes the channel.
async fn read_frames(mut body: Incoming, tx: mpsc::Sender<Result<Vec<u8>, LoomError>>) {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        while let Some(frame) = take_frame(&mut buf) {
            if tx.send(Ok(frame)).await.is_err() {
                return;
            }
        }
        match body.frame().await {
            Some(Ok(chunk)) => {
                if let Ok(data) = chunk.into_data() {
                    buf.extend_from_slice(&data);
                }
            }
            Some(Err(err)) => {
                let _ = tx
                    .send(Err(LoomError::new(Code::Io, format!("stream body: {err}"))))
                    .await;
                return;
            }
            None => {
                while let Some(frame) = take_frame(&mut buf) {
                    if tx.send(Ok(frame)).await.is_err() {
                        return;
                    }
                }
                return;
            }
        }
    }
}

/// A client transport that speaks HTTP/2 over TLS to one endpoint.
pub struct Http2TlsTransport {
    addr: SocketAddr,
    server_name: String,
    call_path: String,
    connector: TlsConnector,
    sender: Mutex<Option<Sender>>,
}

impl Http2TlsTransport {
    /// Bind a transport to `addr`, presenting `server_name` for TLS verification, POSTing calls to
    /// `call_path`, and trusting/verifying peers per `config`.
    pub fn new(
        addr: SocketAddr,
        server_name: impl Into<String>,
        call_path: impl Into<String>,
        config: ClientConfig,
    ) -> Self {
        Self {
            addr,
            server_name: server_name.into(),
            call_path: call_path.into(),
            connector: TlsConnector::from(Arc::new(config)),
            sender: Mutex::new(None),
        }
    }

    async fn connect(&self) -> Result<Sender, LoomError> {
        let tcp = TcpStream::connect(self.addr).await.map_err(io_err)?;
        let name = ServerName::try_from(self.server_name.clone())
            .map_err(|_| LoomError::new(Code::InvalidArgument, "invalid TLS server name"))?;
        let tls = self.connector.connect(name, tcp).await.map_err(io_err)?;
        let io = TokioIo::new(tls);
        let (sender, conn) = hyper::client::conn::http2::handshake(TokioExecutor::new(), io)
            .await
            .map_err(|err| LoomError::new(Code::Io, format!("http2 handshake: {err}")))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        Ok(sender)
    }

    async fn acquire(&self) -> Result<Sender, LoomError> {
        let mut guard = self.sender.lock().await;
        if let Some(sender) = guard.as_ref() {
            return Ok(sender.clone());
        }
        let sender = self.connect().await?;
        *guard = Some(sender.clone());
        Ok(sender)
    }

    async fn request(
        &self,
        method: &'static str,
        path: &str,
        body: Vec<u8>,
    ) -> Result<(u16, Vec<u8>), LoomError> {
        let uri = format!("https://{}{}", self.server_name, path);
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", crate::http::CBOR_CONTENT_TYPE)
            .body(Full::new(Bytes::from(body)))
            .map_err(|err| LoomError::new(Code::Internal, format!("build request: {err}")))?;
        let mut sender = self.acquire().await?;
        let response = sender
            .send_request(request)
            .await
            .map_err(|err| LoomError::new(Code::Io, format!("send request: {err}")))?;
        let status = response.status().as_u16();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|err| LoomError::new(Code::Io, format!("read body: {err}")))?
            .to_bytes()
            .to_vec();
        Ok((status, bytes))
    }
}

fn io_err(err: std::io::Error) -> LoomError {
    LoomError::new(Code::Io, format!("transport: {err}"))
}

impl Transport for Http2TlsTransport {
    async fn discover(&self, path: &str) -> Result<Vec<u8>, LoomError> {
        let parts = discovery_request(path);
        let (status, body) = self.request(parts.method, &parts.path, parts.body).await?;
        parse_response(status, body)
    }

    async fn call(&self, request: Vec<u8>) -> Result<Vec<u8>, LoomError> {
        let parts = call_request(&self.call_path, request);
        let (status, body) = self.request(parts.method, &parts.path, parts.body).await?;
        parse_response(status, body)
    }

    async fn open_session(&self, request: Vec<u8>) -> Result<Vec<u8>, LoomError> {
        let session_path = loom_remote_protocol::session::session_route(&self.call_path);
        let (status, body) = self.request("POST", &session_path, request).await?;
        parse_response(status, body)
    }

    async fn open_stream(&self, request: Vec<u8>) -> Result<FrameSource, LoomError> {
        let parts = call_request(&self.call_path, request);
        let uri = format!("https://{}{}", self.server_name, parts.path);
        let http_request = Request::builder()
            .method(parts.method)
            .uri(uri)
            .header("content-type", crate::http::CBOR_CONTENT_TYPE)
            .body(Full::new(Bytes::from(parts.body)))
            .map_err(|err| LoomError::new(Code::Internal, format!("build request: {err}")))?;
        let mut sender = self.acquire().await?;
        let response = sender
            .send_request(http_request)
            .await
            .map_err(|err| LoomError::new(Code::Io, format!("send request: {err}")))?;
        let status = response.status().as_u16();
        if status != 200 {
            // A non-200 opens no stream: read the (small) body and surface the stable transport error.
            let body = response
                .into_body()
                .collect()
                .await
                .map_err(|err| LoomError::new(Code::Io, format!("read body: {err}")))?
                .to_bytes()
                .to_vec();
            return Err(parse_response(status, body)
                .err()
                .unwrap_or_else(|| LoomError::new(Code::Internal, "stream open failed")));
        }
        // Frames are read off the body incrementally into a bounded channel that backpressures the server
        // via HTTP/2 flow control; the returned source pulls them one at a time.
        let (tx, rx) = mpsc::channel::<Result<Vec<u8>, LoomError>>(STREAM_CREDIT);
        tokio::spawn(read_frames(response.into_body(), tx));
        Ok(FrameSource::new(ChannelSource { rx }))
    }
}
