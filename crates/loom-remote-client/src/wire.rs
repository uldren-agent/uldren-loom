//! Wire helpers shared by the generated `RemoteLoomClient` stubs.
//!
//! The generated per-interface impls in `generated_client.rs` forward every round-trip method to
//! [`RemoteLoomClient::call`](crate::client::RemoteLoomClient) / `open_stream` and decode the reply, and
//! run every local (`Lo`) method here. This module holds the pieces that are hand-written once and reused
//! by all of them: a response-shape error, the pull-based [`RemoteStream`] to async [`LoomStream`]
//! adapter and the engine-free
//! content address for `Store.blob_digest`.
//!
//! Licensed under BUSL-1.1.

use crate::stream::RemoteStream;
use loom_codec::{Value, encode_object};
use loom_remote_protocol::api_types::LoomStream;
use loom_remote_protocol::codec::FromValue;
use loom_types::digest::{Algo, Digest};
use loom_types::{Code, LoomError};

/// The stable error for a reply whose CBOR shape does not match the method's declared return type.
pub fn shape(expected: &str) -> LoomError {
    LoomError::new(
        Code::CorruptObject,
        format!("unexpected response shape (expected {expected})"),
    )
}

/// Decode a reply value into a typed result `T` via the shared codec, mapping a shape or range error to a
/// stable [`LoomError`]. The generated client stubs call this for every scalar, handle, `Digest`, and
/// `Uuid` return.
pub fn from_wire<T: FromValue>(value: &Value) -> Result<T, LoomError> {
    T::from_value(value)
        .map_err(|err| LoomError::new(Code::CorruptObject, format!("decode result: {err}")))
}

/// Adapt an incremental [`RemoteStream`] into the async [`LoomStream`] returned by generated streaming
/// methods. `RemoteStream` is itself a `futures_core::Stream` that pulls one frame at a time, so this is a
/// thin boxing: each server `item` frame surfaces as one `Ok(bytes)` element, a terminal `error` frame as
/// `Err`, and normal completion (or a dropped consumer) as end of stream.
pub fn into_loom_stream(stream: RemoteStream) -> LoomStream<Vec<u8>> {
    Box::pin(stream)
}

/// The content address of a blob, computed locally: the BLAKE3 hash of
/// the canonical `[epoch, type=Blob, bytes]` object framing. This mirrors `loom_core::Object::Blob(..)`
/// exactly (guarded by a cross-check test) so the engine-free client and the engine agree byte-for-byte
/// without the remote client depending on the engine crate.
pub fn blob_digest(data: &[u8]) -> String {
    // Object type code for `Blob` in the Loom v1 object framing (`loom_core::object::ObjectType::Blob`).
    const OBJECT_TYPE_BLOB: u16 = 0x01;
    let canonical = encode_object(OBJECT_TYPE_BLOB, &[Value::Bytes(data.to_vec())])
        .expect("a blob object always encodes to canonical CBOR");
    Digest::hash(Algo::Blake3, &canonical).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_digest_matches_the_engine_object_address() {
        for data in [&b""[..], b"abc", &[0u8; 200][..]] {
            let engine = loom_core::Object::Blob(data.to_vec()).digest().to_string();
            assert_eq!(blob_digest(data), engine, "blob digest drift for {data:?}");
        }
    }
}
