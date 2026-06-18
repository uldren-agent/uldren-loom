use loom_types::{Digest, LoomError, Result};

use crate::sequencer::SequencedOperation;
use crate::validate_text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeCursor {
    pub encoded: String,
}

impl ChangeCursor {
    pub fn new(encoded: impl Into<String>) -> Result<Self> {
        let encoded = encoded.into();
        validate_text("change cursor", &encoded)?;
        Ok(Self { encoded })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEvent {
    pub workspace: String,
    pub ref_name: String,
    pub commit: Digest,
    pub parent: Option<Digest>,
    pub seq: u64,
    pub domain_changes: Vec<DomainChangeRecord>,
    pub unsupported_domains: Vec<UnsupportedDomainRecord>,
    pub lmdiff: Option<Vec<u8>>,
}

impl ChangeEvent {
    pub fn validate(&self) -> Result<()> {
        validate_text("change workspace", &self.workspace)?;
        validate_text("change ref", &self.ref_name)?;
        if self.parent.is_none() && self.lmdiff.is_some() {
            return Err(LoomError::invalid(
                "root change event must not carry LMDIFF",
            ));
        }
        for change in &self.domain_changes {
            change.validate()?;
        }
        for unsupported in &self.unsupported_domains {
            unsupported.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainChangeRecord {
    pub domain: String,
    pub schema_version: u32,
    pub kind: String,
    pub key: Vec<u8>,
    pub before: Option<Digest>,
    pub after: Option<Digest>,
    pub detail: Option<Vec<u8>>,
}

impl DomainChangeRecord {
    pub fn validate(&self) -> Result<()> {
        validate_text("domain change domain", &self.domain)?;
        validate_text("domain change kind", &self.kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedDomainRecord {
    pub domain: String,
    pub capability: String,
}

impl UnsupportedDomainRecord {
    pub fn validate(&self) -> Result<()> {
        validate_text("unsupported domain", &self.domain)?;
        validate_text("unsupported capability", &self.capability)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeBatch {
    pub events: Vec<ChangeEvent>,
    pub next: ChangeCursor,
}

impl ChangeBatch {
    pub fn new(events: Vec<ChangeEvent>, next: ChangeCursor) -> Result<Self> {
        for event in &events {
            event.validate()?;
        }
        Ok(Self { events, next })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationChangeCursor {
    pub scope_id: String,
    pub next_sequence: u64,
}

impl OperationChangeCursor {
    pub fn new(scope_id: impl Into<String>, next_sequence: u64) -> Result<Self> {
        let scope_id = scope_id.into();
        validate_text("operation change scope", &scope_id)?;
        if next_sequence == 0 {
            return Err(LoomError::invalid(
                "operation change next_sequence must be at least 1",
            ));
        }
        Ok(Self {
            scope_id,
            next_sequence,
        })
    }

    pub fn start(scope_id: impl Into<String>) -> Result<Self> {
        Self::new(scope_id, 1)
    }

    pub fn encode(&self) -> String {
        format!("oplog:{}:{}", self.next_sequence, self.scope_id)
    }

    pub fn decode(encoded: &str) -> Result<Self> {
        let mut parts = encoded.splitn(3, ':');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("oplog"), Some(sequence), Some(scope_id)) => {
                let next_sequence = sequence.parse::<u64>().map_err(|_| {
                    LoomError::invalid("operation change cursor sequence is not a u64")
                })?;
                Self::new(scope_id, next_sequence)
            }
            _ => Err(LoomError::invalid("invalid operation change cursor")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationChangeRecord {
    pub workspace_id: String,
    pub app_id: String,
    pub scope_id: String,
    pub operation_id: String,
    pub operation_kind: String,
    pub sequence: u64,
    pub actor_principal: String,
    pub timestamp_ms: u64,
    pub root_after: Digest,
    pub target_entity_id: Option<String>,
    pub payload_digest: Digest,
    pub policy_labels: Vec<String>,
}

impl OperationChangeRecord {
    pub fn from_sequenced(operation: &SequencedOperation) -> Result<Self> {
        let envelope = &operation.envelope;
        let record = Self {
            workspace_id: envelope.workspace_id.clone(),
            app_id: envelope.app_id.clone(),
            scope_id: envelope.scope_id.clone(),
            operation_id: envelope.operation_id.clone(),
            operation_kind: envelope.operation_kind.clone(),
            sequence: envelope.sequence,
            actor_principal: envelope.actor_principal.to_string(),
            timestamp_ms: envelope.timestamp_ms,
            root_after: operation.root_after,
            target_entity_id: envelope.target_entity_id.clone(),
            payload_digest: envelope.payload_digest,
            policy_labels: envelope.policy_labels.clone(),
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<()> {
        validate_text("operation change organization", &self.workspace_id)?;
        validate_text("operation change app", &self.app_id)?;
        validate_text("operation change scope", &self.scope_id)?;
        validate_text("operation change id", &self.operation_id)?;
        validate_text("operation change kind", &self.operation_kind)?;
        if let Some(target) = &self.target_entity_id {
            validate_text("operation change target", target)?;
        }
        if self.sequence == 0 {
            return Err(LoomError::invalid(
                "operation change sequence must be at least 1",
            ));
        }
        for label in &self.policy_labels {
            validate_text("operation change policy label", label)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationChangeBatch {
    pub events: Vec<OperationChangeRecord>,
    pub next: OperationChangeCursor,
}

pub fn operation_log_changes(
    operations: &[SequencedOperation],
    cursor: &OperationChangeCursor,
    max: usize,
) -> Result<OperationChangeBatch> {
    let mut events = Vec::new();
    let mut next_sequence = cursor.next_sequence;
    for operation in operations {
        if operation.envelope.scope_id != cursor.scope_id {
            return Err(LoomError::invalid(
                "operation change cursor scope does not match operation log",
            ));
        }
        if operation.envelope.sequence < cursor.next_sequence {
            continue;
        }
        if events.len() == max {
            break;
        }
        let record = OperationChangeRecord::from_sequenced(operation)?;
        next_sequence = record.sequence + 1;
        events.push(record);
    }
    Ok(OperationChangeBatch {
        events,
        next: OperationChangeCursor::new(cursor.scope_id.clone(), next_sequence)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ActorKind;
    use crate::sequencer::{LocalSequencer, NoopSequencerHooks, OperationDraft, SequenceRequest};
    use loom_types::{Algo, WorkspaceId};

    fn digest(value: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, value)
    }

    #[test]
    fn root_event_rejects_lmdiff() {
        let event = ChangeEvent {
            workspace: "app".to_string(),
            ref_name: "main".to_string(),
            commit: digest(b"c1"),
            parent: None,
            seq: 1,
            domain_changes: Vec::new(),
            unsupported_domains: Vec::new(),
            lmdiff: Some(vec![1]),
        };
        assert!(event.validate().is_err());
    }

    #[test]
    fn change_batch_validates_events_and_cursor() {
        let event = ChangeEvent {
            workspace: "app".to_string(),
            ref_name: "main".to_string(),
            commit: digest(b"c2"),
            parent: Some(digest(b"c1")),
            seq: 2,
            domain_changes: vec![DomainChangeRecord {
                domain: "files".to_string(),
                schema_version: 1,
                kind: "added".to_string(),
                key: b"a.txt".to_vec(),
                before: None,
                after: Some(digest(b"blob")),
                detail: None,
            }],
            unsupported_domains: Vec::new(),
            lmdiff: Some(b"LMDIFF".to_vec()),
        };
        let batch = ChangeBatch::new(vec![event], ChangeCursor::new("cursor").unwrap()).unwrap();
        assert_eq!(batch.events.len(), 1);
    }

    #[test]
    fn operation_log_changes_page_by_sequence() {
        let mut sequencer = LocalSequencer::new();
        sequencer
            .register_scope("project", digest(b"root0"))
            .unwrap();
        let mut hooks = NoopSequencerHooks;
        let first = sequencer
            .sequence(
                Algo::Blake3,
                request(digest(b"root0"), digest(b"root1"), "a"),
                &mut hooks,
            )
            .unwrap();
        sequencer
            .sequence(
                Algo::Blake3,
                request(first.root_after, digest(b"root2"), "b"),
                &mut hooks,
            )
            .unwrap();

        let cursor = OperationChangeCursor::start("project").unwrap();
        let batch = operation_log_changes(sequencer.operations("project"), &cursor, 1).unwrap();
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].operation_id, "op-a");
        assert_eq!(batch.next.encode(), "oplog:2:project");

        let next = OperationChangeCursor::decode(&batch.next.encode()).unwrap();
        let batch = operation_log_changes(sequencer.operations("project"), &next, 10).unwrap();
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].operation_id, "op-b");
        assert_eq!(batch.next.next_sequence, 3);
    }

    fn request(base_root: Digest, root_after: Digest, key: &str) -> SequenceRequest {
        SequenceRequest {
            draft: OperationDraft {
                workspace_id: "organization".to_string(),
                app_id: "tickets".to_string(),
                scope_id: "project".to_string(),
                operation_id: format!("op-{key}"),
                operation_kind: "ticket.updated".to_string(),
                actor_principal: WorkspaceId::from_bytes([7; 16]),
                actor_kind: ActorKind::User,
                timestamp_ms: 42,
                idempotency_key: key.to_string(),
                base_root,
                base_entity_version: None,
                target_entity_id: Some("LOOM-1".to_string()),
                payload: key.as_bytes().to_vec(),
                policy_labels: vec!["audit".to_string()],
                signature: None,
                agent: None,
            },
            root_after,
            alias_requests: Vec::new(),
            order_token_requests: Vec::new(),
        }
    }
}
