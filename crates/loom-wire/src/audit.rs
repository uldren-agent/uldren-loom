//! Canonical wire codecs for the generated `Audit` control surface.

use loom_codec::{Value as CborValue, decode, encode};
use loom_core::Digest;
use loom_types::{Code, LoomError};

fn enc(value: CborValue) -> Vec<u8> {
    encode(&value).expect("canonical cbor encode of audit result never fails")
}

fn arr(bytes: &[u8]) -> Result<Vec<CborValue>, LoomError> {
    match decode(bytes)
        .map_err(|err| LoomError::new(Code::InvalidArgument, format!("audit cbor: {err}")))?
    {
        CborValue::Array(items) => Ok(items),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "audit result must be a CBOR array",
        )),
    }
}

fn uint(items: &[CborValue], i: usize) -> Result<u64, LoomError> {
    match items.get(i) {
        Some(CborValue::Uint(n)) => Ok(*n),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "audit field must be an unsigned integer",
        )),
    }
}

fn opt_uint(items: &[CborValue], i: usize) -> Result<Option<u64>, LoomError> {
    match items.get(i) {
        Some(CborValue::Uint(n)) => Ok(Some(*n)),
        Some(CborValue::Null) => Ok(None),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "audit optional field must be an unsigned integer or null",
        )),
    }
}

fn opt_digest(items: &[CborValue], i: usize) -> Result<Option<Digest>, LoomError> {
    match items.get(i) {
        Some(CborValue::Text(text)) => Digest::parse(text).map(Some),
        Some(CborValue::Null) => Ok(None),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "audit optional digest field must be text or null",
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditCompactResult {
    pub pruned: u64,
    pub checkpoint_seq: Option<u64>,
    pub checkpoint_hash: Option<Digest>,
    pub audit_seq: u64,
}

pub fn audit_compact_result_to_cbor(result: &AuditCompactResult) -> Vec<u8> {
    enc(CborValue::Array(vec![
        CborValue::Uint(result.pruned),
        result
            .checkpoint_seq
            .map_or(CborValue::Null, CborValue::Uint),
        result
            .checkpoint_hash
            .as_ref()
            .map_or(CborValue::Null, |digest| {
                CborValue::Text(digest.to_string())
            }),
        CborValue::Uint(result.audit_seq),
    ]))
}

pub fn audit_compact_result_from_cbor(bytes: &[u8]) -> Result<AuditCompactResult, LoomError> {
    let items = arr(bytes)?;
    if items.len() != 4 {
        return Err(LoomError::new(
            Code::InvalidArgument,
            "audit compact result must have four fields",
        ));
    }
    Ok(AuditCompactResult {
        pruned: uint(&items, 0)?,
        checkpoint_seq: opt_uint(&items, 1)?,
        checkpoint_hash: opt_digest(&items, 2)?,
        audit_seq: uint(&items, 3)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::digest::Algo;

    #[test]
    fn audit_compact_result_round_trips() {
        let result = AuditCompactResult {
            pruned: 3,
            checkpoint_seq: Some(2),
            checkpoint_hash: Some(Digest::hash(Algo::Blake3, b"checkpoint")),
            audit_seq: 4,
        };
        assert_eq!(
            audit_compact_result_from_cbor(&audit_compact_result_to_cbor(&result)).unwrap(),
            result
        );
    }

    #[test]
    fn audit_compact_result_rejects_malformed_cbor() {
        let bad = encode(&CborValue::Uint(1)).unwrap();
        assert_eq!(
            audit_compact_result_from_cbor(&bad).unwrap_err().code,
            Code::InvalidArgument
        );
    }
}
