//! `RemoteLoomClient`: the Loom Remote Protocol client.
//!
//! This crate implements the client half of the remote protocol over `loom-remote-protocol` and a
//! pluggable [`Transport`](transport::Transport). It is engine-free so
//! remote-only packages link it without the engine. The connection layer resolves locators and performs
//! discovery, unary call envelopes, row/watch streams, remote handles, tasks, and result views. The
//! concrete
//! HTTP/2-over-TLS carrier is a `Transport` implementor supplied by the deploying package.
//!
//! Licensed under BUSL-1.1.

#[cfg(feature = "carrier")]
pub mod carrier;
pub mod client;
pub mod connection;
mod generated_client;
pub mod http;
mod local_ops;
pub mod stream;
pub mod transport;
pub mod wire;

#[cfg(feature = "carrier")]
pub use carrier::Http2TlsTransport;

pub use client::{CallOptions, RemoteLoomClient};
pub use connection::{CLIENT_MAX_VERSION, CLIENT_MIN_VERSION, RemoteConnection};
pub use http::{
    HttpRequestParts, call_request, discovery_request, parse_response, parse_stream_response,
};
pub use stream::{HandleTracker, RemoteStream};
pub use transport::Transport;
