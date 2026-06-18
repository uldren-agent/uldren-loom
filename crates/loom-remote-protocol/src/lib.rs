//! Loom Remote Protocol: stable error mapping.
//!
//! The wire error object carries the stable Loom error `Code` verbatim (its SCREAMING_SNAKE_CASE
//! spelling from [`loom_types::Code::as_str`]) plus retry advice. This module maps a wire error to and
//! from [`loom_types::LoomError`] without renaming, collapsing, or repurposing any code, and preserves
//! an unrecognized future code so a newer server's error is never silently dropped.
//!
//! Licensed under BUSL-1.1.

pub mod api_types;
pub mod codec;
pub mod discovery;
pub mod envelope;
pub mod frame;
pub mod generated;
pub mod generated_api;
pub mod session;

use loom_types::{Code, LoomError};

/// Retry advice attached to a remote error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RetryAdvice {
    /// The request must not be retried.
    #[default]
    Never,
    /// The request may be retried after `retry_after_ms`.
    After,
    /// The request may be retried only with the same idempotency key.
    SameIdempotencyKey,
}

impl RetryAdvice {
    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            RetryAdvice::Never => "never",
            RetryAdvice::After => "after",
            RetryAdvice::SameIdempotencyKey => "same_idempotency_key",
        }
    }

    /// Parse the wire spelling; unknown values are rejected.
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "never" => Some(RetryAdvice::Never),
            "after" => Some(RetryAdvice::After),
            "same_idempotency_key" => Some(RetryAdvice::SameIdempotencyKey),
            _ => None,
        }
    }
}

/// A decoded remote error object. `wire_code` is preserved verbatim so an unrecognized future code
/// survives; `code` is the resolved stable code, falling back to [`Code::Internal`] when unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteError {
    /// The wire code spelling as received.
    pub wire_code: String,
    /// The resolved stable code (`Internal` when `wire_code` is unrecognized).
    pub code: Code,
    /// The human-readable message.
    pub message: String,
    /// Retry advice.
    pub retry: RetryAdvice,
    /// Milliseconds to wait before retrying, when `retry` is [`RetryAdvice::After`].
    pub retry_after_ms: Option<u64>,
    /// Opaque protocol-defined detail bytes.
    pub details: Option<Vec<u8>>,
}

impl RemoteError {
    /// Build from a received wire error object.
    pub fn from_wire(
        wire_code: impl Into<String>,
        message: impl Into<String>,
        retry: RetryAdvice,
        retry_after_ms: Option<u64>,
        details: Option<Vec<u8>>,
    ) -> Self {
        let wire_code = wire_code.into();
        let code = code_from_wire(&wire_code).unwrap_or(Code::Internal);
        Self {
            wire_code,
            code,
            message: message.into(),
            retry,
            retry_after_ms,
            details,
        }
    }

    /// Build a wire error object from a local error, defaulting to no retry.
    pub fn from_loom_error(err: &LoomError) -> Self {
        Self {
            wire_code: err.code.as_str().to_string(),
            code: err.code,
            message: err.message.clone(),
            retry: RetryAdvice::Never,
            retry_after_ms: None,
            details: err.details_cbor(),
        }
    }

    /// Convert to a local error. An unrecognized wire code keeps its spelling in the message so no
    /// information is lost.
    pub fn to_loom_error(&self) -> LoomError {
        let details = self
            .details
            .as_deref()
            .and_then(|details| LoomError::details_from_cbor(details).ok());
        if code_from_wire(&self.wire_code).is_some() {
            LoomError::new(self.code, self.message.clone())
                .with_details(details.unwrap_or_default())
        } else {
            LoomError::new(
                Code::Internal,
                format!(
                    "unrecognized remote code `{}`: {}",
                    self.wire_code, self.message
                ),
            )
            .with_details(details.unwrap_or_default())
        }
    }
}

/// Map a wire code spelling back to the stable [`Code`]. The forward direction is
/// [`loom_types::Code::as_str`]. Returns `None` for an unrecognized code.
pub fn code_from_wire(wire: &str) -> Option<Code> {
    let code = match wire {
        "NOT_FOUND" => Code::NotFound,
        "ALREADY_EXISTS" => Code::AlreadyExists,
        "CORRUPT_OBJECT" => Code::CorruptObject,
        "INTEGRITY_FAILURE" => Code::IntegrityFailure,
        "UNSUPPORTED" => Code::Unsupported,
        "INVALID_ARGUMENT" => Code::InvalidArgument,
        "IO" => Code::Io,
        "INTERNAL" => Code::Internal,
        "CROSS_WORKSPACE" => Code::CrossWorkspace,
        "CAS_MISMATCH" => Code::CasMismatch,
        "NOT_FAST_FORWARD" => Code::NotFastForward,
        "DIMENSION_MISMATCH" => Code::DimensionMismatch,
        "PERMISSION_DENIED" => Code::PermissionDenied,
        "AUTHENTICATION_FAILED" => Code::AuthenticationFailed,
        "IDENTITY_NO_ROOT_CREDENTIAL" => Code::IdentityNoRootCredential,
        "TRIGGER_NOT_FOUND" => Code::TriggerNotFound,
        "TRIGGER_DENIED" => Code::TriggerDenied,
        "CURSOR_INVALID" => Code::CursorInvalid,
        "E2E_LOCKED" => Code::E2eLocked,
        "E2E_KEY_INVALID" => Code::E2eKeyInvalid,
        "CONFLICT" => Code::Conflict,
        "LOCKED" => Code::Locked,
        "LOCK_LEASE_EXPIRED" => Code::LockLeaseExpired,
        "FENCING_STALE" => Code::FencingStale,
        "LOCK_NOT_HELD" => Code::LockNotHeld,
        "NO_SUCH_FIELD" => Code::NoSuchField,
        "QUERY_PARSE_ERROR" => Code::QueryParseError,
        "SQL_SYNTAX" => Code::SqlSyntax,
        "SQL_CONSTRAINT_VIOLATION" => Code::SqlConstraintViolation,
        "SQL_TABLE_NOT_FOUND" => Code::SqlTableNotFound,
        "SQL_TYPE_MISMATCH" => Code::SqlTypeMismatch,
        "SQL_EXECUTION_FAILED" => Code::SqlExecutionFailed,
        "RESOURCE_EXHAUSTED" => Code::ResourceExhausted,
        "INDEX_NOT_READY" => Code::IndexNotReady,
        "DOCUMENT_NOT_TEXT" => Code::DocumentNotText,
        "UNAVAILABLE" => Code::Unavailable,
        "RETAINED_GAP" => Code::RetainedGap,
        _ => return None,
    };
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every stable code known at this revision. The i32 coverage assertion guards against an upstream
    /// addition landing without a wire mapping here.
    const ALL_CODES: &[Code] = &[
        Code::NotFound,
        Code::AlreadyExists,
        Code::CorruptObject,
        Code::IntegrityFailure,
        Code::Unsupported,
        Code::InvalidArgument,
        Code::Io,
        Code::Internal,
        Code::CrossWorkspace,
        Code::CasMismatch,
        Code::NotFastForward,
        Code::DimensionMismatch,
        Code::PermissionDenied,
        Code::AuthenticationFailed,
        Code::IdentityNoRootCredential,
        Code::TriggerNotFound,
        Code::TriggerDenied,
        Code::CursorInvalid,
        Code::E2eLocked,
        Code::E2eKeyInvalid,
        Code::Conflict,
        Code::Locked,
        Code::LockLeaseExpired,
        Code::FencingStale,
        Code::LockNotHeld,
        Code::NoSuchField,
        Code::QueryParseError,
        Code::SqlSyntax,
        Code::SqlConstraintViolation,
        Code::SqlTableNotFound,
        Code::SqlTypeMismatch,
        Code::SqlExecutionFailed,
        Code::ResourceExhausted,
        Code::IndexNotReady,
        Code::DocumentNotText,
        Code::Unavailable,
        Code::RetainedGap,
    ];

    #[test]
    fn every_code_round_trips_through_the_wire() {
        for &code in ALL_CODES {
            assert_eq!(
                code_from_wire(code.as_str()),
                Some(code),
                "wire round trip failed for {}",
                code.as_str()
            );
        }
    }

    #[test]
    fn code_list_covers_every_stable_discriminant() {
        let mut ids: Vec<i32> = ALL_CODES.iter().map(|c| c.as_i32()).collect();
        ids.sort_unstable();
        let expected: Vec<i32> = (1..=37).collect();
        assert_eq!(
            ids, expected,
            "ALL_CODES must cover stable discriminants 1..=37 exactly; an upstream Code change needs a wire mapping"
        );
    }

    #[test]
    fn unknown_wire_code_is_none_but_preserved() {
        assert_eq!(code_from_wire("SOME_FUTURE_CODE"), None);
        let err =
            RemoteError::from_wire("SOME_FUTURE_CODE", "boom", RetryAdvice::Never, None, None);
        assert_eq!(err.code, Code::Internal);
        assert_eq!(err.wire_code, "SOME_FUTURE_CODE");
        let mapped = err.to_loom_error();
        assert_eq!(mapped.code, Code::Internal);
        assert!(mapped.message.contains("SOME_FUTURE_CODE"));
    }

    #[test]
    fn remote_error_round_trips_a_loom_error() {
        let original = LoomError::new(Code::PermissionDenied, "no access");
        let wire = RemoteError::from_loom_error(&original);
        assert_eq!(wire.wire_code, "PERMISSION_DENIED");
        let back = wire.to_loom_error();
        assert_eq!(back.code, Code::PermissionDenied);
        assert_eq!(back.message, "no access");
    }

    #[test]
    fn remote_error_preserves_structured_details() {
        let local = LoomError::new(Code::Conflict, "stale ticket root").with_details([
            loom_types::ErrorDetail::optimistic_conflict(
                "ticket",
                "MX-260",
                Some("blake3:old"),
                Some("blake3:new"),
            ),
            loom_types::ErrorDetail::remediation("re_read", "Read the ticket and retry."),
        ]);

        let remote = RemoteError::from_loom_error(&local);
        assert!(remote.details.is_some());
        let round_trip = remote.to_loom_error();
        assert_eq!(round_trip.code, Code::Conflict);
        assert_eq!(round_trip.message, "stale ticket root");
        assert_eq!(round_trip.details, local.details);
    }

    #[test]
    fn retry_advice_round_trips() {
        for advice in [
            RetryAdvice::Never,
            RetryAdvice::After,
            RetryAdvice::SameIdempotencyKey,
        ] {
            assert_eq!(RetryAdvice::from_wire(advice.as_str()), Some(advice));
        }
        assert_eq!(RetryAdvice::from_wire("bogus"), None);
    }
}

#[cfg(test)]
mod registry_tests {
    use crate::generated::METHODS;
    use std::collections::BTreeSet;

    #[test]
    fn registry_covers_the_full_idl_surface() {
        assert_eq!(
            METHODS.len(),
            463,
            "generated registry must list every IDL method"
        );
        let interfaces: BTreeSet<&str> = METHODS.iter().map(|m| m.interface).collect();
        assert_eq!(
            interfaces.len(),
            50,
            "generated registry must cover every IDL interface"
        );
    }

    #[test]
    fn registry_includes_promoted_and_program_surfaces() {
        let has = |interface: &str, method: &str| {
            METHODS
                .iter()
                .any(|m| m.interface == interface && m.method == method)
        };
        assert!(has("Triggers", "trigger_put"));
        assert!(has("Triggers", "trigger_history"));
        assert!(has("Exec", "exec_cbor"));
        assert!(has("Kv", "get"));
        assert!(has("Sql", "sql_query"));
    }
}
