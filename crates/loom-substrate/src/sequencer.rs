use std::collections::BTreeMap;

use loom_codec::Value;
use loom_types::{Algo, Digest, LoomError, Result, WorkspaceId};

use crate::changes::{OperationChangeBatch, OperationChangeCursor, operation_log_changes};
use crate::{ActorKind, OperationEnvelope, OperationEnvelopeInput, codec_error};

pub const SEQUENCED_OPERATION_SCHEMA: &str = "loom.substrate.sequenced-operation.v1";
pub const OPERATION_LOG_SCHEMA: &str = "loom.substrate.operation-log.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationDraft {
    pub workspace_id: String,
    pub app_id: String,
    pub scope_id: String,
    pub operation_id: String,
    pub operation_kind: String,
    pub actor_principal: WorkspaceId,
    pub actor_kind: ActorKind,
    pub timestamp_ms: u64,
    pub idempotency_key: String,
    pub base_root: Digest,
    pub base_entity_version: Option<String>,
    pub target_entity_id: Option<String>,
    pub payload: Vec<u8>,
    pub policy_labels: Vec<String>,
    pub signature: Option<Vec<u8>>,
    pub agent: Option<crate::AgentIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasRequest {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasAssignment {
    pub kind: String,
    pub text: String,
    pub entity_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderTokenRequest {
    pub list_id: String,
    pub entity_id: String,
    pub after: Option<String>,
    pub before: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderTokenAssignment {
    pub list_id: String,
    pub entity_id: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequencerWakeup {
    pub scope_id: String,
    pub sequence: u64,
    pub operation_id: String,
    pub operation_kind: String,
    pub root_after: Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequencedOperation {
    pub envelope: OperationEnvelope,
    pub root_after: Digest,
    pub aliases: Vec<AliasAssignment>,
    pub order_tokens: Vec<OrderTokenAssignment>,
}

impl SequencedOperation {
    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(SEQUENCED_OPERATION_SCHEMA.to_string()),
            Value::Array(vec![
                self.envelope.to_value(),
                Value::Text(self.root_after.to_string()),
                Value::Array(self.aliases.iter().map(alias_value).collect()),
                Value::Array(self.order_tokens.iter().map(order_token_value).collect()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ValueFields::array(value, "sequenced operation")?;
        outer.expect_text(SEQUENCED_OPERATION_SCHEMA)?;
        let mut fields = ValueFields::array(
            outer.next("sequenced operation fields")?,
            "sequenced operation",
        )?;
        outer.end("sequenced operation")?;
        let envelope = OperationEnvelope::from_value(fields.next("envelope")?)?;
        let root_after = fields.digest("root_after")?;
        let aliases = alias_list(fields.next("aliases")?)?;
        let order_tokens = order_token_list(fields.next("order_tokens")?)?;
        fields.end("sequenced operation")?;
        Ok(Self {
            envelope,
            root_after,
            aliases,
            order_tokens,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequenceRequest {
    pub draft: OperationDraft,
    pub root_after: Digest,
    pub alias_requests: Vec<AliasRequest>,
    pub order_token_requests: Vec<OrderTokenRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationLog {
    pub scope_id: String,
    pub operations: Vec<SequencedOperation>,
}

impl OperationLog {
    pub fn new(scope_id: impl Into<String>, operations: Vec<SequencedOperation>) -> Result<Self> {
        let scope_id = scope_id.into();
        validate_text("operation log scope", &scope_id)?;
        let log = Self {
            scope_id,
            operations,
        };
        log.validate()?;
        Ok(log)
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(OPERATION_LOG_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.scope_id.clone()),
                Value::Array(
                    self.operations
                        .iter()
                        .map(SequencedOperation::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ValueFields::array(value, "operation log")?;
        outer.expect_text(OPERATION_LOG_SCHEMA)?;
        let mut fields = ValueFields::array(outer.next("operation log fields")?, "operation log")?;
        outer.end("operation log")?;
        let scope_id = fields.text("scope_id")?;
        let operations = sequenced_operation_list(fields.next("operations")?)?;
        fields.end("operation log")?;
        Self::new(scope_id, operations)
    }

    fn validate(&self) -> Result<()> {
        let mut previous_sequence = None;
        for operation in &self.operations {
            if operation.envelope.scope_id != self.scope_id {
                return Err(LoomError::invalid(
                    "operation log record scope does not match log scope",
                ));
            }
            if let Some(previous_sequence) = previous_sequence
                && operation.envelope.sequence <= previous_sequence
            {
                return Err(LoomError::invalid(
                    "operation log sequence must be strictly increasing",
                ));
            }
            previous_sequence = Some(operation.envelope.sequence);
        }
        Ok(())
    }
}

pub trait SequencerHooks {
    fn allocate_aliases(
        &mut self,
        _envelope: &OperationEnvelope,
        requests: &[AliasRequest],
    ) -> Result<Vec<AliasAssignment>> {
        if requests.is_empty() {
            Ok(Vec::new())
        } else {
            Err(LoomError::unsupported("alias allocation hook unavailable"))
        }
    }

    fn allocate_order_tokens(
        &mut self,
        _envelope: &OperationEnvelope,
        requests: &[OrderTokenRequest],
    ) -> Result<Vec<OrderTokenAssignment>> {
        if requests.is_empty() {
            Ok(Vec::new())
        } else {
            Err(LoomError::unsupported(
                "order-token allocation hook unavailable",
            ))
        }
    }

    fn wake(&mut self, _wakeup: &SequencerWakeup) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct NoopSequencerHooks;

impl SequencerHooks for NoopSequencerHooks {}

#[derive(Debug, Clone)]
pub struct LocalSequencer {
    root_by_scope: BTreeMap<String, Digest>,
    next_sequence_by_scope: BTreeMap<String, u64>,
    idempotency: BTreeMap<IdempotencyKey, SequencedOperation>,
    log_by_scope: BTreeMap<String, Vec<SequencedOperation>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct IdempotencyKey {
    scope_id: String,
    actor_principal: WorkspaceId,
    idempotency_key: String,
}

impl LocalSequencer {
    pub fn new() -> Self {
        Self {
            root_by_scope: BTreeMap::new(),
            next_sequence_by_scope: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            log_by_scope: BTreeMap::new(),
        }
    }

    pub fn register_scope(&mut self, scope_id: impl Into<String>, root: Digest) -> Result<()> {
        let scope_id = scope_id.into();
        validate_text("scope_id", &scope_id)?;
        if self.root_by_scope.insert(scope_id.clone(), root).is_some() {
            return Err(LoomError::invalid("sequencer scope already registered"));
        }
        self.next_sequence_by_scope.insert(scope_id, 1);
        Ok(())
    }

    pub fn current_root(&self, scope_id: &str) -> Result<Digest> {
        self.root_by_scope
            .get(scope_id)
            .copied()
            .ok_or_else(|| LoomError::not_found("sequencer scope not registered"))
    }

    pub fn operations(&self, scope_id: &str) -> &[SequencedOperation] {
        self.log_by_scope
            .get(scope_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn operation_log(&self, scope_id: &str) -> Result<OperationLog> {
        if !self.root_by_scope.contains_key(scope_id) {
            return Err(LoomError::not_found("sequencer scope not registered"));
        }
        OperationLog::new(scope_id, self.operations(scope_id).to_vec())
    }

    pub fn changes(
        &self,
        cursor: &OperationChangeCursor,
        max: usize,
    ) -> Result<OperationChangeBatch> {
        operation_log_changes(self.operations(&cursor.scope_id), cursor, max)
    }

    pub fn sequence(
        &mut self,
        algo: Algo,
        request: SequenceRequest,
        hooks: &mut dyn SequencerHooks,
    ) -> Result<SequencedOperation> {
        validate_text("scope_id", &request.draft.scope_id)?;
        let key = IdempotencyKey {
            scope_id: request.draft.scope_id.clone(),
            actor_principal: request.draft.actor_principal,
            idempotency_key: request.draft.idempotency_key.clone(),
        };
        if let Some(existing) = self.idempotency.get(&key) {
            return Ok(existing.clone());
        }
        let current_root = self.current_root(&request.draft.scope_id)?;
        if current_root != request.draft.base_root {
            return Err(LoomError::new(
                loom_types::Code::Conflict,
                "operation base root does not match sequencer root",
            ));
        }
        let sequence = *self
            .next_sequence_by_scope
            .get(&request.draft.scope_id)
            .ok_or_else(|| LoomError::not_found("sequencer scope not registered"))?;
        let policy_labels = request
            .draft
            .policy_labels
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let envelope = OperationEnvelope::new(
            algo,
            OperationEnvelopeInput {
                workspace_id: &request.draft.workspace_id,
                app_id: &request.draft.app_id,
                scope_id: &request.draft.scope_id,
                operation_id: &request.draft.operation_id,
                operation_kind: &request.draft.operation_kind,
                sequence,
                actor_principal: request.draft.actor_principal,
                actor_kind: request.draft.actor_kind,
                timestamp_ms: request.draft.timestamp_ms,
                idempotency_key: &request.draft.idempotency_key,
                base_root: request.draft.base_root,
                base_entity_version: request.draft.base_entity_version.as_deref(),
                target_entity_id: request.draft.target_entity_id.as_deref(),
                payload: &request.draft.payload,
                policy_labels: &policy_labels,
                signature: request.draft.signature.as_deref(),
                agent: request.draft.agent,
            },
        )?;
        let aliases = hooks.allocate_aliases(&envelope, &request.alias_requests)?;
        let order_tokens = hooks.allocate_order_tokens(&envelope, &request.order_token_requests)?;
        let result = SequencedOperation {
            envelope,
            root_after: request.root_after,
            aliases,
            order_tokens,
        };
        hooks.wake(&SequencerWakeup {
            scope_id: result.envelope.scope_id.clone(),
            sequence: result.envelope.sequence,
            operation_id: result.envelope.operation_id.clone(),
            operation_kind: result.envelope.operation_kind.clone(),
            root_after: result.root_after,
        })?;
        self.root_by_scope
            .insert(result.envelope.scope_id.clone(), result.root_after);
        self.next_sequence_by_scope
            .insert(result.envelope.scope_id.clone(), sequence + 1);
        self.log_by_scope
            .entry(result.envelope.scope_id.clone())
            .or_default()
            .push(result.clone());
        self.idempotency.insert(key, result.clone());
        Ok(result)
    }
}

impl Default for LocalSequencer {
    fn default() -> Self {
        Self::new()
    }
}

fn alias_value(alias: &AliasAssignment) -> Value {
    Value::Array(vec![
        Value::Text(alias.kind.clone()),
        Value::Text(alias.text.clone()),
        Value::Text(alias.entity_id.clone()),
    ])
}

fn order_token_value(order_token: &OrderTokenAssignment) -> Value {
    Value::Array(vec![
        Value::Text(order_token.list_id.clone()),
        Value::Text(order_token.entity_id.clone()),
        Value::Text(order_token.token.clone()),
    ])
}

fn alias_list(value: Value) -> Result<Vec<AliasAssignment>> {
    let mut out = Vec::new();
    for value in value_array(value, "aliases")? {
        let mut fields = ValueFields::array(value, "alias assignment")?;
        let alias = AliasAssignment {
            kind: fields.text("kind")?,
            text: fields.text("text")?,
            entity_id: fields.text("entity_id")?,
        };
        fields.end("alias assignment")?;
        validate_text("alias kind", &alias.kind)?;
        validate_text("alias text", &alias.text)?;
        validate_text("alias entity_id", &alias.entity_id)?;
        out.push(alias);
    }
    Ok(out)
}

fn order_token_list(value: Value) -> Result<Vec<OrderTokenAssignment>> {
    let mut out = Vec::new();
    for value in value_array(value, "order tokens")? {
        let mut fields = ValueFields::array(value, "order token assignment")?;
        let order_token = OrderTokenAssignment {
            list_id: fields.text("list_id")?,
            entity_id: fields.text("entity_id")?,
            token: fields.text("token")?,
        };
        fields.end("order token assignment")?;
        validate_text("order token list_id", &order_token.list_id)?;
        validate_text("order token entity_id", &order_token.entity_id)?;
        validate_text("order token token", &order_token.token)?;
        out.push(order_token);
    }
    Ok(out)
}

fn sequenced_operation_list(value: Value) -> Result<Vec<SequencedOperation>> {
    value_array(value, "sequenced operations")?
        .into_iter()
        .map(SequencedOperation::from_value)
        .collect()
}

fn value_array(value: Value, name: &str) -> Result<Vec<Value>> {
    match value {
        Value::Array(values) => Ok(values),
        _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
    }
}

struct ValueFields {
    values: std::vec::IntoIter<Value>,
    context: &'static str,
}

impl ValueFields {
    fn array(value: Value, context: &'static str) -> Result<Self> {
        Ok(Self {
            values: value_array(value, context)?.into_iter(),
            context,
        })
    }

    fn next(&mut self, name: &str) -> Result<Value> {
        self.values
            .next()
            .ok_or_else(|| LoomError::corrupt(format!("{} missing {name}", self.context)))
    }

    fn end(&mut self, name: &str) -> Result<()> {
        if self.values.next().is_some() {
            return Err(LoomError::corrupt(format!("{name} has trailing fields")));
        }
        Ok(())
    }

    fn expect_text(&mut self, expected: &str) -> Result<()> {
        match self.next("schema")? {
            Value::Text(actual) if actual == expected => Ok(()),
            _ => Err(LoomError::corrupt(format!(
                "{} schema mismatch",
                self.context
            ))),
        }
    }

    fn text(&mut self, name: &str) -> Result<String> {
        match self.next(name)? {
            Value::Text(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be text"))),
        }
    }

    fn digest(&mut self, name: &str) -> Result<Digest> {
        Digest::parse(&self.text(name)?)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingHooks {
        wakeups: Vec<SequencerWakeup>,
    }

    impl SequencerHooks for RecordingHooks {
        fn allocate_aliases(
            &mut self,
            envelope: &OperationEnvelope,
            requests: &[AliasRequest],
        ) -> Result<Vec<AliasAssignment>> {
            Ok(requests
                .iter()
                .map(|request| AliasAssignment {
                    kind: request.kind.clone(),
                    text: request.text.clone(),
                    entity_id: envelope
                        .target_entity_id
                        .clone()
                        .unwrap_or_else(|| envelope.operation_id.clone()),
                })
                .collect())
        }

        fn allocate_order_tokens(
            &mut self,
            _envelope: &OperationEnvelope,
            requests: &[OrderTokenRequest],
        ) -> Result<Vec<OrderTokenAssignment>> {
            Ok(requests
                .iter()
                .enumerate()
                .map(|(idx, request)| OrderTokenAssignment {
                    list_id: request.list_id.clone(),
                    entity_id: request.entity_id.clone(),
                    token: format!("t{idx:04}"),
                })
                .collect())
        }

        fn wake(&mut self, wakeup: &SequencerWakeup) -> Result<()> {
            self.wakeups.push(wakeup.clone());
            Ok(())
        }
    }

    struct FailingWakeHooks;

    impl SequencerHooks for FailingWakeHooks {
        fn wake(&mut self, _wakeup: &SequencerWakeup) -> Result<()> {
            Err(LoomError::unsupported("wake failed"))
        }
    }

    fn id(byte: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([byte; 16])
    }

    fn digest(label: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, label)
    }

    fn draft(base_root: Digest, idempotency_key: &str) -> OperationDraft {
        OperationDraft {
            workspace_id: "organization".to_string(),
            app_id: "tickets".to_string(),
            scope_id: "project".to_string(),
            operation_id: format!("op-{idempotency_key}"),
            operation_kind: "ticket.created".to_string(),
            actor_principal: id(1),
            actor_kind: ActorKind::User,
            timestamp_ms: 100,
            idempotency_key: idempotency_key.to_string(),
            base_root,
            base_entity_version: None,
            target_entity_id: Some("LOOM-1".to_string()),
            payload: b"payload".to_vec(),
            policy_labels: vec!["write".to_string()],
            signature: None,
            agent: None,
        }
    }

    #[test]
    fn local_sequencer_assigns_sequence_advances_root_and_wakes() {
        let base = digest(b"base");
        let next = digest(b"next");
        let mut sequencer = LocalSequencer::new();
        sequencer.register_scope("project", base).unwrap();
        let mut hooks = RecordingHooks::default();
        let result = sequencer
            .sequence(
                Algo::Blake3,
                SequenceRequest {
                    draft: draft(base, "a"),
                    root_after: next,
                    alias_requests: vec![AliasRequest {
                        kind: "ticket".to_string(),
                        text: "LOOM-1".to_string(),
                    }],
                    order_token_requests: vec![OrderTokenRequest {
                        list_id: "backlog".to_string(),
                        entity_id: "LOOM-1".to_string(),
                        after: None,
                        before: None,
                    }],
                },
                &mut hooks,
            )
            .unwrap();
        assert_eq!(result.envelope.sequence, 1);
        assert_eq!(result.envelope.base_root, base);
        assert_eq!(sequencer.current_root("project").unwrap(), next);
        assert_eq!(result.aliases[0].entity_id, "LOOM-1");
        assert_eq!(result.order_tokens[0].token, "t0000");
        assert_eq!(hooks.wakeups[0].sequence, 1);
        assert_eq!(sequencer.operations("project").len(), 1);
    }

    #[test]
    fn local_sequencer_is_idempotent() {
        let base = digest(b"base");
        let next = digest(b"next");
        let mut sequencer = LocalSequencer::new();
        sequencer.register_scope("project", base).unwrap();
        let mut hooks = RecordingHooks::default();
        let request = SequenceRequest {
            draft: draft(base, "same"),
            root_after: next,
            alias_requests: Vec::new(),
            order_token_requests: Vec::new(),
        };
        let first = sequencer
            .sequence(Algo::Blake3, request.clone(), &mut hooks)
            .unwrap();
        let second = sequencer
            .sequence(Algo::Blake3, request, &mut hooks)
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(sequencer.operations("project").len(), 1);
        assert_eq!(hooks.wakeups.len(), 1);
    }

    #[test]
    fn operation_log_round_trips_canonical_bytes() {
        let base = digest(b"base");
        let next = digest(b"next");
        let mut sequencer = LocalSequencer::new();
        sequencer.register_scope("project", base).unwrap();
        let mut hooks = RecordingHooks::default();
        sequencer
            .sequence(
                Algo::Blake3,
                SequenceRequest {
                    draft: draft(base, "a"),
                    root_after: next,
                    alias_requests: vec![AliasRequest {
                        kind: "ticket".to_string(),
                        text: "LOOM-1".to_string(),
                    }],
                    order_token_requests: vec![OrderTokenRequest {
                        list_id: "backlog".to_string(),
                        entity_id: "LOOM-1".to_string(),
                        after: None,
                        before: None,
                    }],
                },
                &mut hooks,
            )
            .unwrap();
        let log = sequencer.operation_log("project").unwrap();
        let encoded = log.encode().unwrap();
        let decoded = OperationLog::decode(&encoded).unwrap();
        assert_eq!(decoded, log);
        assert_eq!(decoded.encode().unwrap(), encoded);
        let operation = decoded.operations[0].encode().unwrap();
        assert_eq!(
            SequencedOperation::decode(&operation).unwrap(),
            decoded.operations[0]
        );
    }

    #[test]
    fn operation_log_rejects_mixed_scope_and_non_monotonic_sequences() {
        let base = digest(b"base");
        let next = digest(b"next");
        let mut sequencer = LocalSequencer::new();
        sequencer.register_scope("project", base).unwrap();
        let mut hooks = RecordingHooks::default();
        let first = sequencer
            .sequence(
                Algo::Blake3,
                SequenceRequest {
                    draft: draft(base, "a"),
                    root_after: next,
                    alias_requests: Vec::new(),
                    order_token_requests: Vec::new(),
                },
                &mut hooks,
            )
            .unwrap();
        let mut wrong_scope = first.clone();
        wrong_scope.envelope.scope_id = "other".to_string();
        assert!(OperationLog::new("project", vec![wrong_scope]).is_err());
        assert!(OperationLog::new("project", vec![first.clone(), first]).is_err());
    }

    #[test]
    fn local_sequencer_rejects_stale_base_root() {
        let base = digest(b"base");
        let mut sequencer = LocalSequencer::new();
        sequencer.register_scope("project", base).unwrap();
        let mut hooks = RecordingHooks::default();
        let err = sequencer
            .sequence(
                Algo::Blake3,
                SequenceRequest {
                    draft: draft(digest(b"stale"), "a"),
                    root_after: digest(b"next"),
                    alias_requests: Vec::new(),
                    order_token_requests: Vec::new(),
                },
                &mut hooks,
            )
            .unwrap_err();
        assert_eq!(err.code, loom_types::Code::Conflict);
        assert!(sequencer.operations("project").is_empty());
        assert!(hooks.wakeups.is_empty());
    }

    #[test]
    fn local_sequencer_does_not_commit_when_wakeup_fails() {
        let base = digest(b"base");
        let next = digest(b"next");
        let mut sequencer = LocalSequencer::new();
        sequencer.register_scope("project", base).unwrap();
        let mut hooks = FailingWakeHooks;
        let err = sequencer
            .sequence(
                Algo::Blake3,
                SequenceRequest {
                    draft: draft(base, "a"),
                    root_after: next,
                    alias_requests: Vec::new(),
                    order_token_requests: Vec::new(),
                },
                &mut hooks,
            )
            .unwrap_err();
        assert_eq!(err.code, loom_types::Code::Unsupported);
        assert_eq!(sequencer.current_root("project").unwrap(), base);
        assert!(sequencer.operations("project").is_empty());
    }
}
