//! Typed errors with stable, machine-readable codes.

use loom_codec::Value;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Code {
    NotFound,
    AlreadyExists,
    CorruptObject,
    IntegrityFailure,
    Unsupported,
    InvalidArgument,
    Io,
    Internal,
    CrossWorkspace,
    CasMismatch,
    NotFastForward,
    DimensionMismatch,
    PermissionDenied,
    AuthenticationFailed,
    IdentityNoRootCredential,
    TriggerNotFound,
    TriggerDenied,
    CursorInvalid,
    E2eLocked,
    E2eKeyInvalid,
    Conflict,
    Locked,
    LockLeaseExpired,
    FencingStale,
    LockNotHeld,
    NoSuchField,
    QueryParseError,
    SqlSyntax,
    SqlConstraintViolation,
    SqlTableNotFound,
    SqlTypeMismatch,
    SqlExecutionFailed,
    ResourceExhausted,
    IndexNotReady,
    DocumentNotText,
    Unavailable,
    RetainedGap,
}

impl Code {
    pub const fn as_str(self) -> &'static str {
        match self {
            Code::NotFound => "NOT_FOUND",
            Code::AlreadyExists => "ALREADY_EXISTS",
            Code::CorruptObject => "CORRUPT_OBJECT",
            Code::IntegrityFailure => "INTEGRITY_FAILURE",
            Code::Unsupported => "UNSUPPORTED",
            Code::InvalidArgument => "INVALID_ARGUMENT",
            Code::Io => "IO",
            Code::Internal => "INTERNAL",
            Code::CrossWorkspace => "CROSS_WORKSPACE",
            Code::CasMismatch => "CAS_MISMATCH",
            Code::NotFastForward => "NOT_FAST_FORWARD",
            Code::DimensionMismatch => "DIMENSION_MISMATCH",
            Code::PermissionDenied => "PERMISSION_DENIED",
            Code::AuthenticationFailed => "AUTHENTICATION_FAILED",
            Code::IdentityNoRootCredential => "IDENTITY_NO_ROOT_CREDENTIAL",
            Code::TriggerNotFound => "TRIGGER_NOT_FOUND",
            Code::TriggerDenied => "TRIGGER_DENIED",
            Code::CursorInvalid => "CURSOR_INVALID",
            Code::E2eLocked => "E2E_LOCKED",
            Code::E2eKeyInvalid => "E2E_KEY_INVALID",
            Code::Conflict => "CONFLICT",
            Code::Locked => "LOCKED",
            Code::LockLeaseExpired => "LOCK_LEASE_EXPIRED",
            Code::FencingStale => "FENCING_STALE",
            Code::LockNotHeld => "LOCK_NOT_HELD",
            Code::NoSuchField => "NO_SUCH_FIELD",
            Code::QueryParseError => "QUERY_PARSE_ERROR",
            Code::SqlSyntax => "SQL_SYNTAX",
            Code::SqlConstraintViolation => "SQL_CONSTRAINT_VIOLATION",
            Code::SqlTableNotFound => "SQL_TABLE_NOT_FOUND",
            Code::SqlTypeMismatch => "SQL_TYPE_MISMATCH",
            Code::SqlExecutionFailed => "SQL_EXECUTION_FAILED",
            Code::ResourceExhausted => "RESOURCE_EXHAUSTED",
            Code::IndexNotReady => "INDEX_NOT_READY",
            Code::DocumentNotText => "DOCUMENT_NOT_TEXT",
            Code::Unavailable => "UNAVAILABLE",
            Code::RetainedGap => "RETAINED_GAP",
        }
    }

    pub const fn as_i32(self) -> i32 {
        match self {
            Code::NotFound => 1,
            Code::AlreadyExists => 2,
            Code::CorruptObject => 3,
            Code::IntegrityFailure => 4,
            Code::Unsupported => 5,
            Code::InvalidArgument => 6,
            Code::Io => 7,
            Code::Internal => 8,
            Code::CrossWorkspace => 9,
            Code::CasMismatch => 10,
            Code::NotFastForward => 11,
            Code::DimensionMismatch => 12,
            Code::PermissionDenied => 13,
            Code::AuthenticationFailed => 14,
            Code::IdentityNoRootCredential => 15,
            Code::TriggerNotFound => 16,
            Code::TriggerDenied => 17,
            Code::CursorInvalid => 18,
            Code::E2eLocked => 19,
            Code::E2eKeyInvalid => 20,
            Code::Conflict => 21,
            Code::Locked => 22,
            Code::LockLeaseExpired => 23,
            Code::FencingStale => 24,
            Code::LockNotHeld => 25,
            Code::NoSuchField => 26,
            Code::QueryParseError => 27,
            Code::SqlSyntax => 28,
            Code::SqlConstraintViolation => 29,
            Code::SqlTableNotFound => 30,
            Code::SqlTypeMismatch => 31,
            Code::SqlExecutionFailed => 32,
            Code::ResourceExhausted => 33,
            Code::IndexNotReady => 34,
            Code::DocumentNotText => 35,
            Code::Unavailable => 36,
            Code::RetainedGap => 37,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorDetail {
    InvalidField {
        field: String,
        rejected: Option<String>,
        accepted: Vec<String>,
    },
    MissingResource {
        resource: String,
        id: String,
    },
    OptimisticConflict {
        resource: String,
        id: String,
        expected_root: Option<String>,
        current_root: Option<String>,
    },
    Retry {
        retryable: bool,
        retry_after_ms: Option<u64>,
    },
    Remediation {
        action: String,
        hint: String,
    },
}

impl ErrorDetail {
    pub fn invalid_field(
        field: impl Into<String>,
        rejected: Option<impl Into<String>>,
        accepted: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self::InvalidField {
            field: field.into(),
            rejected: rejected.map(Into::into),
            accepted: accepted.into_iter().map(Into::into).collect(),
        }
    }

    pub fn missing_resource(resource: impl Into<String>, id: impl Into<String>) -> Self {
        Self::MissingResource {
            resource: resource.into(),
            id: id.into(),
        }
    }

    pub fn optimistic_conflict(
        resource: impl Into<String>,
        id: impl Into<String>,
        expected_root: Option<impl Into<String>>,
        current_root: Option<impl Into<String>>,
    ) -> Self {
        Self::OptimisticConflict {
            resource: resource.into(),
            id: id.into(),
            expected_root: expected_root.map(Into::into),
            current_root: current_root.map(Into::into),
        }
    }

    pub fn retry(retryable: bool, retry_after_ms: Option<u64>) -> Self {
        Self::Retry {
            retryable,
            retry_after_ms,
        }
    }

    pub fn remediation(action: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::Remediation {
            action: action.into(),
            hint: hint.into(),
        }
    }

    fn to_cbor(&self) -> Value {
        match self {
            Self::InvalidField {
                field,
                rejected,
                accepted,
            } => Value::Map(vec![
                text_pair("kind", "invalid_field"),
                text_pair("field", field),
                optional_text_pair("rejected", rejected.as_deref()),
                (
                    Value::Text("accepted".to_string()),
                    Value::Array(
                        accepted
                            .iter()
                            .map(|value| Value::Text(value.clone()))
                            .collect(),
                    ),
                ),
            ]),
            Self::MissingResource { resource, id } => Value::Map(vec![
                text_pair("kind", "missing_resource"),
                text_pair("resource", resource),
                text_pair("id", id),
            ]),
            Self::OptimisticConflict {
                resource,
                id,
                expected_root,
                current_root,
            } => Value::Map(vec![
                text_pair("kind", "optimistic_conflict"),
                text_pair("resource", resource),
                text_pair("id", id),
                optional_text_pair("expected_root", expected_root.as_deref()),
                optional_text_pair("current_root", current_root.as_deref()),
            ]),
            Self::Retry {
                retryable,
                retry_after_ms,
            } => Value::Map(vec![
                text_pair("kind", "retry"),
                (
                    Value::Text("retryable".to_string()),
                    Value::Bool(*retryable),
                ),
                (
                    Value::Text("retry_after_ms".to_string()),
                    retry_after_ms.map(Value::Uint).unwrap_or(Value::Null),
                ),
            ]),
            Self::Remediation { action, hint } => Value::Map(vec![
                text_pair("kind", "remediation"),
                text_pair("action", action),
                text_pair("hint", hint),
            ]),
        }
    }

    fn from_cbor(value: Value) -> std::result::Result<Self, String> {
        let Value::Map(fields) = value else {
            return Err("error detail must be a map".to_string());
        };
        let kind = text_field(&fields, "kind")?;
        match kind.as_str() {
            "invalid_field" => Ok(Self::InvalidField {
                field: text_field(&fields, "field")?,
                rejected: optional_text_field(&fields, "rejected")?,
                accepted: text_array_field(&fields, "accepted")?,
            }),
            "missing_resource" => Ok(Self::MissingResource {
                resource: text_field(&fields, "resource")?,
                id: text_field(&fields, "id")?,
            }),
            "optimistic_conflict" => Ok(Self::OptimisticConflict {
                resource: text_field(&fields, "resource")?,
                id: text_field(&fields, "id")?,
                expected_root: optional_text_field(&fields, "expected_root")?,
                current_root: optional_text_field(&fields, "current_root")?,
            }),
            "retry" => Ok(Self::Retry {
                retryable: bool_field(&fields, "retryable")?,
                retry_after_ms: optional_uint_field(&fields, "retry_after_ms")?,
            }),
            "remediation" => Ok(Self::Remediation {
                action: text_field(&fields, "action")?,
                hint: text_field(&fields, "hint")?,
            }),
            _ => Err("unsupported error detail kind".to_string()),
        }
    }
}

fn text_pair(key: &str, value: &str) -> (Value, Value) {
    (Value::Text(key.to_string()), Value::Text(value.to_string()))
}

fn optional_text_pair(key: &str, value: Option<&str>) -> (Value, Value) {
    (
        Value::Text(key.to_string()),
        value
            .map(|value| Value::Text(value.to_string()))
            .unwrap_or(Value::Null),
    )
}

fn field<'a>(fields: &'a [(Value, Value)], key: &str) -> Option<&'a Value> {
    fields
        .iter()
        .find_map(|(field_key, value)| match field_key {
            Value::Text(field_key) if field_key == key => Some(value),
            _ => None,
        })
}

fn text_field(fields: &[(Value, Value)], key: &str) -> std::result::Result<String, String> {
    match field(fields, key) {
        Some(Value::Text(value)) => Ok(value.clone()),
        _ => Err(format!("{key} must be text")),
    }
}

fn optional_text_field(
    fields: &[(Value, Value)],
    key: &str,
) -> std::result::Result<Option<String>, String> {
    match field(fields, key) {
        Some(Value::Text(value)) => Ok(Some(value.clone())),
        Some(Value::Null) | None => Ok(None),
        _ => Err(format!("{key} must be text or null")),
    }
}

fn optional_uint_field(
    fields: &[(Value, Value)],
    key: &str,
) -> std::result::Result<Option<u64>, String> {
    match field(fields, key) {
        Some(Value::Uint(value)) => Ok(Some(*value)),
        Some(Value::Null) | None => Ok(None),
        _ => Err(format!("{key} must be uint or null")),
    }
}

fn bool_field(fields: &[(Value, Value)], key: &str) -> std::result::Result<bool, String> {
    match field(fields, key) {
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(format!("{key} must be bool")),
    }
}

fn text_array_field(
    fields: &[(Value, Value)],
    key: &str,
) -> std::result::Result<Vec<String>, String> {
    let Some(Value::Array(values)) = field(fields, key) else {
        return Err(format!("{key} must be an array"));
    };
    values
        .iter()
        .map(|value| match value {
            Value::Text(value) => Ok(value.clone()),
            _ => Err(format!("{key} entries must be text")),
        })
        .collect()
}

#[derive(Debug, Clone, Error)]
#[error("{}: {message}", code.as_str())]
pub struct LoomError {
    pub code: Code,
    pub message: String,
    pub details: Vec<ErrorDetail>,
}

impl LoomError {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Vec::new(),
        }
    }

    pub fn with_detail(mut self, detail: ErrorDetail) -> Self {
        self.details.push(detail);
        self
    }

    pub fn with_details(mut self, details: impl IntoIterator<Item = ErrorDetail>) -> Self {
        self.details.extend(details);
        self
    }

    pub fn details_cbor(&self) -> Option<Vec<u8>> {
        if self.details.is_empty() {
            None
        } else {
            loom_codec::encode(&Value::Array(
                self.details.iter().map(ErrorDetail::to_cbor).collect(),
            ))
            .ok()
        }
    }

    pub fn details_from_cbor(bytes: &[u8]) -> std::result::Result<Vec<ErrorDetail>, String> {
        let value = loom_codec::decode(bytes).map_err(|error| error.to_string())?;
        let Value::Array(items) = value else {
            return Err("error details must be an array".to_string());
        };
        items.into_iter().map(ErrorDetail::from_cbor).collect()
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(Code::NotFound, message)
    }

    pub fn corrupt(message: impl Into<String>) -> Self {
        Self::new(Code::CorruptObject, message)
    }

    pub fn integrity_failure(message: impl Into<String>) -> Self {
        Self::new(Code::IntegrityFailure, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(Code::InvalidArgument, message)
    }

    pub fn no_such_field(message: impl Into<String>) -> Self {
        Self::new(Code::NoSuchField, message)
    }

    pub fn query_parse_error(message: impl Into<String>) -> Self {
        Self::new(Code::QueryParseError, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(Code::Unsupported, message)
    }

    pub fn index_not_ready(message: impl Into<String>) -> Self {
        Self::new(Code::IndexNotReady, message)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(Code::Unavailable, message)
    }

    pub fn retained_gap(message: impl Into<String>) -> Self {
        Self::new(Code::RetainedGap, message)
    }

    pub fn cursor_invalid(message: impl Into<String>) -> Self {
        Self::new(Code::CursorInvalid, message)
    }

    pub fn document_not_text(message: impl Into<String>) -> Self {
        Self::new(Code::DocumentNotText, message)
    }

    pub fn cross_workspace(message: impl Into<String>) -> Self {
        Self::new(Code::CrossWorkspace, message)
    }

    pub fn cas_mismatch(message: impl Into<String>) -> Self {
        Self::new(Code::CasMismatch, message)
    }

    pub fn not_fast_forward(message: impl Into<String>) -> Self {
        Self::new(Code::NotFastForward, message)
    }

    pub fn dimension_mismatch(message: impl Into<String>) -> Self {
        Self::new(Code::DimensionMismatch, message)
    }

    pub fn locked(message: impl Into<String>) -> Self {
        Self::new(Code::Locked, message)
    }

    pub fn lock_lease_expired(message: impl Into<String>) -> Self {
        Self::new(Code::LockLeaseExpired, message)
    }

    pub fn fencing_stale(message: impl Into<String>) -> Self {
        Self::new(Code::FencingStale, message)
    }

    pub fn lock_not_held(message: impl Into<String>) -> Self {
        Self::new(Code::LockNotHeld, message)
    }
}

impl From<std::io::Error> for LoomError {
    fn from(e: std::io::Error) -> Self {
        Self::new(Code::Io, e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, LoomError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_is_a_first_class_stable_code() {
        // Token and discriminant are part of the public stable-error contract (0010 §5.1).
        assert_eq!(Code::Unavailable.as_str(), "UNAVAILABLE");
        assert_eq!(Code::Unavailable.as_i32(), 36);
        // Adding it must not disturb the existing generic transient code.
        assert_eq!(Code::IndexNotReady.as_str(), "INDEX_NOT_READY");
        assert_eq!(Code::IndexNotReady.as_i32(), 34);
        // Constructor parity.
        assert_eq!(LoomError::unavailable("down").code, Code::Unavailable);
    }

    #[test]
    fn retained_gap_is_a_first_class_stable_code() {
        assert_eq!(Code::RetainedGap.as_str(), "RETAINED_GAP");
        assert_eq!(Code::RetainedGap.as_i32(), 37);
        assert_eq!(Code::Unavailable.as_i32(), 36);
        assert_eq!(
            LoomError::retained_gap("full resync").code,
            Code::RetainedGap
        );
        assert_eq!(
            LoomError::cursor_invalid("bad token").code,
            Code::CursorInvalid
        );
    }

    #[test]
    fn structured_details_round_trip_as_canonical_cbor() {
        let err = LoomError::invalid("ticket status rejected").with_details([
            ErrorDetail::invalid_field("target_status", Some("accepted"), ["ready"]),
            ErrorDetail::optimistic_conflict(
                "ticket",
                "MX-1",
                Some("blake3:old"),
                Some("blake3:new"),
            ),
            ErrorDetail::retry(true, Some(250)),
            ErrorDetail::remediation("re_read", "Fetch the current ticket root and retry."),
        ]);

        let bytes = err.details_cbor().expect("details cbor");
        let decoded = LoomError::details_from_cbor(&bytes).expect("decode details");
        assert_eq!(decoded, err.details);
    }
}
