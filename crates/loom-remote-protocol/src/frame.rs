//! Stream frames and backpressure primitives.
//!
//! A stream carries `open`, `item`, `credit`, `cancel`, `complete`, `error`, and `trailer` frames. Every
//! stream starts with explicit client credit, and the server must not send more `item` frames than the
//! available credit. This module encodes and decodes the frames; the flow-control accounting lives in the
//! server stream registry.
//!
//! Licensed under BUSL-1.1.

use crate::RemoteError;
use crate::codec::ArgError;
use crate::envelope::{as_map, decode_error, encode_error, field, text, text_field, u64_field};
use loom_codec::{CodecError, Value, decode, encode};

/// One stream frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// Open a stream with an id and the client's initial credit.
    Open {
        /// The session-scoped stream id.
        stream_id: u64,
        /// The initial number of `item` frames the server may send.
        credit: u32,
    },
    /// One stream item, a canonical-CBOR payload.
    Item(Vec<u8>),
    /// Grant additional credit for `item` frames.
    Credit(u32),
    /// Cancel the stream; the server releases stream-owned handles.
    Cancel,
    /// The stream completed normally.
    Complete,
    /// The stream failed; a terminal error follows no further items.
    Error(RemoteError),
    /// Trailer metadata (counts, cursors, final digest, or task ids).
    Trailer(Vec<u8>),
}

impl Frame {
    /// Encode to a canonical CBOR frame map.
    ///
    /// # Errors
    /// Returns [`CodecError`] only for a non-finite float, which these frames never carry.
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let entries = match self {
            Frame::Open { stream_id, credit } => vec![
                (text("kind"), text("open")),
                (text("stream_id"), Value::Uint(*stream_id)),
                (text("credit"), Value::Uint(u64::from(*credit))),
            ],
            Frame::Item(data) => {
                vec![
                    (text("kind"), text("item")),
                    (text("data"), Value::Bytes(data.clone())),
                ]
            }
            Frame::Credit(credit) => vec![
                (text("kind"), text("credit")),
                (text("credit"), Value::Uint(u64::from(*credit))),
            ],
            Frame::Cancel => vec![(text("kind"), text("cancel"))],
            Frame::Complete => vec![(text("kind"), text("complete"))],
            Frame::Error(error) => {
                vec![
                    (text("kind"), text("error")),
                    (text("error"), encode_error(error)),
                ]
            }
            Frame::Trailer(data) => {
                vec![
                    (text("kind"), text("trailer")),
                    (text("data"), Value::Bytes(data.clone())),
                ]
            }
        };
        encode(&Value::Map(entries))
    }

    /// Decode from a canonical CBOR frame map.
    ///
    /// # Errors
    /// Returns [`ArgError`] for a non-map buffer, an unknown kind, or a mistyped field.
    pub fn decode(bytes: &[u8]) -> Result<Self, ArgError> {
        let map = as_map(&decode(bytes)?)?;
        let kind = text_field(&map, "kind")?;
        match kind.as_str() {
            "open" => Ok(Frame::Open {
                stream_id: u64_field(&map, "stream_id")?,
                credit: credit_field(&map)?,
            }),
            "item" => Ok(Frame::Item(bytes_field(&map, "data")?)),
            "credit" => Ok(Frame::Credit(credit_field(&map)?)),
            "cancel" => Ok(Frame::Cancel),
            "complete" => Ok(Frame::Complete),
            "error" => Ok(Frame::Error(decode_error(field(&map, "error").ok_or(
                ArgError::TypeMismatch {
                    expected: "frame error",
                },
            )?)?)),
            "trailer" => Ok(Frame::Trailer(bytes_field(&map, "data")?)),
            _ => Err(ArgError::TypeMismatch {
                expected: "known frame kind",
            }),
        }
    }

    /// Whether this frame terminates the stream (`complete`, `error`, or `cancel`).
    pub fn is_terminal(&self) -> bool {
        matches!(self, Frame::Complete | Frame::Error(_) | Frame::Cancel)
    }
}

fn credit_field(map: &[(Value, Value)]) -> Result<u32, ArgError> {
    let raw = u64_field(map, "credit")?;
    u32::try_from(raw).map_err(|_| ArgError::IntRange { target: "u32" })
}

fn bytes_field(map: &[(Value, Value)], key: &str) -> Result<Vec<u8>, ArgError> {
    match field(map, key) {
        Some(Value::Bytes(bytes)) => Ok(bytes.clone()),
        _ => Err(ArgError::TypeMismatch { expected: "bytes" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RetryAdvice;

    fn round_trip(frame: Frame) {
        let decoded = Frame::decode(&frame.encode().unwrap()).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn all_frame_kinds_round_trip() {
        round_trip(Frame::Open {
            stream_id: 12,
            credit: 32,
        });
        round_trip(Frame::Item(vec![1, 2, 3]));
        round_trip(Frame::Credit(8));
        round_trip(Frame::Cancel);
        round_trip(Frame::Complete);
        round_trip(Frame::Trailer(vec![9]));
        round_trip(Frame::Error(RemoteError::from_wire(
            "CURSOR_INVALID",
            "gone",
            RetryAdvice::Never,
            None,
            None,
        )));
    }

    #[test]
    fn terminal_frames_are_flagged() {
        assert!(Frame::Complete.is_terminal());
        assert!(Frame::Cancel.is_terminal());
        assert!(!Frame::Item(vec![]).is_terminal());
        assert!(!Frame::Credit(1).is_terminal());
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let bytes = encode(&Value::Map(vec![(text("kind"), text("bogus"))])).unwrap();
        assert!(Frame::decode(&bytes).is_err());
    }
}
