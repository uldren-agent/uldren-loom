use std::collections::BTreeSet;

use loom_codec::Value;
use loom_types::{Algo, Code, Digest, LoomError, Result};

pub const CONFLICT_RECORD_SCHEMA: &str = "loom.substrate.conflict.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionState {
    Open,
    Resolved,
    Superseded,
}

impl ResolutionState {
    const fn tag(self) -> u64 {
        match self {
            ResolutionState::Open => 0,
            ResolutionState::Resolved => 1,
            ResolutionState::Superseded => 2,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(ResolutionState::Open),
            1 => Ok(ResolutionState::Resolved),
            2 => Ok(ResolutionState::Superseded),
            other => Err(LoomError::corrupt(format!(
                "unknown conflict resolution state tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRecord {
    pub conflict_id: String,
    pub entity_id: String,
    pub base_version: String,
    pub losing_operation_id: String,
    pub winning_operation_id: String,
    pub field_or_region: Option<String>,
    pub resolution_state: ResolutionState,
}

impl ConflictRecord {
    pub fn new(
        algo: Algo,
        entity_id: impl Into<String>,
        base_version: impl Into<String>,
        losing_operation_id: impl Into<String>,
        winning_operation_id: impl Into<String>,
        field_or_region: Option<String>,
    ) -> Result<Self> {
        let entity_id = entity_id.into();
        let base_version = base_version.into();
        let losing_operation_id = losing_operation_id.into();
        let winning_operation_id = winning_operation_id.into();
        validate_text("entity_id", &entity_id)?;
        validate_text("base_version", &base_version)?;
        validate_text("losing_operation_id", &losing_operation_id)?;
        validate_text("winning_operation_id", &winning_operation_id)?;
        if let Some(field_or_region) = field_or_region.as_deref() {
            validate_text("field_or_region", field_or_region)?;
        }
        let conflict_id = derive_conflict_id(
            algo,
            &entity_id,
            &base_version,
            &losing_operation_id,
            &winning_operation_id,
            field_or_region.as_deref(),
        )?;
        Ok(Self {
            conflict_id,
            entity_id,
            base_version,
            losing_operation_id,
            winning_operation_id,
            field_or_region,
            resolution_state: ResolutionState::Open,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(CONFLICT_RECORD_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.conflict_id.clone()),
                Value::Text(self.entity_id.clone()),
                Value::Text(self.base_version.clone()),
                Value::Text(self.losing_operation_id.clone()),
                Value::Text(self.winning_operation_id.clone()),
                optional_text_value(self.field_or_region.as_deref()),
                Value::Uint(self.resolution_state.tag()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = array(value, "conflict record")?;
        expect_text(next(&mut outer, "conflict schema")?, CONFLICT_RECORD_SCHEMA)?;
        let mut fields = array(
            next(&mut outer, "conflict record fields")?,
            "conflict record",
        )?;
        end(&outer, "conflict record")?;
        let conflict_id = text(next(&mut fields, "conflict_id")?, "conflict_id")?;
        let entity_id = text(next(&mut fields, "entity_id")?, "entity_id")?;
        let base_version = text(next(&mut fields, "base_version")?, "base_version")?;
        let losing_operation_id = text(
            next(&mut fields, "losing_operation_id")?,
            "losing_operation_id",
        )?;
        let winning_operation_id = text(
            next(&mut fields, "winning_operation_id")?,
            "winning_operation_id",
        )?;
        let field_or_region = optional_text(next(&mut fields, "field_or_region")?)?;
        let resolution_state =
            ResolutionState::from_tag(uint(next(&mut fields, "resolution_state")?)?)?;
        end(&fields, "conflict record")?;
        validate_text("conflict_id", &conflict_id)?;
        validate_text("entity_id", &entity_id)?;
        validate_text("base_version", &base_version)?;
        validate_text("losing_operation_id", &losing_operation_id)?;
        validate_text("winning_operation_id", &winning_operation_id)?;
        if let Some(field_or_region) = field_or_region.as_deref() {
            validate_text("field_or_region", field_or_region)?;
        }
        Ok(Self {
            conflict_id,
            entity_id,
            base_version,
            losing_operation_id,
            winning_operation_id,
            field_or_region,
            resolution_state,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardedWritePolicy {
    MergeDisjointFields,
    RecordScalarConflict,
    RevalidateStateMachine,
    RejectStaleBase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedWriteInput {
    pub algo: Algo,
    pub policy: GuardedWritePolicy,
    pub entity_id: String,
    pub base_version: String,
    pub current_version: String,
    pub operation_id: String,
    pub current_operation_id: String,
    pub write_fields: Vec<String>,
    pub changed_fields_since_base: Vec<String>,
    pub revalidation_passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardedWriteDecision {
    Accept,
    AcceptWithConflict(ConflictRecord),
    Reject { code: Code, message: String },
}

pub fn evaluate_guarded_write(input: GuardedWriteInput) -> Result<GuardedWriteDecision> {
    validate_text("entity_id", &input.entity_id)?;
    validate_text("base_version", &input.base_version)?;
    validate_text("current_version", &input.current_version)?;
    validate_text("operation_id", &input.operation_id)?;
    validate_text("current_operation_id", &input.current_operation_id)?;
    let write_fields = canonical_field_set(&input.write_fields)?;
    let changed_fields = canonical_field_set(&input.changed_fields_since_base)?;
    if input.base_version == input.current_version {
        return Ok(GuardedWriteDecision::Accept);
    }
    match input.policy {
        GuardedWritePolicy::MergeDisjointFields => {
            if write_fields.is_disjoint(&changed_fields) {
                Ok(GuardedWriteDecision::Accept)
            } else {
                conflict_decision(
                    input.algo,
                    &input,
                    first_overlap(&write_fields, &changed_fields),
                )
            }
        }
        GuardedWritePolicy::RecordScalarConflict => conflict_decision(
            input.algo,
            &input,
            first_overlap(&write_fields, &changed_fields),
        ),
        GuardedWritePolicy::RevalidateStateMachine => {
            if input.revalidation_passed {
                Ok(GuardedWriteDecision::Accept)
            } else {
                Ok(reject_conflict(
                    "state-machine transition failed revalidation",
                ))
            }
        }
        GuardedWritePolicy::RejectStaleBase => {
            Ok(reject_conflict("entity version advanced past base"))
        }
    }
}

fn conflict_decision(
    algo: Algo,
    input: &GuardedWriteInput,
    field_or_region: Option<String>,
) -> Result<GuardedWriteDecision> {
    Ok(GuardedWriteDecision::AcceptWithConflict(
        ConflictRecord::new(
            algo,
            input.entity_id.clone(),
            input.base_version.clone(),
            input.operation_id.clone(),
            input.current_operation_id.clone(),
            field_or_region,
        )?,
    ))
}

fn reject_conflict(message: &str) -> GuardedWriteDecision {
    GuardedWriteDecision::Reject {
        code: Code::Conflict,
        message: message.to_string(),
    }
}

fn derive_conflict_id(
    algo: Algo,
    entity_id: &str,
    base_version: &str,
    losing_operation_id: &str,
    winning_operation_id: &str,
    field_or_region: Option<&str>,
) -> Result<String> {
    let bytes = loom_codec::encode(&Value::Array(vec![
        Value::Text(CONFLICT_RECORD_SCHEMA.to_string()),
        Value::Text(entity_id.to_string()),
        Value::Text(base_version.to_string()),
        Value::Text(losing_operation_id.to_string()),
        Value::Text(winning_operation_id.to_string()),
        optional_text_value(field_or_region),
    ]))
    .map_err(codec_error)?;
    Ok(Digest::hash(algo, &bytes).to_string())
}

fn canonical_field_set(fields: &[String]) -> Result<BTreeSet<String>> {
    fields
        .iter()
        .map(|field| {
            validate_text("field", field)?;
            Ok(field.clone())
        })
        .collect()
}

fn first_overlap(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Option<String> {
    left.intersection(right).next().cloned()
}

fn optional_text_value(value: Option<&str>) -> Value {
    value
        .map(|value| Value::Text(value.to_string()))
        .unwrap_or(Value::Null)
}

fn array(value: Value, name: &str) -> Result<Vec<Value>> {
    match value {
        Value::Array(items) => Ok(items),
        _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
    }
}

fn next(items: &mut Vec<Value>, name: &str) -> Result<Value> {
    if items.is_empty() {
        return Err(LoomError::corrupt(format!("missing {name}")));
    }
    Ok(items.remove(0))
}

fn end(items: &[Value], name: &str) -> Result<()> {
    if items.is_empty() {
        Ok(())
    } else {
        Err(LoomError::corrupt(format!("{name} has trailing fields")))
    }
}

fn expect_text(value: Value, expected: &str) -> Result<()> {
    let actual = text(value, "schema")?;
    if actual == expected {
        Ok(())
    } else {
        Err(LoomError::corrupt(format!("expected schema {expected}")))
    }
}

fn text(value: Value, name: &str) -> Result<String> {
    match value {
        Value::Text(value) => Ok(value),
        _ => Err(LoomError::corrupt(format!("{name} must be text"))),
    }
}

fn optional_text(value: Value) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::Text(value) => Ok(Some(value)),
        _ => Err(LoomError::corrupt("optional text must be text or null")),
    }
}

fn uint(value: Value) -> Result<u64> {
    match value {
        Value::Uint(value) => Ok(value),
        _ => Err(LoomError::corrupt("field must be unsigned integer")),
    }
}

fn validate_text(name: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid(format!("{name} must not be empty")));
    }
    if value.len() > 512 {
        return Err(LoomError::invalid(format!("{name} is too long")));
    }
    Ok(())
}

fn codec_error(error: loom_codec::CodecError) -> LoomError {
    LoomError::corrupt(format!("conflict record cbor: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(policy: GuardedWritePolicy) -> GuardedWriteInput {
        GuardedWriteInput {
            algo: Algo::Blake3,
            policy,
            entity_id: "ISSUE-1".to_string(),
            base_version: "v1".to_string(),
            current_version: "v2".to_string(),
            operation_id: "op-losing".to_string(),
            current_operation_id: "op-winning".to_string(),
            write_fields: vec!["summary".to_string()],
            changed_fields_since_base: vec!["summary".to_string()],
            revalidation_passed: false,
        }
    }

    #[test]
    fn conflict_record_round_trips_canonical_bytes() {
        let record = ConflictRecord::new(
            Algo::Blake3,
            "ISSUE-1",
            "v1",
            "op-losing",
            "op-winning",
            Some("summary".to_string()),
        )
        .unwrap();
        let encoded = record.encode().unwrap();
        let decoded = ConflictRecord::decode(&encoded).unwrap();
        assert_eq!(decoded, record);
        assert_eq!(decoded.encode().unwrap(), encoded);
        assert_eq!(decoded.resolution_state, ResolutionState::Open);
    }

    #[test]
    fn merge_disjoint_fields_accepts_only_disjoint_writes() {
        let mut disjoint = input(GuardedWritePolicy::MergeDisjointFields);
        disjoint.write_fields = vec!["description".to_string()];
        assert_eq!(
            evaluate_guarded_write(disjoint).unwrap(),
            GuardedWriteDecision::Accept
        );
        let overlapping =
            evaluate_guarded_write(input(GuardedWritePolicy::MergeDisjointFields)).unwrap();
        match overlapping {
            GuardedWriteDecision::AcceptWithConflict(record) => {
                assert_eq!(record.field_or_region.as_deref(), Some("summary"));
            }
            other => panic!("unexpected decision: {other:?}"),
        }
    }

    #[test]
    fn scalar_conflict_records_even_without_field_overlap() {
        let mut scalar = input(GuardedWritePolicy::RecordScalarConflict);
        scalar.write_fields = vec!["description".to_string()];
        let decision = evaluate_guarded_write(scalar).unwrap();
        match decision {
            GuardedWriteDecision::AcceptWithConflict(record) => {
                assert_eq!(record.field_or_region, None);
            }
            other => panic!("unexpected decision: {other:?}"),
        }
    }

    #[test]
    fn state_machine_revalidation_controls_acceptance() {
        let failed =
            evaluate_guarded_write(input(GuardedWritePolicy::RevalidateStateMachine)).unwrap();
        assert_eq!(
            failed,
            GuardedWriteDecision::Reject {
                code: Code::Conflict,
                message: "state-machine transition failed revalidation".to_string(),
            }
        );
        let mut passed = input(GuardedWritePolicy::RevalidateStateMachine);
        passed.revalidation_passed = true;
        assert_eq!(
            evaluate_guarded_write(passed).unwrap(),
            GuardedWriteDecision::Accept
        );
    }

    #[test]
    fn reject_stale_base_is_hard_rejection() {
        assert_eq!(
            evaluate_guarded_write(input(GuardedWritePolicy::RejectStaleBase)).unwrap(),
            GuardedWriteDecision::Reject {
                code: Code::Conflict,
                message: "entity version advanced past base".to_string(),
            }
        );
    }
}
