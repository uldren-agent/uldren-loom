//! Canonical wire codecs for the `StoreAdmin` control plane (`specs/0067` §13, task 640).
//!
//! `StoreAdmin` is the server-owned store-administration surface: `store_stat` (read), `store_policy_get`
//! (read), `store_policy_set` (audited), and `store_rekey` (server-side crypto, audited). Each method
//! returns canonical CBOR encoded here; the plaintext credential and the data-encryption key never cross
//! the wire (rekey generates all key material server-side). Malformed input is `INVALID_ARGUMENT`.

use loom_codec::{Value as CborValue, decode, encode};
use loom_types::{Code, LoomError};

fn enc(value: CborValue) -> Vec<u8> {
    encode(&value).expect("canonical cbor encode of store-admin result never fails")
}

fn arr(bytes: &[u8]) -> Result<Vec<CborValue>, LoomError> {
    match decode(bytes)
        .map_err(|err| LoomError::new(Code::InvalidArgument, format!("store-admin cbor: {err}")))?
    {
        CborValue::Array(items) => Ok(items),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "store-admin result must be a CBOR array",
        )),
    }
}

fn uint(items: &[CborValue], i: usize) -> Result<u64, LoomError> {
    match items.get(i) {
        Some(CborValue::Uint(n)) => Ok(*n),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "store-admin field must be an unsigned integer",
        )),
    }
}

fn opt_uint(items: &[CborValue], i: usize) -> Result<Option<u64>, LoomError> {
    match items.get(i) {
        Some(CborValue::Uint(n)) => Ok(Some(*n)),
        Some(CborValue::Null) => Ok(None),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "store-admin optional field must be an unsigned integer or null",
        )),
    }
}

fn boolean(items: &[CborValue], i: usize) -> Result<bool, LoomError> {
    match items.get(i) {
        Some(CborValue::Bool(b)) => Ok(*b),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "store-admin field must be a boolean",
        )),
    }
}

fn text(items: &[CborValue], i: usize) -> Result<String, LoomError> {
    match items.get(i) {
        Some(CborValue::Text(s)) => Ok(s.clone()),
        _ => Err(LoomError::new(
            Code::InvalidArgument,
            "store-admin field must be text",
        )),
    }
}

/// The store maintenance/size snapshot returned by `store_stat` (`loom.store.stat.v1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreStat {
    pub object_count: u64,
    pub generation: u64,
    pub physical_page_count: u64,
    pub physical_bytes: u64,
    pub reusable_free_pages: u64,
    pub candidate_dead_pages: u64,
    pub last_validated_mark_epoch: u64,
    pub touched_segments: u64,
    pub candidate_segments: u64,
    pub segment_overflow: u64,
}

/// Encode a [`StoreStat`] as the canonical CBOR array (10 unsigned integers, field order below).
pub fn store_stat_to_cbor(stat: &StoreStat) -> Vec<u8> {
    enc(CborValue::Array(vec![
        CborValue::Uint(stat.object_count),
        CborValue::Uint(stat.generation),
        CborValue::Uint(stat.physical_page_count),
        CborValue::Uint(stat.physical_bytes),
        CborValue::Uint(stat.reusable_free_pages),
        CborValue::Uint(stat.candidate_dead_pages),
        CborValue::Uint(stat.last_validated_mark_epoch),
        CborValue::Uint(stat.touched_segments),
        CborValue::Uint(stat.candidate_segments),
        CborValue::Uint(stat.segment_overflow),
    ]))
}

/// Decode a [`StoreStat`] CBOR array.
pub fn store_stat_from_cbor(bytes: &[u8]) -> Result<StoreStat, LoomError> {
    let items = arr(bytes)?;
    Ok(StoreStat {
        object_count: uint(&items, 0)?,
        generation: uint(&items, 1)?,
        physical_page_count: uint(&items, 2)?,
        physical_bytes: uint(&items, 3)?,
        reusable_free_pages: uint(&items, 4)?,
        candidate_dead_pages: uint(&items, 5)?,
        last_validated_mark_epoch: uint(&items, 6)?,
        touched_segments: uint(&items, 7)?,
        candidate_segments: uint(&items, 8)?,
        segment_overflow: uint(&items, 9)?,
    })
}

/// The result of `store_policy_get`/`store_policy_set` (`loom.store.policy.v1`). `audit_seq` is present
/// after a `set` (the audit sequence assigned to the mutation) and absent for a `get`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorePolicyResult {
    pub fips_required: bool,
    pub audit_seq: Option<u64>,
}

/// Encode a [`StorePolicyResult`] as canonical CBOR `[fips_required, audit_seq|null]`.
pub fn store_policy_result_to_cbor(result: &StorePolicyResult) -> Vec<u8> {
    enc(CborValue::Array(vec![
        CborValue::Bool(result.fips_required),
        result.audit_seq.map_or(CborValue::Null, CborValue::Uint),
    ]))
}

/// Decode a [`StorePolicyResult`] CBOR array.
pub fn store_policy_result_from_cbor(bytes: &[u8]) -> Result<StorePolicyResult, LoomError> {
    let items = arr(bytes)?;
    Ok(StorePolicyResult {
        fips_required: boolean(&items, 0)?,
        audit_seq: opt_uint(&items, 1)?,
    })
}

/// The result of `store_rekey` (`loom.store.rekey.v1`): the audit sequence, whether every object was
/// re-sealed under a fresh DEK, the active AEAD suite, and the reseal byte deltas (present only for a
/// reseal). No key material is ever included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRekeyResult {
    pub audit_seq: u64,
    pub resealed: bool,
    pub suite: String,
    pub bytes_before: Option<u64>,
    pub bytes_after: Option<u64>,
}

/// Encode a [`StoreRekeyResult`] as canonical CBOR `[audit_seq, resealed, suite, before|null, after|null]`.
pub fn store_rekey_result_to_cbor(result: &StoreRekeyResult) -> Vec<u8> {
    enc(CborValue::Array(vec![
        CborValue::Uint(result.audit_seq),
        CborValue::Bool(result.resealed),
        CborValue::Text(result.suite.clone()),
        result.bytes_before.map_or(CborValue::Null, CborValue::Uint),
        result.bytes_after.map_or(CborValue::Null, CborValue::Uint),
    ]))
}

/// Decode a [`StoreRekeyResult`] CBOR array.
pub fn store_rekey_result_from_cbor(bytes: &[u8]) -> Result<StoreRekeyResult, LoomError> {
    let items = arr(bytes)?;
    Ok(StoreRekeyResult {
        audit_seq: uint(&items, 0)?,
        resealed: boolean(&items, 1)?,
        suite: text(&items, 2)?,
        bytes_before: opt_uint(&items, 3)?,
        bytes_after: opt_uint(&items, 4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_stat_round_trips() {
        let stat = StoreStat {
            object_count: 42,
            generation: 3,
            physical_page_count: 100,
            physical_bytes: 4096,
            reusable_free_pages: 5,
            candidate_dead_pages: 2,
            last_validated_mark_epoch: 7,
            touched_segments: 1,
            candidate_segments: 0,
            segment_overflow: 0,
        };
        assert_eq!(
            store_stat_from_cbor(&store_stat_to_cbor(&stat)).unwrap(),
            stat
        );
    }

    #[test]
    fn store_policy_result_round_trips_get_and_set() {
        let get = StorePolicyResult {
            fips_required: true,
            audit_seq: None,
        };
        let set = StorePolicyResult {
            fips_required: false,
            audit_seq: Some(9),
        };
        assert_eq!(
            store_policy_result_from_cbor(&store_policy_result_to_cbor(&get)).unwrap(),
            get
        );
        assert_eq!(
            store_policy_result_from_cbor(&store_policy_result_to_cbor(&set)).unwrap(),
            set
        );
    }

    #[test]
    fn store_rekey_result_round_trips_fast_and_reseal() {
        let fast = StoreRekeyResult {
            audit_seq: 1,
            resealed: false,
            suite: "xchacha20poly1305".to_string(),
            bytes_before: None,
            bytes_after: None,
        };
        let reseal = StoreRekeyResult {
            audit_seq: 2,
            resealed: true,
            suite: "aes256gcm".to_string(),
            bytes_before: Some(1000),
            bytes_after: Some(1024),
        };
        assert_eq!(
            store_rekey_result_from_cbor(&store_rekey_result_to_cbor(&fast)).unwrap(),
            fast
        );
        assert_eq!(
            store_rekey_result_from_cbor(&store_rekey_result_to_cbor(&reseal)).unwrap(),
            reseal
        );
    }

    #[test]
    fn rejects_non_array() {
        let bad = encode(&CborValue::Uint(1)).unwrap();
        assert_eq!(
            store_stat_from_cbor(&bad).unwrap_err().code,
            Code::InvalidArgument
        );
    }
}
