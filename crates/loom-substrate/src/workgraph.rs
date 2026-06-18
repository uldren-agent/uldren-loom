use loom_codec::Value;
use loom_types::{Digest, LoomError, Result};
use std::collections::BTreeSet;

use crate::changes::{OperationChangeBatch, OperationChangeCursor, OperationChangeRecord};
use crate::{ActorKind, Fields, OperationEnvelope, validate_text};

pub const WORKGRAPH_FACT_SCHEMA: &str = "loom.substrate.workgraph-fact.v1";
pub const WORKGRAPH_OPERATION_LOG_SCHEMA: &str = "loom.substrate.workgraph.operation-log.v1";
pub const PROFILE_CONTROL_PREFIX: &str = "profile/workgraph/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkgraphFactKind {
    AssignmentIssued,
    BoardReadAcknowledged,
    BoardReadObserved,
    ResultWritten,
    VerificationAccepted,
    RevisionRequested,
    TaskBlocked,
    TaskUnblocked,
    TaskCompleted,
}

impl WorkgraphFactKind {
    fn tag(self) -> u64 {
        match self {
            Self::AssignmentIssued => 0,
            Self::BoardReadAcknowledged => 1,
            Self::BoardReadObserved => 2,
            Self::ResultWritten => 3,
            Self::VerificationAccepted => 4,
            Self::RevisionRequested => 5,
            Self::TaskBlocked => 6,
            Self::TaskUnblocked => 7,
            Self::TaskCompleted => 8,
        }
    }
    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::AssignmentIssued),
            1 => Ok(Self::BoardReadAcknowledged),
            2 => Ok(Self::BoardReadObserved),
            3 => Ok(Self::ResultWritten),
            4 => Ok(Self::VerificationAccepted),
            5 => Ok(Self::RevisionRequested),
            6 => Ok(Self::TaskBlocked),
            7 => Ok(Self::TaskUnblocked),
            8 => Ok(Self::TaskCompleted),
            _ => Err(LoomError::corrupt("unknown workgraph fact kind")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkgraphState {
    Ready,
    Assigned,
    Blocked,
    Completed,
    RevisionRequested,
    Accepted,
}

impl WorkgraphState {
    fn tag(self) -> u64 {
        match self {
            Self::Ready => 0,
            Self::Assigned => 1,
            Self::Blocked => 2,
            Self::Completed => 3,
            Self::RevisionRequested => 4,
            Self::Accepted => 5,
        }
    }
    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(Self::Ready),
            1 => Ok(Self::Assigned),
            2 => Ok(Self::Blocked),
            3 => Ok(Self::Completed),
            4 => Ok(Self::RevisionRequested),
            5 => Ok(Self::Accepted),
            _ => Err(LoomError::corrupt("unknown workgraph state")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkgraphFact {
    pub event_id: String,
    pub occurred_at: u64,
    pub task_id: String,
    pub batch_id: String,
    pub actor_kind: ActorKind,
    pub actor_id: String,
    pub correlation_id: String,
    pub causation_id: String,
    pub attempt: u64,
    pub previous_state: WorkgraphState,
    pub next_state: WorkgraphState,
    pub payload_digest: Digest,
    pub reason_code: Option<String>,
    pub kind: WorkgraphFactKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkgraphFactLog {
    pub facts: Vec<WorkgraphFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkgraphOperationRecord {
    pub sequence: u64,
    pub operation_id: String,
    pub operation_kind: String,
    pub task_id: String,
    pub event_id: String,
    pub root_after: Digest,
    pub envelope: Vec<u8>,
    pub fact: WorkgraphFact,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkgraphOperationLog {
    pub workspace_id: String,
    pub records: Vec<WorkgraphOperationRecord>,
}

impl WorkgraphFactLog {
    pub fn new(facts: Vec<WorkgraphFact>) -> Result<Self> {
        let log = Self { facts };
        log.validate()?;
        Ok(log)
    }

    pub fn append(&mut self, fact: WorkgraphFact) -> Result<()> {
        fact.validate()?;
        if self
            .facts
            .iter()
            .any(|existing| existing.event_id == fact.event_id)
        {
            return Err(LoomError::invalid("workgraph event ids must be unique"));
        }
        if self.facts.iter().any(|existing| {
            existing.task_id == fact.task_id
                && existing.previous_state == fact.previous_state
                && existing.next_state == fact.next_state
                && existing.kind == fact.kind
        }) {
            return Err(LoomError::invalid(
                "duplicate workgraph transition identity",
            ));
        }
        self.facts.push(fact);
        Ok(())
    }

    pub fn page(&self, next: usize, max: usize) -> (&[WorkgraphFact], usize) {
        let start = next.min(self.facts.len());
        let end = start.saturating_add(max).min(self.facts.len());
        (&self.facts[start..end], end)
    }

    fn validate(&self) -> Result<()> {
        let mut events = BTreeSet::new();
        let mut transitions = BTreeSet::new();
        for fact in &self.facts {
            fact.validate()?;
            if !events.insert(fact.event_id.clone()) {
                return Err(LoomError::invalid("workgraph event ids must be unique"));
            }
            if !transitions.insert((
                fact.task_id.clone(),
                fact.previous_state.tag(),
                fact.next_state.tag(),
                fact.kind.tag(),
            )) {
                return Err(LoomError::invalid(
                    "duplicate workgraph transition identity",
                ));
            }
        }
        Ok(())
    }
}

impl WorkgraphFact {
    pub fn validate(&self) -> Result<()> {
        for (n, v) in [
            ("workgraph event", &self.event_id),
            ("workgraph task", &self.task_id),
            ("workgraph batch", &self.batch_id),
            ("workgraph actor", &self.actor_id),
            ("workgraph correlation", &self.correlation_id),
            ("workgraph causation", &self.causation_id),
        ] {
            validate_text(n, v)?;
        }
        if self.attempt == 0 {
            return Err(LoomError::invalid("workgraph attempt must be at least 1"));
        }
        if let Some(reason) = &self.reason_code {
            validate_text("workgraph reason", reason)?;
        }
        if !valid_transition(self.previous_state, self.next_state, self.kind) {
            return Err(LoomError::invalid("invalid workgraph state transition"));
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        loom_codec::encode(&self.to_value()).map_err(|e| LoomError::corrupt(e.to_string()))
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(|e| LoomError::corrupt(e.to_string()))?)
    }
    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(WORKGRAPH_FACT_SCHEMA.into()),
            Value::Array(vec![
                Value::Text(self.event_id.clone()),
                Value::Uint(self.occurred_at),
                Value::Text(self.task_id.clone()),
                Value::Text(self.batch_id.clone()),
                Value::Uint(match self.actor_kind {
                    ActorKind::User => 0,
                    ActorKind::Agent => 1,
                    ActorKind::Service => 2,
                }),
                Value::Text(self.actor_id.clone()),
                Value::Text(self.correlation_id.clone()),
                Value::Text(self.causation_id.clone()),
                Value::Uint(self.attempt),
                Value::Uint(self.previous_state.tag()),
                Value::Uint(self.next_state.tag()),
                Value::Text(self.payload_digest.to_string()),
                match &self.reason_code {
                    Some(v) => Value::Text(v.clone()),
                    None => Value::Null,
                },
                Value::Uint(self.kind.tag()),
            ]),
        ])
    }
    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "workgraph fact")?;
        outer.expect_text(WORKGRAPH_FACT_SCHEMA)?;
        let mut f = Fields::array(outer.next("workgraph fact fields")?, "workgraph fact")?;
        outer.end("workgraph fact")?;
        let event_id = f.text("event_id")?;
        let occurred_at = f.uint("occurred_at")?;
        let task_id = f.text("task_id")?;
        let batch_id = f.text("batch_id")?;
        let actor_kind = match f.uint("actor_kind")? {
            0 => ActorKind::User,
            1 => ActorKind::Agent,
            2 => ActorKind::Service,
            _ => return Err(LoomError::corrupt("unknown actor kind")),
        };
        let actor_id = f.text("actor_id")?;
        let correlation_id = f.text("correlation_id")?;
        let causation_id = f.text("causation_id")?;
        let attempt = f.uint("attempt")?;
        let previous_state = WorkgraphState::from_tag(f.uint("previous_state")?)?;
        let next_state = WorkgraphState::from_tag(f.uint("next_state")?)?;
        let payload_digest = f.digest("payload_digest")?;
        let reason_code = match f.next("reason_code")? {
            Value::Null => None,
            Value::Text(v) => Some(v),
            _ => return Err(LoomError::corrupt("workgraph reason must be text or null")),
        };
        let kind = WorkgraphFactKind::from_tag(f.uint("kind")?)?;
        f.end("workgraph fact")?;
        let fact = Self {
            event_id,
            occurred_at,
            task_id,
            batch_id,
            actor_kind,
            actor_id,
            correlation_id,
            causation_id,
            attempt,
            previous_state,
            next_state,
            payload_digest,
            reason_code,
            kind,
        };
        fact.validate()?;
        Ok(fact)
    }
}

impl WorkgraphOperationRecord {
    pub fn new(
        sequence: u64,
        operation_id: impl Into<String>,
        operation_kind: impl Into<String>,
        task_id: impl Into<String>,
        event_id: impl Into<String>,
        root_after: Digest,
        envelope: Vec<u8>,
        fact: WorkgraphFact,
    ) -> Result<Self> {
        let record = Self {
            sequence,
            operation_id: operation_id.into(),
            operation_kind: operation_kind.into(),
            task_id: task_id.into(),
            event_id: event_id.into(),
            root_after,
            envelope,
            fact,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn fact(
        sequence: u64,
        fact: WorkgraphFact,
        root_after: Digest,
        envelope: Vec<u8>,
    ) -> Result<Self> {
        Self::new(
            sequence,
            fact.event_id.clone(),
            workgraph_operation_kind(fact.kind),
            fact.task_id.clone(),
            fact.event_id.clone(),
            root_after,
            envelope,
            fact,
        )
    }

    fn validate(&self) -> Result<()> {
        if self.sequence == 0 {
            return Err(LoomError::invalid(
                "workgraph operation sequence must be at least 1",
            ));
        }
        validate_text("workgraph operation_id", &self.operation_id)?;
        validate_text("workgraph operation_kind", &self.operation_kind)?;
        validate_text("workgraph operation task_id", &self.task_id)?;
        validate_text("workgraph operation event_id", &self.event_id)?;
        if self.envelope.is_empty() {
            return Err(LoomError::invalid(
                "workgraph operation envelope must not be empty",
            ));
        }
        self.fact.validate()?;
        if self.fact.task_id != self.task_id {
            return Err(LoomError::corrupt(
                "workgraph operation task id does not match fact",
            ));
        }
        if self.fact.event_id != self.event_id {
            return Err(LoomError::corrupt(
                "workgraph operation event id does not match fact",
            ));
        }
        let envelope = OperationEnvelope::decode(&self.envelope)?;
        if envelope.operation_id != self.operation_id {
            return Err(LoomError::corrupt(
                "workgraph operation id does not match envelope",
            ));
        }
        if envelope.operation_kind != self.operation_kind {
            return Err(LoomError::corrupt(
                "workgraph operation kind does not match envelope",
            ));
        }
        if envelope.sequence != self.sequence {
            return Err(LoomError::corrupt(
                "workgraph operation sequence does not match envelope",
            ));
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Uint(self.sequence),
            Value::Text(self.operation_id.clone()),
            Value::Text(self.operation_kind.clone()),
            Value::Text(self.task_id.clone()),
            Value::Text(self.event_id.clone()),
            Value::Text(self.root_after.to_string()),
            Value::Bytes(self.envelope.clone()),
            self.fact.to_value(),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "workgraph operation record")?;
        let sequence = fields.uint("sequence")?;
        let operation_id = fields.text("operation_id")?;
        let operation_kind = fields.text("operation_kind")?;
        let task_id = fields.text("task_id")?;
        let event_id = fields.text("event_id")?;
        let root_after = fields.digest("root_after")?;
        let envelope = fields.bytes("envelope")?;
        let fact = WorkgraphFact::from_value(fields.next("fact")?)?;
        fields.end("workgraph operation record")?;
        Self::new(
            sequence,
            operation_id,
            operation_kind,
            task_id,
            event_id,
            root_after,
            envelope,
            fact,
        )
    }
}

impl WorkgraphOperationLog {
    pub fn new(
        workspace_id: impl Into<String>,
        records: Vec<WorkgraphOperationRecord>,
    ) -> Result<Self> {
        let log = Self {
            workspace_id: workspace_id.into(),
            records,
        };
        log.validate()?;
        Ok(log)
    }

    pub fn append(&mut self, record: WorkgraphOperationRecord) -> Result<()> {
        record.validate()?;
        if let Some(previous) = self.records.last()
            && record.sequence <= previous.sequence
        {
            return Err(LoomError::invalid(
                "workgraph operation records must be ordered by increasing sequence",
            ));
        }
        if self
            .records
            .iter()
            .any(|existing| existing.operation_id == record.operation_id)
        {
            return Err(LoomError::invalid("workgraph operation ids must be unique"));
        }
        if self
            .records
            .iter()
            .any(|existing| existing.event_id == record.event_id)
        {
            return Err(LoomError::invalid("workgraph event ids must be unique"));
        }
        if self.records.iter().any(|existing| {
            existing.fact.task_id == record.fact.task_id
                && existing.fact.previous_state == record.fact.previous_state
                && existing.fact.next_state == record.fact.next_state
                && existing.fact.kind == record.fact.kind
        }) {
            return Err(LoomError::invalid(
                "duplicate workgraph transition identity",
            ));
        }
        self.records.push(record);
        self.validate()
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(|e| LoomError::corrupt(e.to_string()))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(|e| LoomError::corrupt(e.to_string()))?)
    }

    pub fn changes(
        &self,
        cursor: &OperationChangeCursor,
        max: usize,
    ) -> Result<OperationChangeBatch> {
        let expected_scope = workgraph_operation_cursor_scope(&self.workspace_id);
        if cursor.scope_id != expected_scope {
            return Err(LoomError::invalid(
                "operation change cursor scope does not match workgraph operation log",
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
        validate_text("workgraph operation log workspace_id", &self.workspace_id)?;
        let mut previous = None;
        let mut operation_ids = BTreeSet::new();
        let mut event_ids = BTreeSet::new();
        let mut transitions = BTreeSet::new();
        for record in &self.records {
            record.validate()?;
            if let Some(previous) = previous
                && record.sequence <= previous
            {
                return Err(LoomError::invalid(
                    "workgraph operation records must be ordered by increasing sequence",
                ));
            }
            previous = Some(record.sequence);
            if !operation_ids.insert(record.operation_id.clone()) {
                return Err(LoomError::invalid("workgraph operation ids must be unique"));
            }
            if !event_ids.insert(record.event_id.clone()) {
                return Err(LoomError::invalid("workgraph event ids must be unique"));
            }
            if !transitions.insert((
                record.fact.task_id.clone(),
                record.fact.previous_state.tag(),
                record.fact.next_state.tag(),
                record.fact.kind.tag(),
            )) {
                return Err(LoomError::invalid(
                    "duplicate workgraph transition identity",
                ));
            }
        }
        Ok(())
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(WORKGRAPH_OPERATION_LOG_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(
                    self.records
                        .iter()
                        .map(WorkgraphOperationRecord::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "workgraph operation log")?;
        outer.expect_text(WORKGRAPH_OPERATION_LOG_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("workgraph operation log fields")?,
            "workgraph operation log",
        )?;
        outer.end("workgraph operation log")?;
        let workspace_id = fields.text("workspace_id")?;
        let records = workgraph_operation_record_list(fields.next("records")?)?;
        fields.end("workgraph operation log")?;
        Self::new(workspace_id, records)
    }
}

pub fn workgraph_operation_cursor_scope(workspace_id: &str) -> String {
    format!("workgraph:{workspace_id}")
}

pub fn workgraph_operation_log_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workgraph operation log workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/operations").into_bytes())
}

pub fn workgraph_operation_kind(kind: WorkgraphFactKind) -> String {
    match kind {
        WorkgraphFactKind::AssignmentIssued => "workgraph.assignment_issued",
        WorkgraphFactKind::BoardReadAcknowledged => "workgraph.board_read_acknowledged",
        WorkgraphFactKind::BoardReadObserved => "workgraph.board_read_observed",
        WorkgraphFactKind::ResultWritten => "workgraph.result_written",
        WorkgraphFactKind::VerificationAccepted => "workgraph.verification_accepted",
        WorkgraphFactKind::RevisionRequested => "workgraph.revision_requested",
        WorkgraphFactKind::TaskBlocked => "workgraph.task_blocked",
        WorkgraphFactKind::TaskUnblocked => "workgraph.task_unblocked",
        WorkgraphFactKind::TaskCompleted => "workgraph.task_completed",
    }
    .to_string()
}

fn workgraph_operation_record_list(value: Value) -> Result<Vec<WorkgraphOperationRecord>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(WorkgraphOperationRecord::from_value)
            .collect(),
        _ => Err(LoomError::corrupt(
            "workgraph operation records must be an array",
        )),
    }
}

fn valid_transition(
    previous: WorkgraphState,
    next: WorkgraphState,
    kind: WorkgraphFactKind,
) -> bool {
    matches!(
        (previous, next, kind),
        (
            WorkgraphState::Ready,
            WorkgraphState::Assigned,
            WorkgraphFactKind::AssignmentIssued
        ) | (
            WorkgraphState::Assigned,
            WorkgraphState::Blocked,
            WorkgraphFactKind::TaskBlocked
        ) | (
            WorkgraphState::Blocked,
            WorkgraphState::Assigned,
            WorkgraphFactKind::TaskUnblocked
        ) | (
            WorkgraphState::Assigned,
            WorkgraphState::Completed,
            WorkgraphFactKind::TaskCompleted
        ) | (
            WorkgraphState::Completed,
            WorkgraphState::Accepted,
            WorkgraphFactKind::VerificationAccepted
        ) | (
            WorkgraphState::Completed,
            WorkgraphState::RevisionRequested,
            WorkgraphFactKind::RevisionRequested
        ) | (
            WorkgraphState::Assigned,
            WorkgraphState::Assigned,
            WorkgraphFactKind::BoardReadAcknowledged
        ) | (
            WorkgraphState::Assigned,
            WorkgraphState::Assigned,
            WorkgraphFactKind::BoardReadObserved
        ) | (
            WorkgraphState::Assigned,
            WorkgraphState::Assigned,
            WorkgraphFactKind::ResultWritten
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OperationEnvelopeInput, changes::OperationChangeCursor};
    use loom_types::Algo;

    fn fact(
        kind: WorkgraphFactKind,
        previous: WorkgraphState,
        next: WorkgraphState,
    ) -> WorkgraphFact {
        WorkgraphFact {
            event_id: "event-1".into(),
            occurred_at: 1,
            task_id: "task-1".into(),
            batch_id: "batch-1".into(),
            actor_kind: ActorKind::Agent,
            actor_id: "agent-1".into(),
            correlation_id: "corr-1".into(),
            causation_id: "cause-1".into(),
            attempt: 1,
            previous_state: previous,
            next_state: next,
            payload_digest: Digest::hash(Algo::Blake3, b"payload"),
            reason_code: None,
            kind,
        }
    }

    fn digest(label: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, label)
    }

    fn actor(byte: u8) -> loom_types::WorkspaceId {
        loom_types::WorkspaceId::from_bytes([byte; 16])
    }

    fn operation_record(sequence: u64, event_id: &str) -> WorkgraphOperationRecord {
        let mut fact = fact(
            WorkgraphFactKind::AssignmentIssued,
            WorkgraphState::Ready,
            WorkgraphState::Assigned,
        );
        fact.event_id = event_id.to_string();
        fact.payload_digest = digest(event_id.as_bytes());
        let payload = fact.encode().unwrap();
        let operation_kind = workgraph_operation_kind(fact.kind);
        let envelope = OperationEnvelope::new(
            Algo::Blake3,
            OperationEnvelopeInput {
                workspace_id: "studio",
                app_id: "workgraph",
                scope_id: &workgraph_operation_cursor_scope("studio"),
                operation_id: event_id,
                operation_kind: &operation_kind,
                sequence,
                actor_principal: actor(1),
                actor_kind: ActorKind::Agent,
                timestamp_ms: sequence * 10,
                idempotency_key: event_id,
                base_root: digest(b"base"),
                base_entity_version: None,
                target_entity_id: Some("workgraph:task-1"),
                payload: &payload,
                policy_labels: &["team"],
                signature: None,
                agent: None,
            },
        )
        .unwrap();
        WorkgraphOperationRecord::fact(
            sequence,
            fact,
            digest(format!("root-{sequence}").as_bytes()),
            envelope.encode().unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn canonical_fact_round_trips() {
        let fact = fact(
            WorkgraphFactKind::AssignmentIssued,
            WorkgraphState::Ready,
            WorkgraphState::Assigned,
        );
        assert_eq!(
            WorkgraphFact::decode(&fact.encode().unwrap()).unwrap(),
            fact
        );
    }

    #[test]
    fn invalid_transition_is_rejected() {
        assert!(
            fact(
                WorkgraphFactKind::TaskCompleted,
                WorkgraphState::Ready,
                WorkgraphState::Completed
            )
            .validate()
            .is_err()
        );
    }

    #[test]
    fn operation_log_round_trips_and_projects_changes() {
        let mut second = operation_record(2, "event-2");
        second.fact.previous_state = WorkgraphState::Assigned;
        second.fact.next_state = WorkgraphState::Assigned;
        second.fact.kind = WorkgraphFactKind::ResultWritten;
        second.operation_kind = workgraph_operation_kind(second.fact.kind);
        let payload = second.fact.encode().unwrap();
        let envelope = OperationEnvelope::new(
            Algo::Blake3,
            OperationEnvelopeInput {
                workspace_id: "studio",
                app_id: "workgraph",
                scope_id: &workgraph_operation_cursor_scope("studio"),
                operation_id: "event-2",
                operation_kind: &second.operation_kind,
                sequence: 2,
                actor_principal: actor(1),
                actor_kind: ActorKind::Agent,
                timestamp_ms: 20,
                idempotency_key: "event-2",
                base_root: digest(b"base"),
                base_entity_version: None,
                target_entity_id: Some("workgraph:task-1"),
                payload: &payload,
                policy_labels: &["team"],
                signature: None,
                agent: None,
            },
        )
        .unwrap();
        second.envelope = envelope.encode().unwrap();

        let log =
            WorkgraphOperationLog::new("studio", vec![operation_record(1, "event-1"), second])
                .unwrap();
        assert_eq!(
            WorkgraphOperationLog::decode(&log.encode().unwrap()).unwrap(),
            log
        );
        assert_eq!(
            workgraph_operation_log_key("studio").unwrap(),
            b"profile/workgraph/v1/studio/operations".to_vec()
        );
        let changes = log
            .changes(
                &OperationChangeCursor::new(workgraph_operation_cursor_scope("studio"), 2).unwrap(),
                10,
            )
            .unwrap();
        assert_eq!(changes.events.len(), 1);
        assert_eq!(changes.events[0].operation_kind, "workgraph.result_written");
        assert_eq!(changes.events[0].app_id, "workgraph");
        assert_eq!(changes.events[0].policy_labels, vec!["team"]);
        assert_eq!(changes.next.encode(), "oplog:3:workgraph:studio");
    }

    #[test]
    fn operation_log_rejects_duplicate_transition_identity() {
        let first = operation_record(1, "event-1");
        let second = operation_record(2, "event-2");
        assert_eq!(
            WorkgraphOperationLog::new("studio", vec![first, second])
                .unwrap_err()
                .code,
            loom_types::Code::InvalidArgument
        );
    }
}
