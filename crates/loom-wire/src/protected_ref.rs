//! Canonical wire codec for the protected-ref admin facet. A [`ProtectedRefPolicy`] crosses as the
//! CBOR array `[fast_forward_only, signed_commits_required, signed_ref_advance_required,
//! required_review_count, retention_lock, governance_lock]`. The list form
//! ([`named_protected_ref_policy_to_cbor`]) prepends the ref name, mirroring the FFI JSON shape which
//! carries the `"ref"` field.

use loom_codec::{Value as CborValue, encode};
use loom_core::ProtectedRefPolicy;
use loom_types::{Code, LoomError};

fn policy_fields(policy: &ProtectedRefPolicy) -> Vec<CborValue> {
    vec![
        CborValue::Bool(policy.fast_forward_only),
        CborValue::Bool(policy.signed_commits_required),
        CborValue::Bool(policy.signed_ref_advance_required),
        CborValue::Uint(u64::from(policy.required_review_count)),
        CborValue::Bool(policy.retention_lock),
        CborValue::Bool(policy.governance_lock),
    ]
}

fn encode_array(items: Vec<CborValue>) -> Result<Vec<u8>, LoomError> {
    encode(&CborValue::Array(items))
        .map_err(|err| LoomError::new(Code::CorruptObject, format!("cbor: {err}")))
}

/// Encode a [`ProtectedRefPolicy`] as the canonical CBOR array of its six fields (the `protected_ref_get`
/// wire form).
pub fn protected_ref_policy_to_cbor(policy: &ProtectedRefPolicy) -> Result<Vec<u8>, LoomError> {
    encode_array(policy_fields(policy))
}

/// Encode a named protected-ref policy as `[ref_name, ..policy fields]` (the `protected_ref_list` wire
/// form for one entry).
pub fn named_protected_ref_policy_to_cbor(
    ref_name: &str,
    policy: &ProtectedRefPolicy,
) -> Result<Vec<u8>, LoomError> {
    let mut items = policy_fields(policy);
    items.insert(0, CborValue::Text(ref_name.to_string()));
    encode_array(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ProtectedRefPolicy {
        ProtectedRefPolicy {
            fast_forward_only: true,
            signed_commits_required: false,
            signed_ref_advance_required: true,
            required_review_count: 2,
            retention_lock: false,
            governance_lock: true,
        }
    }

    #[test]
    fn policy_encodes_as_six_field_array() {
        let CborValue::Array(items) =
            loom_codec::decode(&protected_ref_policy_to_cbor(&sample()).unwrap()).unwrap()
        else {
            panic!("expected array");
        };
        assert_eq!(
            items,
            vec![
                CborValue::Bool(true),
                CborValue::Bool(false),
                CborValue::Bool(true),
                CborValue::Uint(2),
                CborValue::Bool(false),
                CborValue::Bool(true),
            ]
        );
    }

    #[test]
    fn named_policy_prepends_ref_name() {
        let CborValue::Array(items) = loom_codec::decode(
            &named_protected_ref_policy_to_cbor("branch/main", &sample()).unwrap(),
        )
        .unwrap() else {
            panic!("expected array");
        };
        assert_eq!(items.len(), 7);
        assert_eq!(items[0], CborValue::Text("branch/main".to_string()));
        assert_eq!(items[4], CborValue::Uint(2));
    }
}
