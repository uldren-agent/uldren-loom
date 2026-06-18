//! Canonical API/wire CBOR encoders and decoders for the generated Loom API surface.
//!
//! This crate owns the wire forms for composite API values so that the in-process client
//! (`loom-client`), the remote server dispatch, and the C ABI (`loom-ffi`) all produce identical
//! bytes from a single implementation. It stays engine-adjacent: it may depend on `loom-core`,
//! `loom-codec`, and `loom-types`, and must not depend on `loom-ffi`, `loom-client`, the hosted
//! server, or the remote client.
//!
//! Licensed under BUSL-1.1.

pub mod acl;
pub mod calendar;
pub mod columnar;
pub mod document;
pub mod fs;
pub mod graph;
pub mod identity;
pub mod lock;
pub mod protected_ref;
pub mod store_admin;
pub mod transfer;
pub mod vcs;
pub mod vector;
pub mod watch;
pub mod workspace;

use loom_codec::{Value, encode};
use loom_core::Digest;
use loom_types::{Code, LoomError};

/// Encode a list of content addresses as a canonical CBOR array of `algo:hex` strings.
pub fn digest_list_to_cbor(digests: Vec<Digest>) -> Result<Vec<u8>, LoomError> {
    let values = digests
        .into_iter()
        .map(|digest| Value::Text(digest.to_string()))
        .collect::<Vec<_>>();
    encode(&Value::Array(values))
        .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

/// Encode a list of strings as a canonical CBOR array of text. This is the wire form for the
/// `list_collections`/`list_books`/`list_mailboxes`/`get_flags` facet accessors.
pub fn string_list_to_cbor(strings: Vec<String>) -> Result<Vec<u8>, LoomError> {
    let values = strings.into_iter().map(Value::Text).collect::<Vec<_>>();
    encode(&Value::Array(values))
        .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

/// Encode a list of opaque byte blobs as a canonical CBOR array of byte strings. This is the wire
/// form for accessors that return a list of already-encoded records (e.g. `trigger_list`,
/// `trigger_history`, `list_entries`, `list_messages`, calendar `search`).
pub fn bytes_list_to_cbor(items: Vec<Vec<u8>>) -> Result<Vec<u8>, LoomError> {
    let values = items.into_iter().map(Value::Bytes).collect::<Vec<_>>();
    encode(&Value::Array(values))
        .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

/// Decode a canonical CBOR array of text into a list of strings (the inverse of
/// [`string_list_to_cbor`]; the wire form for `set_flags`).
pub fn string_list_from_cbor(bytes: &[u8]) -> Result<Vec<String>, LoomError> {
    let value = loom_codec::decode(bytes)
        .map_err(|err| LoomError::new(Code::InvalidArgument, format!("cbor: {err}")))?;
    let Value::Array(items) = value else {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "expected a CBOR array",
        ));
    };
    items
        .into_iter()
        .map(|item| match item {
            Value::Text(s) => Ok(s),
            _ => Err(LoomError::new(
                Code::InvalidArgument,
                "list item must be text",
            )),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::digest::Algo;

    #[test]
    fn digest_list_round_trips_as_text_array() {
        let a = Digest::hash(Algo::Blake3, b"a");
        let b = Digest::hash(Algo::Blake3, b"b");
        let a_text = a.to_string();
        let b_text = b.to_string();
        let bytes = digest_list_to_cbor(vec![a, b]).unwrap();
        let Value::Array(items) = loom_codec::decode(&bytes).unwrap() else {
            panic!("expected array");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], Value::Text(a_text));
        assert_eq!(items[1], Value::Text(b_text));
    }

    #[test]
    fn empty_list_encodes_to_empty_array() {
        let bytes = digest_list_to_cbor(Vec::new()).unwrap();
        assert_eq!(
            loom_codec::decode(&bytes).unwrap(),
            Value::Array(Vec::new())
        );
    }

    #[test]
    fn string_list_round_trips() {
        let bytes = string_list_to_cbor(vec!["a".to_string(), "b".to_string()]).unwrap();
        assert_eq!(
            string_list_from_cbor(&bytes).unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn string_list_from_malformed_cbor_is_invalid_argument() {
        // A truncated/garbage buffer that is not decodable CBOR.
        let err = string_list_from_cbor(&[0xff, 0xff, 0xff]).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
    }

    #[test]
    fn string_list_from_non_array_is_invalid_argument() {
        let bytes = loom_codec::encode(&Value::Text("not an array".to_string())).unwrap();
        let err = string_list_from_cbor(&bytes).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
    }

    #[test]
    fn string_list_from_non_text_item_is_invalid_argument() {
        let bytes = loom_codec::encode(&Value::Array(vec![Value::Uint(7)])).unwrap();
        let err = string_list_from_cbor(&bytes).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
    }
}
