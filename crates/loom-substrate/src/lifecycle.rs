use std::collections::BTreeSet;

use loom_codec::Value;
use loom_types::{Code, Digest, LoomError, Result};

use crate::changes::{OperationChangeBatch, OperationChangeCursor, OperationChangeRecord};
use crate::{
    Fields, OperationEnvelope, codec_error, optional_digest, optional_text_value, string_array,
    validate_text,
};

pub const APP_ID: &str = "lifecycle";
pub const LIFECYCLE_DEFINITION_SCHEMA: &str = "loom.studio.lifecycle.definition.v1";
pub const LIFECYCLE_INSTANCE_SCHEMA: &str = "loom.studio.lifecycle.instance.v1";
pub const LIFECYCLE_TRANSITION_SCHEMA: &str = "loom.studio.lifecycle.transition.v1";
pub const LIFECYCLE_OPERATION_LOG_SCHEMA: &str = "loom.studio.lifecycle.operation-log.v1";
pub const LIFECYCLE_STAGE_SURFACE_SCHEMA: &str = "loom.studio.lifecycle.stage-surface.v1";
pub const LIFECYCLE_SNAPSHOT_PLAN_SCHEMA: &str = "loom.studio.lifecycle.snapshot-plan.v1";
pub const LIFECYCLE_SNAPSHOT_RECORD_SCHEMA: &str = "loom.studio.lifecycle.snapshot-record.v1";
pub const PROFILE_CONTROL_PREFIX: &str = "profile/lifecycle/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateKind {
    Predicate,
    Attestation,
}

impl GateKind {
    const fn tag(self) -> u64 {
        match self {
            Self::Predicate => 0,
            Self::Attestation => 1,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Predicate),
            1 => Ok(Self::Attestation),
            other => Err(LoomError::corrupt(format!(
                "unknown lifecycle gate kind tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotPolicy {
    None,
    FreezeScope,
}

impl SnapshotPolicy {
    const fn tag(self) -> u64 {
        match self {
            Self::None => 0,
            Self::FreezeScope => 1,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::None),
            1 => Ok(Self::FreezeScope),
            other => Err(LoomError::corrupt(format!(
                "unknown lifecycle snapshot policy tag {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardLifecycleKind {
    Feature,
    Bug,
    Incident,
    Design,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandardLifecycleInput {
    pub kind: StandardLifecycleKind,
    pub version: String,
    pub completion_predicate_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleGate {
    pub gate_id: String,
    pub label: String,
    pub kind: GateKind,
    pub predicate_digest: Option<Digest>,
    pub required_role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleGateInput<'a> {
    pub gate_id: &'a str,
    pub label: &'a str,
    pub kind: GateKind,
    pub predicate_digest: Option<Digest>,
    pub required_role: Option<&'a str>,
}

impl LifecycleGate {
    pub fn new(input: LifecycleGateInput<'_>) -> Result<Self> {
        let gate = Self {
            gate_id: input.gate_id.to_string(),
            label: input.label.to_string(),
            kind: input.kind,
            predicate_digest: input.predicate_digest,
            required_role: input.required_role.map(str::to_string),
        };
        gate.validate()?;
        Ok(gate)
    }

    fn validate(&self) -> Result<()> {
        validate_text("lifecycle gate_id", &self.gate_id)?;
        validate_text("lifecycle gate label", &self.label)?;
        if self.kind == GateKind::Predicate && self.predicate_digest.is_none() {
            return Err(LoomError::invalid(
                "predicate lifecycle gate requires predicate digest",
            ));
        }
        if self.kind == GateKind::Attestation && self.predicate_digest.is_some() {
            return Err(LoomError::invalid(
                "attestation lifecycle gate must not carry predicate digest",
            ));
        }
        if let Some(role) = &self.required_role {
            validate_text("lifecycle gate required_role", role)?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.gate_id.clone()),
            Value::Text(self.label.clone()),
            Value::Uint(self.kind.tag()),
            optional_digest(self.predicate_digest),
            optional_text_value(self.required_role.as_deref()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "lifecycle gate")?;
        let gate = Self {
            gate_id: fields.text("gate_id")?,
            label: fields.text("label")?,
            kind: GateKind::from_tag(fields.uint("kind")?)?,
            predicate_digest: fields.optional_digest("predicate_digest")?,
            required_role: fields.optional_text("required_role")?,
        };
        fields.end("lifecycle gate")?;
        gate.validate()?;
        Ok(gate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleStage {
    pub stage_id: String,
    pub label: String,
    pub entry_gates: Vec<LifecycleGate>,
    pub exit_gates: Vec<LifecycleGate>,
    pub snapshot_policy: SnapshotPolicy,
    pub surfaced_tools: Vec<String>,
    pub prompt_refs: Vec<String>,
}

impl LifecycleStage {
    pub fn new(stage_id: impl Into<String>, label: impl Into<String>) -> Result<Self> {
        let stage = Self {
            stage_id: stage_id.into(),
            label: label.into(),
            entry_gates: Vec::new(),
            exit_gates: Vec::new(),
            snapshot_policy: SnapshotPolicy::None,
            surfaced_tools: Vec::new(),
            prompt_refs: Vec::new(),
        };
        stage.validate()?;
        Ok(stage)
    }

    fn validate(&self) -> Result<()> {
        validate_text("lifecycle stage_id", &self.stage_id)?;
        validate_text("lifecycle stage label", &self.label)?;
        unique_ids(
            "lifecycle entry gate ids",
            self.entry_gates.iter().map(|gate| gate.gate_id.as_str()),
        )?;
        unique_ids(
            "lifecycle exit gate ids",
            self.exit_gates.iter().map(|gate| gate.gate_id.as_str()),
        )?;
        for gate in self.entry_gates.iter().chain(&self.exit_gates) {
            gate.validate()?;
        }
        validate_text_list("lifecycle surfaced tool", &self.surfaced_tools)?;
        validate_text_list("lifecycle prompt ref", &self.prompt_refs)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.stage_id.clone()),
            Value::Text(self.label.clone()),
            Value::Array(
                self.entry_gates
                    .iter()
                    .map(LifecycleGate::to_value)
                    .collect(),
            ),
            Value::Array(
                self.exit_gates
                    .iter()
                    .map(LifecycleGate::to_value)
                    .collect(),
            ),
            Value::Uint(self.snapshot_policy.tag()),
            string_array(&self.surfaced_tools),
            string_array(&self.prompt_refs),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "lifecycle stage")?;
        let stage = Self {
            stage_id: fields.text("stage_id")?,
            label: fields.text("label")?,
            entry_gates: gate_list(fields.next("entry_gates")?)?,
            exit_gates: gate_list(fields.next("exit_gates")?)?,
            snapshot_policy: SnapshotPolicy::from_tag(fields.uint("snapshot_policy")?)?,
            surfaced_tools: fields.string_array("surfaced_tools")?,
            prompt_refs: fields.string_array("prompt_refs")?,
        };
        fields.end("lifecycle stage")?;
        stage.validate()?;
        Ok(stage)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleDefinition {
    pub definition_id: String,
    pub version: String,
    pub stages: Vec<LifecycleStage>,
    pub initial_stage_id: String,
}

impl LifecycleDefinition {
    pub fn new(
        definition_id: impl Into<String>,
        version: impl Into<String>,
        stages: Vec<LifecycleStage>,
        initial_stage_id: impl Into<String>,
    ) -> Result<Self> {
        let definition = Self {
            definition_id: definition_id.into(),
            version: version.into(),
            stages,
            initial_stage_id: initial_stage_id.into(),
        };
        definition.validate()?;
        Ok(definition)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn stage(&self, stage_id: &str) -> Option<&LifecycleStage> {
        self.stages.iter().find(|stage| stage.stage_id == stage_id)
    }

    fn validate(&self) -> Result<()> {
        validate_text("lifecycle definition_id", &self.definition_id)?;
        validate_text("lifecycle version", &self.version)?;
        validate_text("lifecycle initial_stage_id", &self.initial_stage_id)?;
        if self.stages.is_empty() {
            return Err(LoomError::invalid("lifecycle definition requires stages"));
        }
        unique_ids(
            "lifecycle stage ids",
            self.stages.iter().map(|stage| stage.stage_id.as_str()),
        )?;
        for stage in &self.stages {
            stage.validate()?;
        }
        if self.stage(&self.initial_stage_id).is_none() {
            return Err(LoomError::invalid(
                "lifecycle initial stage must exist in definition",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(LIFECYCLE_DEFINITION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.definition_id.clone()),
                Value::Text(self.version.clone()),
                Value::Array(self.stages.iter().map(LifecycleStage::to_value).collect()),
                Value::Text(self.initial_stage_id.clone()),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "lifecycle definition")?;
        outer.expect_text(LIFECYCLE_DEFINITION_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("lifecycle definition fields")?,
            "lifecycle definition",
        )?;
        outer.end("lifecycle definition")?;
        let definition = Self {
            definition_id: fields.text("definition_id")?,
            version: fields.text("version")?,
            stages: stage_list(fields.next("stages")?)?,
            initial_stage_id: fields.text("initial_stage_id")?,
        };
        fields.end("lifecycle definition")?;
        definition.validate()?;
        Ok(definition)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateEvaluation {
    pub gate_id: String,
    pub passed: bool,
    pub principal_id: Option<String>,
    pub evidence_digest: Option<Digest>,
    pub evaluated_at_ms: u64,
}

impl GateEvaluation {
    pub fn new(gate_id: impl Into<String>, passed: bool, evaluated_at_ms: u64) -> Result<Self> {
        let evaluation = Self {
            gate_id: gate_id.into(),
            passed,
            principal_id: None,
            evidence_digest: None,
            evaluated_at_ms,
        };
        evaluation.validate()?;
        Ok(evaluation)
    }

    fn validate(&self) -> Result<()> {
        validate_text("gate evaluation gate_id", &self.gate_id)?;
        if let Some(principal) = &self.principal_id {
            validate_text("gate evaluation principal_id", principal)?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(self.gate_id.clone()),
            Value::Bool(self.passed),
            optional_text_value(self.principal_id.as_deref()),
            optional_digest(self.evidence_digest),
            Value::Uint(self.evaluated_at_ms),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "gate evaluation")?;
        let evaluation = Self {
            gate_id: fields.text("gate_id")?,
            passed: fields.bool("passed")?,
            principal_id: fields.optional_text("principal_id")?,
            evidence_digest: fields.optional_digest("evidence_digest")?,
            evaluated_at_ms: fields.uint("evaluated_at_ms")?,
        };
        fields.end("gate evaluation")?;
        evaluation.validate()?;
        Ok(evaluation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleTransitionRecord {
    pub transition_id: String,
    pub instance_id: String,
    pub definition_id: String,
    pub definition_version: String,
    pub from_stage_id: String,
    pub to_stage_id: String,
    pub actor_principal_id: String,
    pub gate_evaluations: Vec<GateEvaluation>,
    pub snapshot_digest: Option<Digest>,
    pub recorded_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleTransitionInput {
    pub transition_id: String,
    pub instance_id: String,
    pub definition_id: String,
    pub definition_version: String,
    pub from_stage_id: String,
    pub to_stage_id: String,
    pub actor_principal_id: String,
    pub gate_evaluations: Vec<GateEvaluation>,
    pub snapshot_digest: Option<Digest>,
    pub recorded_at_ms: u64,
}

impl LifecycleTransitionRecord {
    pub fn new(input: LifecycleTransitionInput) -> Result<Self> {
        let transition = Self {
            transition_id: input.transition_id,
            instance_id: input.instance_id,
            definition_id: input.definition_id,
            definition_version: input.definition_version,
            from_stage_id: input.from_stage_id,
            to_stage_id: input.to_stage_id,
            actor_principal_id: input.actor_principal_id,
            gate_evaluations: input.gate_evaluations,
            snapshot_digest: input.snapshot_digest,
            recorded_at_ms: input.recorded_at_ms,
        };
        transition.validate()?;
        Ok(transition)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("lifecycle transition_id", &self.transition_id)?;
        validate_text("lifecycle instance_id", &self.instance_id)?;
        validate_text("lifecycle definition_id", &self.definition_id)?;
        validate_text("lifecycle definition_version", &self.definition_version)?;
        validate_text("lifecycle from_stage_id", &self.from_stage_id)?;
        validate_text("lifecycle to_stage_id", &self.to_stage_id)?;
        validate_text("lifecycle actor_principal_id", &self.actor_principal_id)?;
        unique_ids(
            "gate evaluation ids",
            self.gate_evaluations
                .iter()
                .map(|evaluation| evaluation.gate_id.as_str()),
        )?;
        for evaluation in &self.gate_evaluations {
            evaluation.validate()?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(LIFECYCLE_TRANSITION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.transition_id.clone()),
                Value::Text(self.instance_id.clone()),
                Value::Text(self.definition_id.clone()),
                Value::Text(self.definition_version.clone()),
                Value::Text(self.from_stage_id.clone()),
                Value::Text(self.to_stage_id.clone()),
                Value::Text(self.actor_principal_id.clone()),
                Value::Array(
                    self.gate_evaluations
                        .iter()
                        .map(GateEvaluation::to_value)
                        .collect(),
                ),
                optional_digest(self.snapshot_digest),
                Value::Uint(self.recorded_at_ms),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "lifecycle transition")?;
        outer.expect_text(LIFECYCLE_TRANSITION_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("lifecycle transition fields")?,
            "lifecycle transition",
        )?;
        outer.end("lifecycle transition")?;
        let transition = Self {
            transition_id: fields.text("transition_id")?,
            instance_id: fields.text("instance_id")?,
            definition_id: fields.text("definition_id")?,
            definition_version: fields.text("definition_version")?,
            from_stage_id: fields.text("from_stage_id")?,
            to_stage_id: fields.text("to_stage_id")?,
            actor_principal_id: fields.text("actor_principal_id")?,
            gate_evaluations: gate_evaluation_list(fields.next("gate_evaluations")?)?,
            snapshot_digest: fields.optional_digest("snapshot_digest")?,
            recorded_at_ms: fields.uint("recorded_at_ms")?,
        };
        fields.end("lifecycle transition")?;
        transition.validate()?;
        Ok(transition)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleOperationRecord {
    pub sequence: u64,
    pub operation_id: String,
    pub operation_kind: String,
    pub instance_id: String,
    pub target_entity_id: Option<String>,
    pub root_after: Digest,
    pub envelope: Vec<u8>,
}

impl LifecycleOperationRecord {
    pub fn new(
        sequence: u64,
        operation_id: impl Into<String>,
        operation_kind: impl Into<String>,
        instance_id: impl Into<String>,
        target_entity_id: Option<String>,
        root_after: Digest,
        envelope: Vec<u8>,
    ) -> Result<Self> {
        let record = Self {
            sequence,
            operation_id: operation_id.into(),
            operation_kind: operation_kind.into(),
            instance_id: instance_id.into(),
            target_entity_id,
            root_after,
            envelope,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn transition(
        sequence: u64,
        transition: &LifecycleTransitionRecord,
        root_after: Digest,
        envelope: Vec<u8>,
    ) -> Result<Self> {
        Self::new(
            sequence,
            transition.transition_id.clone(),
            "lifecycle.transitioned",
            transition.instance_id.clone(),
            Some(format!("lifecycle:{}", transition.instance_id)),
            root_after,
            envelope,
        )
    }

    fn validate(&self) -> Result<()> {
        if self.sequence == 0 {
            return Err(LoomError::invalid(
                "lifecycle operation sequence must be at least 1",
            ));
        }
        validate_text("lifecycle operation_id", &self.operation_id)?;
        validate_text("lifecycle operation_kind", &self.operation_kind)?;
        validate_text("lifecycle operation instance_id", &self.instance_id)?;
        if let Some(target) = &self.target_entity_id {
            validate_text("lifecycle operation target", target)?;
        }
        if self.envelope.is_empty() {
            return Err(LoomError::invalid(
                "lifecycle operation envelope must not be empty",
            ));
        }
        let envelope = OperationEnvelope::decode(&self.envelope)?;
        if envelope.operation_id != self.operation_id {
            return Err(LoomError::corrupt(
                "lifecycle operation id does not match envelope",
            ));
        }
        if envelope.operation_kind != self.operation_kind {
            return Err(LoomError::corrupt(
                "lifecycle operation kind does not match envelope",
            ));
        }
        if envelope.sequence != self.sequence {
            return Err(LoomError::corrupt(
                "lifecycle operation sequence does not match envelope",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Uint(self.sequence),
            Value::Text(self.operation_id.clone()),
            Value::Text(self.operation_kind.clone()),
            Value::Text(self.instance_id.clone()),
            optional_text_value(self.target_entity_id.as_deref()),
            Value::Text(self.root_after.to_string()),
            Value::Bytes(self.envelope.clone()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "lifecycle operation record")?;
        let sequence = fields.uint("sequence")?;
        let operation_id = fields.text("operation_id")?;
        let operation_kind = fields.text("operation_kind")?;
        let instance_id = fields.text("instance_id")?;
        let target_entity_id = fields.optional_text("target_entity_id")?;
        let root_after = fields.digest("root_after")?;
        let envelope = fields.bytes("envelope")?;
        fields.end("lifecycle operation record")?;
        Self::new(
            sequence,
            operation_id,
            operation_kind,
            instance_id,
            target_entity_id,
            root_after,
            envelope,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleOperationLog {
    pub workspace_id: String,
    pub records: Vec<LifecycleOperationRecord>,
}

impl LifecycleOperationLog {
    pub fn new(
        workspace_id: impl Into<String>,
        records: Vec<LifecycleOperationRecord>,
    ) -> Result<Self> {
        let log = Self {
            workspace_id: workspace_id.into(),
            records,
        };
        log.validate()?;
        Ok(log)
    }

    pub fn append(&mut self, record: LifecycleOperationRecord) -> Result<()> {
        record.validate()?;
        if let Some(previous) = self.records.last()
            && record.sequence <= previous.sequence
        {
            return Err(LoomError::invalid(
                "lifecycle operation records must be ordered by increasing sequence",
            ));
        }
        if self
            .records
            .iter()
            .any(|existing| existing.operation_id == record.operation_id)
        {
            return Err(LoomError::invalid("lifecycle operation ids must be unique"));
        }
        self.records.push(record);
        self.validate()
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn changes(
        &self,
        cursor: &OperationChangeCursor,
        max: usize,
    ) -> Result<OperationChangeBatch> {
        let expected_scope = lifecycle_operation_cursor_scope(&self.workspace_id);
        if cursor.scope_id != expected_scope {
            return Err(LoomError::invalid(
                "operation change cursor scope does not match lifecycle operation log",
            ));
        }
        let mut events = Vec::new();
        let mut next_sequence = cursor.next_sequence;
        for record in &self.records {
            if record.sequence < cursor.next_sequence {
                continue;
            }
            if events.len() == max {
                break;
            }
            let envelope = OperationEnvelope::decode(&record.envelope)?;
            let change = OperationChangeRecord {
                workspace_id: envelope.workspace_id,
                app_id: envelope.app_id,
                scope_id: envelope.scope_id,
                operation_id: record.operation_id.clone(),
                operation_kind: record.operation_kind.clone(),
                sequence: record.sequence,
                actor_principal: envelope.actor_principal.to_string(),
                timestamp_ms: envelope.timestamp_ms,
                root_after: record.root_after,
                target_entity_id: envelope.target_entity_id,
                payload_digest: envelope.payload_digest,
                policy_labels: envelope.policy_labels,
            };
            change.validate()?;
            next_sequence = change.sequence + 1;
            events.push(change);
        }
        Ok(OperationChangeBatch {
            events,
            next: OperationChangeCursor::new(expected_scope, next_sequence)?,
        })
    }

    fn validate(&self) -> Result<()> {
        validate_text("lifecycle operation log workspace_id", &self.workspace_id)?;
        let mut previous = None;
        let mut ids = BTreeSet::new();
        for record in &self.records {
            record.validate()?;
            if !ids.insert(record.operation_id.clone()) {
                return Err(LoomError::invalid("lifecycle operation ids must be unique"));
            }
            if let Some(previous) = previous
                && record.sequence <= previous
            {
                return Err(LoomError::invalid(
                    "lifecycle operation records must be ordered by increasing sequence",
                ));
            }
            previous = Some(record.sequence);
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(LIFECYCLE_OPERATION_LOG_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(
                    self.records
                        .iter()
                        .map(LifecycleOperationRecord::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "lifecycle operation log")?;
        outer.expect_text(LIFECYCLE_OPERATION_LOG_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("lifecycle operation log fields")?,
            "lifecycle operation log",
        )?;
        outer.end("lifecycle operation log")?;
        let workspace_id = fields.text("workspace_id")?;
        let records = lifecycle_operation_record_list(fields.next("records")?)?;
        fields.end("lifecycle operation log")?;
        Self::new(workspace_id, records)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleInstance {
    pub instance_id: String,
    pub definition_id: String,
    pub definition_version: String,
    pub subject_refs: Vec<String>,
    pub current_stage_id: String,
    pub stage_history: Vec<LifecycleTransitionRecord>,
}

impl LifecycleInstance {
    pub fn new(
        instance_id: impl Into<String>,
        definition: &LifecycleDefinition,
        subject_refs: Vec<String>,
    ) -> Result<Self> {
        let instance = Self {
            instance_id: instance_id.into(),
            definition_id: definition.definition_id.clone(),
            definition_version: definition.version.clone(),
            subject_refs,
            current_stage_id: definition.initial_stage_id.clone(),
            stage_history: Vec::new(),
        };
        instance.validate()?;
        Ok(instance)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn apply_transition(
        &self,
        definition: &LifecycleDefinition,
        transition: LifecycleTransitionRecord,
    ) -> Result<Self> {
        validate_transition(definition, self, &transition)?;
        let mut next = self.clone();
        next.current_stage_id = transition.to_stage_id.clone();
        next.stage_history.push(transition);
        next.validate()?;
        Ok(next)
    }

    fn validate(&self) -> Result<()> {
        validate_text("lifecycle instance_id", &self.instance_id)?;
        validate_text("lifecycle instance definition_id", &self.definition_id)?;
        validate_text(
            "lifecycle instance definition_version",
            &self.definition_version,
        )?;
        validate_text("lifecycle current_stage_id", &self.current_stage_id)?;
        validate_text_list("lifecycle subject ref", &self.subject_refs)?;
        unique_ids(
            "lifecycle transition ids",
            self.stage_history
                .iter()
                .map(|transition| transition.transition_id.as_str()),
        )?;
        for transition in &self.stage_history {
            transition.validate()?;
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(LIFECYCLE_INSTANCE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.instance_id.clone()),
                Value::Text(self.definition_id.clone()),
                Value::Text(self.definition_version.clone()),
                string_array(&self.subject_refs),
                Value::Text(self.current_stage_id.clone()),
                Value::Array(
                    self.stage_history
                        .iter()
                        .map(LifecycleTransitionRecord::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "lifecycle instance")?;
        outer.expect_text(LIFECYCLE_INSTANCE_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("lifecycle instance fields")?,
            "lifecycle instance",
        )?;
        outer.end("lifecycle instance")?;
        let instance = Self {
            instance_id: fields.text("instance_id")?,
            definition_id: fields.text("definition_id")?,
            definition_version: fields.text("definition_version")?,
            subject_refs: fields.string_array("subject_refs")?,
            current_stage_id: fields.text("current_stage_id")?,
            stage_history: transition_list(fields.next("stage_history")?)?,
        };
        fields.end("lifecycle instance")?;
        instance.validate()?;
        Ok(instance)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageSurface {
    pub instance_id: String,
    pub stage_id: String,
    pub surfaced_tools: Vec<String>,
    pub prompt_refs: Vec<String>,
    pub read_only: bool,
}

impl StageSurface {
    pub fn for_instance(
        definition: &LifecycleDefinition,
        instance: &LifecycleInstance,
    ) -> Result<Self> {
        if instance.definition_id != definition.definition_id
            || instance.definition_version != definition.version
        {
            return Err(LoomError::new(
                Code::Conflict,
                "lifecycle surface definition version does not match",
            ));
        }
        let stage = definition
            .stage(&instance.current_stage_id)
            .ok_or_else(|| LoomError::invalid("lifecycle surface stage is unknown"))?;
        let mut surfaced_tools = stage.surfaced_tools.clone();
        surfaced_tools.sort();
        surfaced_tools.dedup();
        let mut prompt_refs = stage.prompt_refs.clone();
        prompt_refs.sort();
        prompt_refs.dedup();
        let surface = Self {
            instance_id: instance.instance_id.clone(),
            stage_id: stage.stage_id.clone(),
            read_only: stage.stage_id == "archive",
            surfaced_tools,
            prompt_refs,
        };
        surface.validate()?;
        Ok(surface)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("stage surface instance_id", &self.instance_id)?;
        validate_text("stage surface stage_id", &self.stage_id)?;
        validate_text_list("stage surface tool", &self.surfaced_tools)?;
        validate_text_list("stage surface prompt", &self.prompt_refs)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(LIFECYCLE_STAGE_SURFACE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.instance_id.clone()),
                Value::Text(self.stage_id.clone()),
                string_array(&self.surfaced_tools),
                string_array(&self.prompt_refs),
                Value::Bool(self.read_only),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "lifecycle stage surface")?;
        outer.expect_text(LIFECYCLE_STAGE_SURFACE_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("lifecycle stage surface fields")?,
            "lifecycle stage surface",
        )?;
        outer.end("lifecycle stage surface")?;
        let surface = Self {
            instance_id: fields.text("instance_id")?,
            stage_id: fields.text("stage_id")?,
            surfaced_tools: fields.string_array("surfaced_tools")?,
            prompt_refs: fields.string_array("prompt_refs")?,
            read_only: fields.bool("read_only")?,
        };
        fields.end("lifecycle stage surface")?;
        surface.validate()?;
        Ok(surface)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPlan {
    pub instance_id: String,
    pub from_stage_id: String,
    pub to_stage_id: String,
    pub required: bool,
    pub subject_refs: Vec<String>,
    pub policy: SnapshotPolicy,
}

impl SnapshotPlan {
    pub fn for_transition(
        definition: &LifecycleDefinition,
        instance: &LifecycleInstance,
        to_stage_id: impl Into<String>,
    ) -> Result<Self> {
        if instance.definition_id != definition.definition_id
            || instance.definition_version != definition.version
        {
            return Err(LoomError::new(
                Code::Conflict,
                "lifecycle snapshot definition version does not match",
            ));
        }
        let to_stage_id = to_stage_id.into();
        let to_stage = definition
            .stage(&to_stage_id)
            .ok_or_else(|| LoomError::invalid("lifecycle snapshot target stage is unknown"))?;
        let mut subject_refs = instance.subject_refs.clone();
        subject_refs.sort();
        subject_refs.dedup();
        let plan = Self {
            instance_id: instance.instance_id.clone(),
            from_stage_id: instance.current_stage_id.clone(),
            to_stage_id,
            required: to_stage.snapshot_policy == SnapshotPolicy::FreezeScope,
            subject_refs,
            policy: to_stage.snapshot_policy,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("snapshot plan instance_id", &self.instance_id)?;
        validate_text("snapshot plan from_stage_id", &self.from_stage_id)?;
        validate_text("snapshot plan to_stage_id", &self.to_stage_id)?;
        validate_text_list("snapshot plan subject ref", &self.subject_refs)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(LIFECYCLE_SNAPSHOT_PLAN_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.instance_id.clone()),
                Value::Text(self.from_stage_id.clone()),
                Value::Text(self.to_stage_id.clone()),
                Value::Bool(self.required),
                string_array(&self.subject_refs),
                Value::Uint(self.policy.tag()),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "lifecycle snapshot plan")?;
        outer.expect_text(LIFECYCLE_SNAPSHOT_PLAN_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("lifecycle snapshot plan fields")?,
            "lifecycle snapshot plan",
        )?;
        outer.end("lifecycle snapshot plan")?;
        let plan = Self {
            instance_id: fields.text("instance_id")?,
            from_stage_id: fields.text("from_stage_id")?,
            to_stage_id: fields.text("to_stage_id")?,
            required: fields.bool("required")?,
            subject_refs: fields.string_array("subject_refs")?,
            policy: SnapshotPolicy::from_tag(fields.uint("policy")?)?,
        };
        fields.end("lifecycle snapshot plan")?;
        plan.validate()?;
        Ok(plan)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRecord {
    pub snapshot_id: String,
    pub instance_id: String,
    pub transition_id: String,
    pub from_stage_id: String,
    pub to_stage_id: String,
    pub subject_refs: Vec<String>,
    pub policy: SnapshotPolicy,
    pub snapshot_digest: Digest,
    pub recorded_at_ms: u64,
}

impl SnapshotRecord {
    pub fn from_plan(
        plan: &SnapshotPlan,
        transition_id: impl Into<String>,
        snapshot_digest: Digest,
        recorded_at_ms: u64,
    ) -> Result<Self> {
        let transition_id = transition_id.into();
        let record = Self {
            snapshot_id: format!("{}:{}", plan.instance_id, transition_id),
            instance_id: plan.instance_id.clone(),
            transition_id,
            from_stage_id: plan.from_stage_id.clone(),
            to_stage_id: plan.to_stage_id.clone(),
            subject_refs: plan.subject_refs.clone(),
            policy: plan.policy,
            snapshot_digest,
            recorded_at_ms,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn validate(&self) -> Result<()> {
        validate_text("snapshot record snapshot_id", &self.snapshot_id)?;
        validate_text("snapshot record instance_id", &self.instance_id)?;
        validate_text("snapshot record transition_id", &self.transition_id)?;
        validate_text("snapshot record from_stage_id", &self.from_stage_id)?;
        validate_text("snapshot record to_stage_id", &self.to_stage_id)?;
        validate_text_list("snapshot record subject ref", &self.subject_refs)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(LIFECYCLE_SNAPSHOT_RECORD_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.snapshot_id.clone()),
                Value::Text(self.instance_id.clone()),
                Value::Text(self.transition_id.clone()),
                Value::Text(self.from_stage_id.clone()),
                Value::Text(self.to_stage_id.clone()),
                string_array(&self.subject_refs),
                Value::Uint(self.policy.tag()),
                Value::Text(self.snapshot_digest.to_string()),
                Value::Uint(self.recorded_at_ms),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "lifecycle snapshot record")?;
        outer.expect_text(LIFECYCLE_SNAPSHOT_RECORD_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("lifecycle snapshot record fields")?,
            "lifecycle snapshot record",
        )?;
        outer.end("lifecycle snapshot record")?;
        let record = Self {
            snapshot_id: fields.text("snapshot_id")?,
            instance_id: fields.text("instance_id")?,
            transition_id: fields.text("transition_id")?,
            from_stage_id: fields.text("from_stage_id")?,
            to_stage_id: fields.text("to_stage_id")?,
            subject_refs: fields.string_array("subject_refs")?,
            policy: SnapshotPolicy::from_tag(fields.uint("policy")?)?,
            snapshot_digest: fields.digest("snapshot_digest")?,
            recorded_at_ms: fields.uint("recorded_at_ms")?,
        };
        fields.end("lifecycle snapshot record")?;
        record.validate()?;
        Ok(record)
    }
}

pub fn validate_transition(
    definition: &LifecycleDefinition,
    instance: &LifecycleInstance,
    transition: &LifecycleTransitionRecord,
) -> Result<()> {
    if transition.instance_id != instance.instance_id {
        return Err(LoomError::invalid(
            "lifecycle transition instance does not match",
        ));
    }
    if transition.definition_id != definition.definition_id
        || transition.definition_version != definition.version
        || instance.definition_id != definition.definition_id
        || instance.definition_version != definition.version
    {
        return Err(LoomError::new(
            Code::Conflict,
            "lifecycle transition definition version does not match",
        ));
    }
    if transition.from_stage_id != instance.current_stage_id {
        return Err(LoomError::new(
            Code::Conflict,
            "lifecycle transition starts from stale stage",
        ));
    }
    let from_stage = definition
        .stage(&transition.from_stage_id)
        .ok_or_else(|| LoomError::invalid("lifecycle transition from stage is unknown"))?;
    let to_stage = definition
        .stage(&transition.to_stage_id)
        .ok_or_else(|| LoomError::invalid("lifecycle transition to stage is unknown"))?;
    if to_stage.snapshot_policy == SnapshotPolicy::FreezeScope
        && transition.snapshot_digest.is_none()
    {
        return Err(LoomError::invalid(
            "lifecycle transition requires snapshot digest",
        ));
    }
    let required_gates = from_stage
        .exit_gates
        .iter()
        .chain(&to_stage.entry_gates)
        .collect::<Vec<_>>();
    let passed = transition
        .gate_evaluations
        .iter()
        .filter(|evaluation| evaluation.passed)
        .map(|evaluation| evaluation.gate_id.as_str())
        .collect::<BTreeSet<_>>();
    for gate in required_gates {
        if !passed.contains(gate.gate_id.as_str()) {
            return Err(LoomError::new(
                Code::Conflict,
                format!("lifecycle transition gate {} did not pass", gate.gate_id),
            ));
        }
    }
    Ok(())
}

pub fn standard_lifecycle_definition(input: StandardLifecycleInput) -> Result<LifecycleDefinition> {
    let version = input.version;
    match input.kind {
        StandardLifecycleKind::Feature => standard_definition(
            "feature",
            &version,
            &[
                StandardStage::new(
                    "ideate",
                    "Ideate",
                    &["pages_create"],
                    &["prompt:lifecycle_feature_ideate"],
                ),
                StandardStage::new(
                    "draft",
                    "Draft",
                    &["pages_update"],
                    &["prompt:lifecycle_feature_draft"],
                ),
                StandardStage::new(
                    "structure",
                    "Structure",
                    &["structures_create", "tickets_create"],
                    &["prompt:lifecycle_feature_structure"],
                ),
                StandardStage::freeze(
                    "ready",
                    "Ready",
                    &["tickets_update"],
                    &["prompt:lifecycle_feature_ready"],
                ),
                StandardStage::new(
                    "build",
                    "Build",
                    &["tickets_update", "pages_publish"],
                    &["prompt:lifecycle_feature_build"],
                ),
                StandardStage::freeze(
                    "done",
                    "Done",
                    &["pages_publish"],
                    &["prompt:lifecycle_feature_done"],
                ),
                StandardStage::archive(),
            ],
            input.completion_predicate_digest,
        ),
        StandardLifecycleKind::Bug => standard_definition(
            "bug",
            &version,
            &[
                StandardStage::new(
                    "triage",
                    "Triage",
                    &["tickets_update"],
                    &["prompt:lifecycle_bug_triage"],
                ),
                StandardStage::new(
                    "reproduce",
                    "Reproduce",
                    &["pages_create"],
                    &["prompt:lifecycle_bug_reproduce"],
                ),
                StandardStage::new(
                    "fix",
                    "Fix",
                    &["tickets_update"],
                    &["prompt:lifecycle_bug_fix"],
                ),
                StandardStage::freeze(
                    "verify",
                    "Verify",
                    &["tickets_update"],
                    &["prompt:lifecycle_bug_verify"],
                ),
                StandardStage::freeze(
                    "done",
                    "Done",
                    &["pages_publish"],
                    &["prompt:lifecycle_bug_done"],
                ),
                StandardStage::archive(),
            ],
            input.completion_predicate_digest,
        ),
        StandardLifecycleKind::Incident => standard_definition(
            "incident",
            &version,
            &[
                StandardStage::new(
                    "triage",
                    "Triage",
                    &["tickets_create", "chat_post_message"],
                    &["prompt:lifecycle_incident_triage"],
                ),
                StandardStage::freeze(
                    "mitigate",
                    "Mitigate",
                    &["tickets_update", "chat_post_message"],
                    &["prompt:lifecycle_incident_mitigate"],
                ),
                StandardStage::new(
                    "resolve",
                    "Resolve",
                    &["tickets_update"],
                    &["prompt:lifecycle_incident_resolve"],
                ),
                StandardStage::freeze(
                    "review",
                    "Review",
                    &["pages_publish"],
                    &["prompt:lifecycle_incident_review"],
                ),
                StandardStage::archive(),
            ],
            input.completion_predicate_digest,
        ),
        StandardLifecycleKind::Design => standard_definition(
            "design",
            &version,
            &[
                StandardStage::new(
                    "ideate",
                    "Ideate",
                    &["pages_create"],
                    &["prompt:lifecycle_design_ideate"],
                ),
                StandardStage::new(
                    "draft",
                    "Draft",
                    &["pages_update"],
                    &["prompt:lifecycle_design_draft"],
                ),
                StandardStage::new(
                    "review",
                    "Review",
                    &["pages_publish"],
                    &["prompt:lifecycle_design_review"],
                ),
                StandardStage::freeze(
                    "accepted",
                    "Accepted",
                    &["pages_publish"],
                    &["prompt:lifecycle_design_accepted"],
                ),
                StandardStage::archive(),
            ],
            input.completion_predicate_digest,
        ),
    }
}

struct StandardStage<'a> {
    stage_id: &'a str,
    label: &'a str,
    tools: &'a [&'a str],
    prompts: &'a [&'a str],
    snapshot_policy: SnapshotPolicy,
}

impl<'a> StandardStage<'a> {
    const fn new(
        stage_id: &'a str,
        label: &'a str,
        tools: &'a [&'a str],
        prompts: &'a [&'a str],
    ) -> Self {
        Self {
            stage_id,
            label,
            tools,
            prompts,
            snapshot_policy: SnapshotPolicy::None,
        }
    }

    const fn freeze(
        stage_id: &'a str,
        label: &'a str,
        tools: &'a [&'a str],
        prompts: &'a [&'a str],
    ) -> Self {
        Self {
            stage_id,
            label,
            tools,
            prompts,
            snapshot_policy: SnapshotPolicy::FreezeScope,
        }
    }

    const fn archive() -> Self {
        Self {
            stage_id: "archive",
            label: "Archive",
            tools: &[],
            prompts: &["prompt:lifecycle_archive"],
            snapshot_policy: SnapshotPolicy::None,
        }
    }
}

fn standard_definition(
    definition_id: &str,
    version: &str,
    stages: &[StandardStage<'_>],
    completion_predicate_digest: Digest,
) -> Result<LifecycleDefinition> {
    let mut lifecycle_stages = Vec::with_capacity(stages.len());
    for (index, stage) in stages.iter().enumerate() {
        let mut lifecycle_stage = LifecycleStage::new(stage.stage_id, stage.label)?;
        lifecycle_stage.snapshot_policy = stage.snapshot_policy;
        lifecycle_stage.surfaced_tools =
            stage.tools.iter().map(|tool| (*tool).to_string()).collect();
        lifecycle_stage.prompt_refs = stage
            .prompts
            .iter()
            .map(|prompt| (*prompt).to_string())
            .collect();
        lifecycle_stage.entry_gates = if index == 0 {
            Vec::new()
        } else {
            vec![LifecycleGate::new(LifecycleGateInput {
                gate_id: &format!("enter-{}", stage.stage_id),
                label: &format!("Enter {}", stage.label),
                kind: if stage.snapshot_policy == SnapshotPolicy::FreezeScope {
                    GateKind::Predicate
                } else {
                    GateKind::Attestation
                },
                predicate_digest: if stage.snapshot_policy == SnapshotPolicy::FreezeScope {
                    Some(completion_predicate_digest)
                } else {
                    None
                },
                required_role: Some("operator"),
            })?]
        };
        lifecycle_stages.push(lifecycle_stage);
    }
    LifecycleDefinition::new(
        definition_id,
        version,
        lifecycle_stages,
        stages
            .first()
            .ok_or_else(|| LoomError::invalid("standard lifecycle requires stages"))?
            .stage_id,
    )
}

fn gate_list(value: Value) -> Result<Vec<LifecycleGate>> {
    read_list(value, "lifecycle gates", LifecycleGate::from_value)
}

fn stage_list(value: Value) -> Result<Vec<LifecycleStage>> {
    read_list(value, "lifecycle stages", LifecycleStage::from_value)
}

fn gate_evaluation_list(value: Value) -> Result<Vec<GateEvaluation>> {
    read_list(
        value,
        "lifecycle gate evaluations",
        GateEvaluation::from_value,
    )
}

fn transition_list(value: Value) -> Result<Vec<LifecycleTransitionRecord>> {
    read_list(
        value,
        "lifecycle transitions",
        LifecycleTransitionRecord::from_value,
    )
}

fn lifecycle_operation_record_list(value: Value) -> Result<Vec<LifecycleOperationRecord>> {
    read_list(
        value,
        "lifecycle operation records",
        LifecycleOperationRecord::from_value,
    )
}

pub fn lifecycle_operation_cursor_scope(workspace_id: &str) -> String {
    format!("lifecycle:{workspace_id}")
}

pub fn lifecycle_operation_log_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("lifecycle operation log workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/operations").into_bytes())
}

fn read_list<T>(value: Value, name: &str, read: fn(Value) -> Result<T>) -> Result<Vec<T>> {
    match value {
        Value::Array(items) => items.into_iter().map(read).collect(),
        _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
    }
}

fn validate_text_list(name: &str, values: &[String]) -> Result<()> {
    for value in values {
        validate_text(name, value)?;
    }
    Ok(())
}

fn unique_ids<'a>(name: &str, ids: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for id in ids {
        validate_text(name, id)?;
        if !seen.insert(id) {
            return Err(LoomError::invalid(format!("{name} must be unique")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActorKind, OperationEnvelopeInput};
    use loom_types::Algo;

    fn digest(label: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, label)
    }

    fn actor(byte: u8) -> loom_types::WorkspaceId {
        loom_types::WorkspaceId::from_bytes([byte; 16])
    }

    fn feature_definition() -> LifecycleDefinition {
        let mut ideate = LifecycleStage::new("ideate", "Ideate").unwrap();
        ideate.exit_gates = vec![
            LifecycleGate::new(LifecycleGateInput {
                gate_id: "idea-framed",
                label: "Idea framed",
                kind: GateKind::Attestation,
                predicate_digest: None,
                required_role: Some("operator"),
            })
            .unwrap(),
        ];
        ideate.surfaced_tools = vec!["pages_create".to_string()];
        let mut ready = LifecycleStage::new("ready", "Ready").unwrap();
        ready.snapshot_policy = SnapshotPolicy::FreezeScope;
        ready.entry_gates = vec![
            LifecycleGate::new(LifecycleGateInput {
                gate_id: "tickets-accepted",
                label: "Scope tickets accepted",
                kind: GateKind::Predicate,
                predicate_digest: Some(digest(b"predicate")),
                required_role: None,
            })
            .unwrap(),
        ];
        ready.surfaced_tools = vec!["tickets_update".to_string()];
        LifecycleDefinition::new("feature", "1", vec![ideate, ready], "ideate").unwrap()
    }

    fn transition_record(sequence: u64, operation_id: &str) -> LifecycleTransitionRecord {
        LifecycleTransitionRecord::new(LifecycleTransitionInput {
            transition_id: operation_id.to_string(),
            instance_id: "feat-1".to_string(),
            definition_id: "feature".to_string(),
            definition_version: "1".to_string(),
            from_stage_id: "ideate".to_string(),
            to_stage_id: "ready".to_string(),
            actor_principal_id: "principal-1".to_string(),
            gate_evaluations: vec![
                GateEvaluation::new("idea-framed", true, sequence * 10).unwrap(),
                GateEvaluation::new("tickets-accepted", true, sequence * 10).unwrap(),
            ],
            snapshot_digest: Some(digest(format!("snapshot-{sequence}").as_bytes())),
            recorded_at_ms: sequence * 10,
        })
        .unwrap()
    }

    fn lifecycle_operation_record(sequence: u64, operation_id: &str) -> LifecycleOperationRecord {
        let transition = transition_record(sequence, operation_id);
        let payload = transition.encode().unwrap();
        let envelope = OperationEnvelope::new(
            Algo::Blake3,
            OperationEnvelopeInput {
                workspace_id: "studio",
                app_id: APP_ID,
                scope_id: &lifecycle_operation_cursor_scope("studio"),
                operation_id,
                operation_kind: "lifecycle.transitioned",
                sequence,
                actor_principal: actor(1),
                actor_kind: ActorKind::User,
                timestamp_ms: sequence * 10,
                idempotency_key: operation_id,
                base_root: digest(b"base"),
                base_entity_version: None,
                target_entity_id: Some("lifecycle:feat-1"),
                payload: &payload,
                policy_labels: &["team"],
                signature: None,
                agent: None,
            },
        )
        .unwrap();
        LifecycleOperationRecord::transition(
            sequence,
            &transition,
            digest(format!("root-{sequence}").as_bytes()),
            envelope.encode().unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn lifecycle_definition_round_trips_canonical_bytes() {
        let definition = feature_definition();
        let encoded = definition.encode().unwrap();
        let decoded = LifecycleDefinition::decode(&encoded).unwrap();
        assert_eq!(decoded, definition);
        assert_eq!(decoded.encode().unwrap(), encoded);
    }

    #[test]
    fn lifecycle_instance_transitions_after_gates_pass() {
        let definition = feature_definition();
        let instance = LifecycleInstance::new(
            "feat-1",
            &definition,
            vec!["page:spec-1".to_string(), "ticket:LOOM-1".to_string()],
        )
        .unwrap();
        let transition = LifecycleTransitionRecord::new(LifecycleTransitionInput {
            transition_id: "tr-1".to_string(),
            instance_id: "feat-1".to_string(),
            definition_id: "feature".to_string(),
            definition_version: "1".to_string(),
            from_stage_id: "ideate".to_string(),
            to_stage_id: "ready".to_string(),
            actor_principal_id: "principal-1".to_string(),
            gate_evaluations: vec![
                GateEvaluation::new("idea-framed", true, 100).unwrap(),
                GateEvaluation::new("tickets-accepted", true, 100).unwrap(),
            ],
            snapshot_digest: Some(digest(b"ready")),
            recorded_at_ms: 100,
        })
        .unwrap();
        let next = instance.apply_transition(&definition, transition).unwrap();
        assert_eq!(next.current_stage_id, "ready");
        assert_eq!(next.stage_history.len(), 1);
        let encoded = next.encode().unwrap();
        assert_eq!(LifecycleInstance::decode(&encoded).unwrap(), next);
    }

    #[test]
    fn lifecycle_transition_rejects_missing_gate_and_snapshot() {
        let definition = feature_definition();
        let instance =
            LifecycleInstance::new("feat-1", &definition, vec!["page:spec-1".to_string()]).unwrap();
        let missing_gate = LifecycleTransitionRecord::new(LifecycleTransitionInput {
            transition_id: "tr-1".to_string(),
            instance_id: "feat-1".to_string(),
            definition_id: "feature".to_string(),
            definition_version: "1".to_string(),
            from_stage_id: "ideate".to_string(),
            to_stage_id: "ready".to_string(),
            actor_principal_id: "principal-1".to_string(),
            gate_evaluations: vec![GateEvaluation::new("idea-framed", true, 100).unwrap()],
            snapshot_digest: Some(digest(b"ready")),
            recorded_at_ms: 100,
        })
        .unwrap();
        assert_eq!(
            validate_transition(&definition, &instance, &missing_gate)
                .unwrap_err()
                .code,
            loom_types::Code::Conflict
        );
        let missing_snapshot = LifecycleTransitionRecord::new(LifecycleTransitionInput {
            transition_id: "tr-2".to_string(),
            instance_id: "feat-1".to_string(),
            definition_id: "feature".to_string(),
            definition_version: "1".to_string(),
            from_stage_id: "ideate".to_string(),
            to_stage_id: "ready".to_string(),
            actor_principal_id: "principal-1".to_string(),
            gate_evaluations: vec![
                GateEvaluation::new("idea-framed", true, 100).unwrap(),
                GateEvaluation::new("tickets-accepted", true, 100).unwrap(),
            ],
            snapshot_digest: None,
            recorded_at_ms: 100,
        })
        .unwrap();
        assert_eq!(
            validate_transition(&definition, &instance, &missing_snapshot)
                .unwrap_err()
                .code,
            loom_types::Code::InvalidArgument
        );
    }

    #[test]
    fn lifecycle_operation_log_round_trips_and_projects_changes() {
        let log = LifecycleOperationLog::new(
            "studio",
            vec![
                lifecycle_operation_record(1, "transition-1"),
                lifecycle_operation_record(2, "transition-2"),
            ],
        )
        .unwrap();
        assert_eq!(
            LifecycleOperationLog::decode(&log.encode().unwrap()).unwrap(),
            log
        );
        assert_eq!(
            lifecycle_operation_log_key("studio").unwrap(),
            b"profile/lifecycle/v1/studio/operations".to_vec()
        );
        let changes = log
            .changes(
                &OperationChangeCursor::new(lifecycle_operation_cursor_scope("studio"), 2).unwrap(),
                10,
            )
            .unwrap();
        assert_eq!(changes.events.len(), 1);
        assert_eq!(changes.events[0].operation_kind, "lifecycle.transitioned");
        assert_eq!(changes.events[0].app_id, APP_ID);
        assert_eq!(changes.events[0].policy_labels, vec!["team"]);
        assert_eq!(changes.next.encode(), "oplog:3:lifecycle:studio");
    }

    #[test]
    fn lifecycle_operation_log_rejects_duplicates_and_envelope_mismatch() {
        let first = lifecycle_operation_record(1, "transition-1");
        assert_eq!(
            LifecycleOperationLog::new("studio", vec![first.clone(), first])
                .unwrap_err()
                .code,
            loom_types::Code::InvalidArgument
        );
        let transition = transition_record(2, "transition-2");
        let payload = transition.encode().unwrap();
        let mismatched = OperationEnvelope::new(
            Algo::Blake3,
            OperationEnvelopeInput {
                workspace_id: "studio",
                app_id: APP_ID,
                scope_id: &lifecycle_operation_cursor_scope("studio"),
                operation_id: "transition-2",
                operation_kind: "lifecycle.other",
                sequence: 2,
                actor_principal: actor(1),
                actor_kind: ActorKind::User,
                timestamp_ms: 20,
                idempotency_key: "transition-2",
                base_root: digest(b"base"),
                base_entity_version: None,
                target_entity_id: Some("lifecycle:feat-1"),
                payload: &payload,
                policy_labels: &[],
                signature: None,
                agent: None,
            },
        )
        .unwrap();
        assert_eq!(
            LifecycleOperationRecord::transition(
                2,
                &transition,
                digest(b"root-2"),
                mismatched.encode().unwrap(),
            )
            .unwrap_err()
            .code,
            loom_types::Code::CorruptObject
        );
    }

    #[test]
    fn predicate_gate_requires_predicate_digest() {
        let err = LifecycleGate::new(LifecycleGateInput {
            gate_id: "tickets-accepted",
            label: "Scope tickets accepted",
            kind: GateKind::Predicate,
            predicate_digest: None,
            required_role: None,
        })
        .unwrap_err();
        assert_eq!(err.code, loom_types::Code::InvalidArgument);
    }

    #[test]
    fn snapshot_plan_and_stage_surface_round_trip() {
        let definition = feature_definition();
        let instance = LifecycleInstance::new(
            "feat-1",
            &definition,
            vec!["ticket:LOOM-1".to_string(), "page:spec-1".to_string()],
        )
        .unwrap();
        let surface = StageSurface::for_instance(&definition, &instance).unwrap();
        assert_eq!(surface.stage_id, "ideate");
        assert_eq!(surface.surfaced_tools, vec!["pages_create"]);
        let encoded_surface = surface.encode().unwrap();
        assert_eq!(StageSurface::decode(&encoded_surface).unwrap(), surface);

        let plan = SnapshotPlan::for_transition(&definition, &instance, "ready").unwrap();
        assert!(plan.required);
        assert_eq!(
            plan.subject_refs,
            vec!["page:spec-1".to_string(), "ticket:LOOM-1".to_string()]
        );
        let encoded_plan = plan.encode().unwrap();
        assert_eq!(SnapshotPlan::decode(&encoded_plan).unwrap(), plan);

        let snapshot =
            SnapshotRecord::from_plan(&plan, "transition-1", digest(b"snapshot-root"), 120)
                .unwrap();
        assert_eq!(snapshot.snapshot_id, "feat-1:transition-1");
        assert_eq!(snapshot.instance_id, "feat-1");
        assert_eq!(snapshot.transition_id, "transition-1");
        assert_eq!(snapshot.from_stage_id, "ideate");
        assert_eq!(snapshot.to_stage_id, "ready");
        assert_eq!(snapshot.policy, SnapshotPolicy::FreezeScope);
        assert_eq!(snapshot.subject_refs, plan.subject_refs);
        let encoded_snapshot = snapshot.encode().unwrap();
        assert_eq!(SnapshotRecord::decode(&encoded_snapshot).unwrap(), snapshot);
    }

    #[test]
    fn standard_lifecycle_library_contains_resolved_roster() {
        for (kind, definition_id) in [
            (StandardLifecycleKind::Feature, "feature"),
            (StandardLifecycleKind::Bug, "bug"),
            (StandardLifecycleKind::Incident, "incident"),
            (StandardLifecycleKind::Design, "design"),
        ] {
            let definition = standard_lifecycle_definition(StandardLifecycleInput {
                kind,
                version: "1".to_string(),
                completion_predicate_digest: digest(b"complete"),
            })
            .unwrap();
            assert_eq!(definition.definition_id, definition_id);
            assert!(definition.stage("archive").is_some());
            assert!(
                definition
                    .stages
                    .iter()
                    .any(|stage| stage.snapshot_policy == SnapshotPolicy::FreezeScope)
            );
            let encoded = definition.encode().unwrap();
            assert_eq!(LifecycleDefinition::decode(&encoded).unwrap(), definition);
        }
    }
}
