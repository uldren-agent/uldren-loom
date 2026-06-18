//! Canonical wire codec for the byte-transfer interchange (`specs/0067` §17).
//!
//! `transfer_import_write` returns a `TransferAccept` (IDL struct `{accepted_bytes, credit}`), encoded
//! here as the canonical CBOR array `[accepted_bytes, credit]` of unsigned integers. `transfer_import_open`
//! returns an opaque `TransferId` carried as raw handle bytes (no codec is needed for opaque bytes), and
//! `transfer_import_finish` returns the canonical `loom.interchange.import-report.v1` produced by the
//! interchange layer (already a byte payload). Malformed input is `INVALID_ARGUMENT`.

use loom_codec::{Value as CborValue, decode, encode};
use loom_types::{Code, LoomError};

/// Encode a `TransferAccept` as canonical CBOR `[accepted_bytes, credit]`.
pub fn transfer_accept_to_cbor(accepted_bytes: u64, credit: u64) -> Vec<u8> {
    encode(&CborValue::Array(vec![
        CborValue::Uint(accepted_bytes),
        CborValue::Uint(credit),
    ]))
    .expect("canonical cbor encode of transfer accept never fails")
}

/// Decode a `TransferAccept` CBOR array into `(accepted_bytes, credit)`.
pub fn transfer_accept_from_cbor(bytes: &[u8]) -> Result<(u64, u64), LoomError> {
    let value = decode(bytes).map_err(|err| {
        LoomError::new(
            Code::InvalidArgument,
            format!("transfer accept cbor: {err}"),
        )
    })?;
    let CborValue::Array(items) = value else {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "transfer accept must be a CBOR array",
        ));
    };
    if items.len() != 2 {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "transfer accept must be [accepted_bytes, credit]",
        ));
    }
    Ok((as_uint(&items[0])?, as_uint(&items[1])?))
}

fn as_uint(value: &CborValue) -> Result<u64, LoomError> {
    match value {
        CborValue::Uint(n) => Ok(*n),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "transfer accept field must be an unsigned integer",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_accept_round_trips() {
        let bytes = transfer_accept_to_cbor(42, 100);
        assert_eq!(transfer_accept_from_cbor(&bytes).unwrap(), (42, 100));
    }

    #[test]
    fn transfer_accept_rejects_non_array() {
        let bad = encode(&CborValue::Uint(1)).unwrap();
        assert_eq!(
            transfer_accept_from_cbor(&bad).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn transfer_accept_rejects_wrong_arity() {
        let bad = encode(&CborValue::Array(vec![CborValue::Uint(1)])).unwrap();
        assert_eq!(
            transfer_accept_from_cbor(&bad).unwrap_err().code,
            Code::InvalidArgument
        );
    }
}
