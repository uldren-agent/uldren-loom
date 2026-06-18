//! Reusable operation-substrate contracts.

use loom_codec::Value;
use loom_types::{Algo, Digest, LoomError, Result, WorkspaceId};
use serde_json::{Value as JsonValue, json};

pub mod admission;
pub mod annotation;
pub mod body;
pub mod changes;
pub mod chat;
pub mod conflict;
pub mod drive;
pub mod facilities;
pub mod order_token;
pub mod predicate;
pub mod promotion;
pub mod refs;
pub mod search;
pub mod sequencer;
pub mod versioning;
pub mod view;
pub mod web;
pub mod workgraph;

pub const OPERATION_ENVELOPE_SCHEMA: &str = "loom.substrate.operation.v1";
pub const AGENT_IDENTITY_SCHEMA: &str = "loom.substrate.agent-identity.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    User,
    Agent,
    Service,
}

impl ActorKind {
    const fn tag(self) -> u64 {
        match self {
            ActorKind::User => 0,
            ActorKind::Agent => 1,
            ActorKind::Service => 2,
        }
    }

    fn from_tag(tag: u64) -> Result<Self> {
        match tag {
            0 => Ok(ActorKind::User),
            1 => Ok(ActorKind::Agent),
            2 => Ok(ActorKind::Service),
            other => Err(LoomError::corrupt(format!(
                "unknown actor kind tag {other}"
            ))),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            ActorKind::User => "user",
            ActorKind::Agent => "agent",
            ActorKind::Service => "service",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentIdentity {
    pub agent_id: String,
    pub model_or_runtime: String,
    pub operator_principal: Option<WorkspaceId>,
    pub source_entity_ids: Vec<String>,
    pub tool_calls: Vec<String>,
    pub confidence_ppm: Option<u32>,
    pub policy_labels: Vec<String>,
    pub trace_digest: Option<Digest>,
}

impl AgentIdentity {
    pub fn new(agent_id: impl Into<String>, model_or_runtime: impl Into<String>) -> Result<Self> {
        let agent_id = agent_id.into();
        let model_or_runtime = model_or_runtime.into();
        validate_text("agent_id", &agent_id)?;
        validate_text("model_or_runtime", &model_or_runtime)?;
        Ok(Self {
            agent_id,
            model_or_runtime,
            operator_principal: None,
            source_entity_ids: Vec::new(),
            tool_calls: Vec::new(),
            confidence_ppm: None,
            policy_labels: Vec::new(),
            trace_digest: None,
        })
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(AGENT_IDENTITY_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.agent_id.clone()),
                Value::Text(self.model_or_runtime.clone()),
                optional_id(self.operator_principal),
                string_array(&self.source_entity_ids),
                string_array(&self.tool_calls),
                optional_u32(self.confidence_ppm),
                string_array(&self.policy_labels),
                optional_digest(self.trace_digest),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "agent identity")?;
        outer.expect_text(AGENT_IDENTITY_SCHEMA)?;
        let mut fields = Fields::array(outer.next("agent identity fields")?, "agent identity")?;
        outer.end("agent identity")?;
        let agent_id = fields.text("agent_id")?;
        let model_or_runtime = fields.text("model_or_runtime")?;
        validate_text("agent_id", &agent_id)?;
        validate_text("model_or_runtime", &model_or_runtime)?;
        let operator_principal = fields.optional_id("operator_principal")?;
        let source_entity_ids = fields.string_array("source_entity_ids")?;
        let tool_calls = fields.string_array("tool_calls")?;
        let confidence_ppm = fields.optional_u32("confidence_ppm")?;
        if let Some(confidence_ppm) = confidence_ppm
            && confidence_ppm > 1_000_000
        {
            return Err(LoomError::corrupt("agent confidence_ppm exceeds 1000000"));
        }
        let policy_labels = canonical_labels(fields.string_array("policy_labels")?)?;
        let trace_digest = fields.optional_digest("trace_digest")?;
        fields.end("agent identity")?;
        Ok(Self {
            agent_id,
            model_or_runtime,
            operator_principal,
            source_entity_ids,
            tool_calls,
            confidence_ppm,
            policy_labels,
            trace_digest,
        })
    }

    pub fn debug_json(&self) -> JsonValue {
        json!({
            "agent_id": self.agent_id,
            "model_or_runtime": self.model_or_runtime,
            "operator_principal": self.operator_principal.map(|id| id.to_string()),
            "source_entity_ids": self.source_entity_ids,
            "tool_calls": self.tool_calls,
            "confidence_ppm": self.confidence_ppm,
            "policy_labels": self.policy_labels,
            "trace_digest": self.trace_digest.map(|digest| digest.to_string())
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationEnvelope {
    pub workspace_id: String,
    pub app_id: String,
    pub scope_id: String,
    pub operation_id: String,
    pub operation_kind: String,
    pub sequence: u64,
    pub actor_principal: WorkspaceId,
    pub actor_kind: ActorKind,
    pub timestamp_ms: u64,
    pub idempotency_key: String,
    pub base_root: Digest,
    pub base_entity_version: Option<String>,
    pub target_entity_id: Option<String>,
    pub payload_digest: Digest,
    pub payload: Vec<u8>,
    pub policy_labels: Vec<String>,
    pub signature: Option<Vec<u8>>,
    pub agent: Option<AgentIdentity>,
}

#[derive(Debug, Clone)]
pub struct OperationEnvelopeInput<'a> {
    pub workspace_id: &'a str,
    pub app_id: &'a str,
    pub scope_id: &'a str,
    pub operation_id: &'a str,
    pub operation_kind: &'a str,
    pub sequence: u64,
    pub actor_principal: WorkspaceId,
    pub actor_kind: ActorKind,
    pub timestamp_ms: u64,
    pub idempotency_key: &'a str,
    pub base_root: Digest,
    pub base_entity_version: Option<&'a str>,
    pub target_entity_id: Option<&'a str>,
    pub payload: &'a [u8],
    pub policy_labels: &'a [&'a str],
    pub signature: Option<&'a [u8]>,
    pub agent: Option<AgentIdentity>,
}

impl OperationEnvelope {
    pub fn new(algo: Algo, input: OperationEnvelopeInput<'_>) -> Result<Self> {
        validate_text("workspace_id", input.workspace_id)?;
        validate_text("app_id", input.app_id)?;
        validate_text("scope_id", input.scope_id)?;
        validate_text("operation_id", input.operation_id)?;
        validate_text("operation_kind", input.operation_kind)?;
        validate_text("idempotency_key", input.idempotency_key)?;
        let base_entity_version = optional_text("base_entity_version", input.base_entity_version)?;
        let target_entity_id = optional_text("target_entity_id", input.target_entity_id)?;
        let policy_labels = canonical_labels(
            input
                .policy_labels
                .iter()
                .map(|label| (*label).to_string())
                .collect(),
        )?;
        let agent = input.agent.map(normalize_agent).transpose()?;
        Ok(Self {
            workspace_id: input.workspace_id.to_string(),
            app_id: input.app_id.to_string(),
            scope_id: input.scope_id.to_string(),
            operation_id: input.operation_id.to_string(),
            operation_kind: input.operation_kind.to_string(),
            sequence: input.sequence,
            actor_principal: input.actor_principal,
            actor_kind: input.actor_kind,
            timestamp_ms: input.timestamp_ms,
            idempotency_key: input.idempotency_key.to_string(),
            base_root: input.base_root,
            base_entity_version,
            target_entity_id,
            payload_digest: Digest::hash(algo, input.payload),
            payload: input.payload.to_vec(),
            policy_labels,
            signature: input.signature.map(<[u8]>::to_vec),
            agent,
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
            Value::Text(OPERATION_ENVELOPE_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Text(self.app_id.clone()),
                Value::Text(self.scope_id.clone()),
                Value::Text(self.operation_id.clone()),
                Value::Text(self.operation_kind.clone()),
                Value::Uint(self.sequence),
                Value::Text(self.actor_principal.to_string()),
                Value::Uint(self.actor_kind.tag()),
                Value::Uint(self.timestamp_ms),
                Value::Text(self.idempotency_key.clone()),
                Value::Text(self.base_root.to_string()),
                optional_text_value(self.base_entity_version.as_deref()),
                optional_text_value(self.target_entity_id.as_deref()),
                Value::Text(self.payload_digest.to_string()),
                Value::Uint(self.payload.len() as u64),
                Value::Bytes(self.payload.clone()),
                string_array(&self.policy_labels),
                optional_bytes(self.signature.as_deref()),
                optional_agent(self.agent.as_ref()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "operation envelope")?;
        outer.expect_text(OPERATION_ENVELOPE_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("operation envelope fields")?,
            "operation envelope",
        )?;
        outer.end("operation envelope")?;
        let workspace_id = fields.text("workspace_id")?;
        let app_id = fields.text("app_id")?;
        let scope_id = fields.text("scope_id")?;
        let operation_id = fields.text("operation_id")?;
        let operation_kind = fields.text("operation_kind")?;
        let sequence = fields.uint("sequence")?;
        let actor_principal = fields.id("actor_principal")?;
        let actor_kind = ActorKind::from_tag(fields.uint("actor_kind")?)?;
        let timestamp_ms = fields.uint("timestamp_ms")?;
        let idempotency_key = fields.text("idempotency_key")?;
        let base_root = fields.digest("base_root")?;
        let base_entity_version = fields.optional_text("base_entity_version")?;
        let target_entity_id = fields.optional_text("target_entity_id")?;
        let payload_digest = fields.digest("payload_digest")?;
        let payload_len = fields.uint("payload_len")?;
        let payload = fields.bytes("payload")?;
        if payload.len() as u64 != payload_len {
            return Err(LoomError::corrupt("operation payload length mismatch"));
        }
        let policy_labels = canonical_labels(fields.string_array("policy_labels")?)?;
        let signature = fields.optional_bytes("signature")?;
        let agent = fields.optional_agent("agent")?;
        fields.end("operation envelope")?;
        validate_text("workspace_id", &workspace_id)?;
        validate_text("app_id", &app_id)?;
        validate_text("scope_id", &scope_id)?;
        validate_text("operation_id", &operation_id)?;
        validate_text("operation_kind", &operation_kind)?;
        validate_text("idempotency_key", &idempotency_key)?;
        Ok(Self {
            workspace_id,
            app_id,
            scope_id,
            operation_id,
            operation_kind,
            sequence,
            actor_principal,
            actor_kind,
            timestamp_ms,
            idempotency_key,
            base_root,
            base_entity_version,
            target_entity_id,
            payload_digest,
            payload,
            policy_labels,
            signature,
            agent,
        })
    }

    pub fn validate_payload_digest(&self, algo: Algo) -> Result<()> {
        let actual = Digest::hash(algo, &self.payload);
        if actual != self.payload_digest {
            return Err(LoomError::integrity_failure(
                "operation payload digest mismatch",
            ));
        }
        Ok(())
    }

    pub fn debug_json(&self) -> JsonValue {
        json!({
            "schema": OPERATION_ENVELOPE_SCHEMA,
            "workspace_id": self.workspace_id,
            "app_id": self.app_id,
            "scope_id": self.scope_id,
            "operation_id": self.operation_id,
            "operation_kind": self.operation_kind,
            "sequence": self.sequence,
            "actor_principal": self.actor_principal.to_string(),
            "actor_kind": self.actor_kind.as_str(),
            "timestamp_ms": self.timestamp_ms,
            "idempotency_key": self.idempotency_key,
            "base_root": self.base_root.to_string(),
            "base_entity_version": self.base_entity_version,
            "target_entity_id": self.target_entity_id,
            "payload_digest": self.payload_digest.to_string(),
            "payload_len": self.payload.len(),
            "policy_labels": self.policy_labels,
            "signature_len": self.signature.as_ref().map(Vec::len),
            "agent": self.agent.as_ref().map(AgentIdentity::debug_json)
        })
    }
}

#[cfg(feature = "studio-driveish")]
pub mod driveish {
    pub const APP_ID: &str = "drive";
}

#[cfg(feature = "studio-lifecycle")]
pub mod lifecycle;

#[cfg(feature = "studio-meetings")]
pub mod meetings;

#[cfg(feature = "studio-pages")]
pub mod pages;

#[cfg(feature = "studio-slackish")]
pub mod slackish {
    pub const APP_ID: &str = "chat";
}

#[cfg(feature = "studio-surfaces")]
pub mod surfaces;

#[cfg(feature = "studio-webish")]
pub mod webish {
    pub const APP_ID: &str = "web";
}

pub fn codec_error(error: impl std::fmt::Display) -> LoomError {
    LoomError::corrupt(format!("operation envelope cbor: {error}"))
}

pub fn validate_text(name: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(LoomError::invalid(format!("{name} must not be empty")));
    }
    if value.len() > 512 {
        return Err(LoomError::invalid(format!("{name} is too long")));
    }
    Ok(())
}

fn optional_text(name: &str, value: Option<&str>) -> Result<Option<String>> {
    value
        .map(|value| {
            validate_text(name, value)?;
            Ok(value.to_string())
        })
        .transpose()
}

pub(crate) fn canonical_labels(mut labels: Vec<String>) -> Result<Vec<String>> {
    for label in &labels {
        validate_text("policy label", label)?;
    }
    labels.sort();
    labels.dedup();
    Ok(labels)
}

fn normalize_agent(mut agent: AgentIdentity) -> Result<AgentIdentity> {
    if let Some(confidence_ppm) = agent.confidence_ppm
        && confidence_ppm > 1_000_000
    {
        return Err(LoomError::invalid("agent confidence_ppm exceeds 1000000"));
    }
    for value in &agent.source_entity_ids {
        validate_text("source_entity_id", value)?;
    }
    for value in &agent.tool_calls {
        validate_text("tool_call", value)?;
    }
    agent.policy_labels = canonical_labels(agent.policy_labels)?;
    Ok(agent)
}

pub(crate) fn string_array(values: &[String]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|value| Value::Text(value.clone()))
            .collect(),
    )
}

fn optional_text_value(value: Option<&str>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Text(value.to_string())]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_bytes(value: Option<&[u8]>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Bytes(value.to_vec())]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_u32(value: Option<u32>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Uint(u64::from(value))]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_id(value: Option<WorkspaceId>) -> Value {
    optional_text_value(value.map(|id| id.to_string()).as_deref())
}

fn optional_digest(value: Option<Digest>) -> Value {
    optional_text_value(value.map(|digest| digest.to_string()).as_deref())
}

fn optional_agent(value: Option<&AgentIdentity>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), value.to_value()]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

pub struct Fields {
    items: std::vec::IntoIter<Value>,
}

impl Fields {
    pub fn array(value: Value, name: &str) -> Result<Self> {
        match value {
            Value::Array(items) => Ok(Self {
                items: items.into_iter(),
            }),
            _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
        }
    }

    pub fn next(&mut self, name: &str) -> Result<Value> {
        self.items
            .next()
            .ok_or_else(|| LoomError::corrupt(format!("{name} is missing")))
    }

    pub fn expect_text(&mut self, expected: &str) -> Result<()> {
        match self.next("schema")? {
            Value::Text(actual) if actual == expected => Ok(()),
            _ => Err(LoomError::corrupt("unexpected schema")),
        }
    }

    pub fn text(&mut self, name: &str) -> Result<String> {
        match self.next(name)? {
            Value::Text(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be text"))),
        }
    }

    pub fn bytes(&mut self, name: &str) -> Result<Vec<u8>> {
        match self.next(name)? {
            Value::Bytes(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be bytes"))),
        }
    }

    pub fn uint(&mut self, name: &str) -> Result<u64> {
        match self.next(name)? {
            Value::Uint(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be uint"))),
        }
    }

    fn bool(&mut self, name: &str) -> Result<bool> {
        match self.next(name)? {
            Value::Bool(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be bool"))),
        }
    }

    fn id(&mut self, name: &str) -> Result<WorkspaceId> {
        WorkspaceId::parse(&self.text(name)?)
    }

    pub fn digest(&mut self, name: &str) -> Result<Digest> {
        Digest::parse(&self.text(name)?)
    }

    fn string_array(&mut self, name: &str) -> Result<Vec<String>> {
        match self.next(name)? {
            Value::Array(items) => items
                .into_iter()
                .map(|item| match item {
                    Value::Text(value) => Ok(value),
                    _ => Err(LoomError::corrupt(format!("{name} item must be text"))),
                })
                .collect(),
            _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
        }
    }

    fn optional_text(&mut self, name: &str) -> Result<Option<String>> {
        match optional_value(self.next(name)?, name)? {
            Some(Value::Text(value)) => Ok(Some(value)),
            Some(_) => Err(LoomError::corrupt(format!("{name} value must be text"))),
            None => Ok(None),
        }
    }

    fn optional_bytes(&mut self, name: &str) -> Result<Option<Vec<u8>>> {
        match optional_value(self.next(name)?, name)? {
            Some(Value::Bytes(value)) => Ok(Some(value)),
            Some(_) => Err(LoomError::corrupt(format!("{name} value must be bytes"))),
            None => Ok(None),
        }
    }

    fn optional_u32(&mut self, name: &str) -> Result<Option<u32>> {
        match optional_value(self.next(name)?, name)? {
            Some(Value::Uint(value)) => u32::try_from(value)
                .map(Some)
                .map_err(|_| LoomError::corrupt(format!("{name} value is too large"))),
            Some(_) => Err(LoomError::corrupt(format!("{name} value must be uint"))),
            None => Ok(None),
        }
    }

    fn optional_id(&mut self, name: &str) -> Result<Option<WorkspaceId>> {
        self.optional_text(name)?
            .map(|value| WorkspaceId::parse(&value))
            .transpose()
    }

    fn optional_digest(&mut self, name: &str) -> Result<Option<Digest>> {
        self.optional_text(name)?
            .map(|value| Digest::parse(&value))
            .transpose()
    }

    fn optional_agent(&mut self, name: &str) -> Result<Option<AgentIdentity>> {
        optional_value(self.next(name)?, name)?
            .map(AgentIdentity::from_value)
            .transpose()
    }

    pub fn end(mut self, name: &str) -> Result<()> {
        if self.items.next().is_some() {
            return Err(LoomError::corrupt(format!("{name} has trailing fields")));
        }
        Ok(())
    }
}

fn optional_value(value: Value, name: &str) -> Result<Option<Value>> {
    let mut fields = Fields::array(value, name)?;
    let tag = fields.uint(name)?;
    let value = match tag {
        0 => None,
        1 => Some(fields.next(name)?),
        other => {
            return Err(LoomError::corrupt(format!(
                "{name} has unknown optional tag {other}"
            )));
        }
    };
    fields.end(name)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([byte; 16])
    }

    #[test]
    fn operation_envelope_round_trips_canonical_bytes() {
        let mut agent = AgentIdentity::new("agent-1", "model/runtime").unwrap();
        agent.operator_principal = Some(id(2));
        agent.source_entity_ids = vec!["ticket:one".to_string()];
        agent.tool_calls = vec!["tickets.comment".to_string()];
        agent.confidence_ppm = Some(875_000);
        agent.policy_labels = vec!["write".to_string(), "write".to_string(), "ai".to_string()];
        agent.trace_digest = Some(Digest::hash(Algo::Blake3, b"trace"));
        let envelope = OperationEnvelope::new(
            Algo::Blake3,
            OperationEnvelopeInput {
                workspace_id: "organization",
                app_id: "tickets",
                scope_id: "project",
                operation_id: "op-1",
                operation_kind: "ticket.created",
                sequence: 42,
                actor_principal: id(1),
                actor_kind: ActorKind::Agent,
                timestamp_ms: 123,
                idempotency_key: "idem-1",
                base_root: Digest::hash(Algo::Blake3, b"root"),
                base_entity_version: Some("v1"),
                target_entity_id: Some("LOOM-1"),
                payload: b"payload",
                policy_labels: &["beta", "alpha", "alpha"],
                signature: Some(b"sig"),
                agent: Some(agent),
            },
        )
        .unwrap();
        assert_eq!(envelope.policy_labels, ["alpha", "beta"]);
        assert_eq!(
            envelope.agent.as_ref().unwrap().policy_labels,
            ["ai", "write"]
        );
        envelope.validate_payload_digest(Algo::Blake3).unwrap();
        let encoded = envelope.encode().unwrap();
        let decoded = OperationEnvelope::decode(&encoded).unwrap();
        assert_eq!(decoded, envelope);
        assert_eq!(decoded.encode().unwrap(), encoded);
        assert_eq!(
            decoded.debug_json()["payload_digest"],
            json!(Digest::hash(Algo::Blake3, b"payload").to_string())
        );
    }

    #[test]
    fn operation_envelope_rejects_payload_digest_mismatch() {
        let mut envelope = OperationEnvelope::new(
            Algo::Blake3,
            OperationEnvelopeInput {
                workspace_id: "organization",
                app_id: "tickets",
                scope_id: "project",
                operation_id: "op-1",
                operation_kind: "ticket.created",
                sequence: 42,
                actor_principal: id(1),
                actor_kind: ActorKind::User,
                timestamp_ms: 123,
                idempotency_key: "idem-1",
                base_root: Digest::hash(Algo::Blake3, b"root"),
                base_entity_version: None,
                target_entity_id: None,
                payload: b"payload",
                policy_labels: &[],
                signature: None,
                agent: None,
            },
        )
        .unwrap();
        envelope.payload = b"other".to_vec();
        assert_eq!(
            envelope
                .validate_payload_digest(Algo::Blake3)
                .unwrap_err()
                .code,
            loom_types::Code::IntegrityFailure
        );
    }

    #[test]
    fn operation_envelope_rejects_wrong_schema() {
        let bytes = loom_codec::encode(&Value::Array(vec![
            Value::Text("other".to_string()),
            Value::Array(Vec::new()),
        ]))
        .unwrap();
        assert_eq!(
            OperationEnvelope::decode(&bytes).unwrap_err().code,
            loom_types::Code::CorruptObject
        );
    }
}
