//! Canonical wire codecs for the coordination-lock facet.
//!
//! A lock `mode` crosses as exactly one stable byte: `0` Exclusive, `1` Shared, `2` Semaphore. For
//! Exclusive/Shared the wire `permits`/`capacity` must both be the canonical `1`; a Semaphore carries
//! its own `permits`/`capacity` (validated by `loom_core`). A [`LockToken`] crosses as the CBOR array
//! `[key text, principal text, session text, mode_tag uint, permits uint, capacity uint,
//! fence_authority uint, fence_epoch uint, fence_sequence uint, lease_deadline_ms uint]`.

use loom_codec::{Value as CborValue, encode};
use loom_core::{LockMode, LockOwner, LockToken};
use loom_types::{Code, Fence, LoomError};

const MODE_EXCLUSIVE: u8 = 0;
const MODE_SHARED: u8 = 1;
const MODE_SEMAPHORE: u8 = 2;

fn require_single_permit(permits: u32, capacity: u32) -> Result<(), LoomError> {
    if permits != 1 || capacity != 1 {
        return Err(LoomError::invalid(
            "exclusive/shared lock requires permits=1 and capacity=1",
        ));
    }
    Ok(())
}

/// Decode the one-byte `mode` wire atom into a [`LockMode`], folding in the wire `permits`/`capacity`.
pub fn lock_mode_from_wire(
    bytes: &[u8],
    permits: u32,
    capacity: u32,
) -> Result<LockMode, LoomError> {
    let tag = match bytes {
        [tag] => *tag,
        _ => return Err(LoomError::invalid("lock mode must be exactly one byte")),
    };
    match tag {
        MODE_EXCLUSIVE => {
            require_single_permit(permits, capacity)?;
            Ok(LockMode::Exclusive)
        }
        MODE_SHARED => {
            require_single_permit(permits, capacity)?;
            Ok(LockMode::Shared)
        }
        MODE_SEMAPHORE => Ok(LockMode::Semaphore { permits, capacity }),
        other => Err(LoomError::invalid(format!("unknown lock mode {other}"))),
    }
}

/// The `(mode_tag, permits, capacity)` wire triple for a [`LockMode`].
fn mode_parts(mode: &LockMode) -> (u8, u32, u32) {
    match mode {
        LockMode::Exclusive => (MODE_EXCLUSIVE, 1, 1),
        LockMode::Shared => (MODE_SHARED, 1, 1),
        LockMode::Semaphore { permits, capacity } => (MODE_SEMAPHORE, *permits, *capacity),
    }
}

/// Reconstruct a [`LockToken`] from the wire-typed `lock_refresh`/`lock_release` arguments. Holder
/// identity is `(key, owner, mode, fence)`, so `lease_deadline_ms` is not part of it and is set to `0`.
pub fn lock_token_from_wire(
    key: String,
    principal: String,
    session: String,
    mode: &[u8],
    permits: u32,
    capacity: u32,
    fence_low: u64,
    fence_high: u64,
) -> Result<LockToken, LoomError> {
    Ok(LockToken {
        key: key.into_bytes(),
        owner: LockOwner { principal, session },
        mode: lock_mode_from_wire(mode, permits, capacity)?,
        fence: Fence::from_limbs(fence_low, fence_high),
        lease_deadline_ms: 0,
    })
}

/// Encode a [`LockToken`] as the canonical CBOR array `[key, principal, session, mode_tag, permits,
/// capacity, fence_authority, fence_epoch, fence_sequence, lease_deadline_ms]`.
pub fn lock_token_to_cbor(token: &LockToken) -> Result<Vec<u8>, LoomError> {
    let key = String::from_utf8(token.key.clone())
        .map_err(|_| LoomError::new(Code::CorruptObject, "lock key is not valid utf-8"))?;
    let (mode_tag, permits, capacity) = mode_parts(&token.mode);
    encode(&CborValue::Array(vec![
        CborValue::Text(key),
        CborValue::Text(token.owner.principal.clone()),
        CborValue::Text(token.owner.session.clone()),
        CborValue::Uint(u64::from(mode_tag)),
        CborValue::Uint(u64::from(permits)),
        CborValue::Uint(u64::from(capacity)),
        CborValue::Uint(u64::from(token.fence.authority())),
        CborValue::Uint(u64::from(token.fence.epoch())),
        CborValue::Uint(token.fence.sequence()),
        CborValue::Uint(token.lease_deadline_ms),
    ]))
    .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_atom_decode_errors() {
        assert_eq!(
            lock_mode_from_wire(&[], 1, 1).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            lock_mode_from_wire(&[0, 1], 1, 1).unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            lock_mode_from_wire(&[9], 1, 1).unwrap_err().code,
            Code::InvalidArgument
        );
        // Exclusive/Shared reject non-canonical permits/capacity.
        assert_eq!(
            lock_mode_from_wire(&[MODE_EXCLUSIVE], 2, 2)
                .unwrap_err()
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            lock_mode_from_wire(&[MODE_SHARED], 1, 2).unwrap_err().code,
            Code::InvalidArgument
        );
    }

    #[test]
    fn mode_atom_decodes_canonical_and_semaphore() {
        assert_eq!(
            lock_mode_from_wire(&[MODE_EXCLUSIVE], 1, 1).unwrap(),
            LockMode::Exclusive
        );
        assert_eq!(
            lock_mode_from_wire(&[MODE_SHARED], 1, 1).unwrap(),
            LockMode::Shared
        );
        assert_eq!(
            lock_mode_from_wire(&[MODE_SEMAPHORE], 3, 5).unwrap(),
            LockMode::Semaphore {
                permits: 3,
                capacity: 5,
            }
        );
    }

    fn decode(bytes: &[u8]) -> Vec<CborValue> {
        let CborValue::Array(items) = loom_codec::decode(bytes).unwrap() else {
            panic!("expected array");
        };
        items
    }

    #[test]
    fn token_encodes_exclusive() {
        let token = LockToken {
            key: b"resource/a".to_vec(),
            owner: LockOwner {
                principal: "p".to_string(),
                session: "s".to_string(),
            },
            mode: LockMode::Exclusive,
            fence: Fence::embedded(7),
            lease_deadline_ms: 1234,
        };
        assert_eq!(
            decode(&lock_token_to_cbor(&token).unwrap()),
            vec![
                CborValue::Text("resource/a".to_string()),
                CborValue::Text("p".to_string()),
                CborValue::Text("s".to_string()),
                CborValue::Uint(0),
                CborValue::Uint(1),
                CborValue::Uint(1),
                CborValue::Uint(0),
                CborValue::Uint(0),
                CborValue::Uint(7),
                CborValue::Uint(1234),
            ]
        );
    }

    #[test]
    fn token_encodes_semaphore_permits_and_capacity() {
        let token = LockToken {
            key: b"sem".to_vec(),
            owner: LockOwner {
                principal: "p".to_string(),
                session: "s".to_string(),
            },
            mode: LockMode::Semaphore {
                permits: 2,
                capacity: 4,
            },
            fence: Fence::embedded(0),
            lease_deadline_ms: 0,
        };
        let items = decode(&lock_token_to_cbor(&token).unwrap());
        assert_eq!(items[3], CborValue::Uint(u64::from(MODE_SEMAPHORE)));
        assert_eq!(items[4], CborValue::Uint(2));
        assert_eq!(items[5], CborValue::Uint(4));
    }
}
