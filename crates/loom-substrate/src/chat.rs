use std::collections::{BTreeMap, BTreeSet};

use loom_codec::Value;
use loom_types::{Code, Digest, LoomError, Result, WorkspaceId};

use crate::annotation::{
    AnnotationAction, AnnotationAnchor, AnnotationEvent, AnnotationStore, ReactionKey,
};
use crate::changes::{OperationChangeBatch, OperationChangeCursor, OperationChangeRecord};
use crate::{Fields, OperationEnvelope, codec_error, validate_text};

pub const APP_ID: &str = "chat";
pub const CHAT_OPERATION_PAYLOAD_SCHEMA: &str = "loom.studio.chat.operation-payload.v1";
pub const CHANNEL_OPERATION_LOG_SCHEMA: &str = "loom.studio.chat.channel-operation-log.v1";
pub const CHANNEL_DIRECTORY_SCHEMA: &str = "loom.studio.chat.channel-directory.v1";
pub const PROFILE_CONTROL_PREFIX: &str = "profile/chat/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatChannel {
    pub id: WorkspaceId,
    pub handle: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatChannelDirectory {
    pub workspace_id: String,
    channels: BTreeMap<WorkspaceId, ChatChannel>,
    handles: BTreeMap<String, WorkspaceId>,
}

impl ChatChannelDirectory {
    pub fn new(workspace_id: impl Into<String>) -> Result<Self> {
        let workspace_id = workspace_id.into();
        validate_text("workspace_id", &workspace_id)?;
        Ok(Self {
            workspace_id,
            channels: BTreeMap::new(),
            handles: BTreeMap::new(),
        })
    }

    pub fn create_channel(
        &mut self,
        id: WorkspaceId,
        handle: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<ChatChannel> {
        if self.channels.contains_key(&id) {
            return Err(LoomError::new(Code::AlreadyExists, "chat channel exists"));
        }
        let handle = normalize_channel_handle(&handle.into())?;
        if self.handles.contains_key(&handle) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "chat channel handle is reserved",
            ));
        }
        let name = name.into();
        validate_text("chat channel name", &name)?;
        let channel = ChatChannel { id, handle, name };
        self.handles.insert(channel.handle.clone(), id);
        self.channels.insert(id, channel.clone());
        Ok(channel)
    }

    pub fn resolve(&self, selector: &str) -> Result<Option<&ChatChannel>> {
        if let Ok(id) = WorkspaceId::parse(selector) {
            return Ok(self.channels.get(&id));
        }
        let handle = normalize_channel_handle(selector)?;
        Ok(self
            .handles
            .get(&handle)
            .and_then(|id| self.channels.get(id)))
    }

    pub fn rename_channel(&mut self, id: WorkspaceId, handle: impl Into<String>) -> Result<()> {
        let handle = normalize_channel_handle(&handle.into())?;
        if let Some(existing) = self.handles.get(&handle)
            && *existing != id
        {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "chat channel handle is reserved",
            ));
        }
        let channel = self
            .channels
            .get_mut(&id)
            .ok_or_else(|| LoomError::new(Code::NotFound, "chat channel not found"))?;
        channel.handle = handle.clone();
        self.handles.insert(handle, id);
        Ok(())
    }

    pub fn channels(&self) -> impl Iterator<Item = &ChatChannel> {
        self.channels.values()
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(CHANNEL_DIRECTORY_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Array(
                    self.channels
                        .values()
                        .map(|channel| {
                            Value::Array(vec![
                                Value::Bytes(channel.id.as_bytes().to_vec()),
                                Value::Text(channel.handle.clone()),
                                Value::Text(channel.name.clone()),
                            ])
                        })
                        .collect(),
                ),
                Value::Array(
                    self.handles
                        .iter()
                        .map(|(handle, id)| {
                            Value::Array(vec![
                                Value::Text(handle.clone()),
                                Value::Bytes(id.as_bytes().to_vec()),
                            ])
                        })
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "chat channel directory")?;
        outer.expect_text(CHANNEL_DIRECTORY_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("chat channel directory fields")?,
            "chat channel directory",
        )?;
        outer.end("chat channel directory")?;
        let workspace_id = fields.text("workspace_id")?;
        let entries = match fields.next("channels")? {
            Value::Array(entries) => entries,
            _ => return Err(LoomError::corrupt("chat channels must be an array")),
        };
        let aliases = match fields.next("handles")? {
            Value::Array(aliases) => aliases,
            _ => return Err(LoomError::corrupt("chat channel handles must be an array")),
        };
        fields.end("chat channel directory")?;
        let mut directory = Self::new(workspace_id)?;
        for entry in entries {
            let mut entry = Fields::array(entry, "chat channel")?;
            let id = WorkspaceId::from_bytes(
                entry
                    .bytes("channel id")?
                    .as_slice()
                    .try_into()
                    .map_err(|_| LoomError::corrupt("chat channel id is not 16 bytes"))?,
            );
            let handle = entry.text("channel handle")?;
            let name = entry.text("channel name")?;
            entry.end("chat channel")?;
            directory.create_channel(id, handle, name)?;
        }
        for alias in aliases {
            let mut alias = Fields::array(alias, "chat channel handle")?;
            let handle = normalize_channel_handle(&alias.text("channel handle")?)?;
            let id = WorkspaceId::from_bytes(
                alias
                    .bytes("channel id")?
                    .as_slice()
                    .try_into()
                    .map_err(|_| LoomError::corrupt("chat channel id is not 16 bytes"))?,
            );
            alias.end("chat channel handle")?;
            if !directory.channels.contains_key(&id) {
                return Err(LoomError::corrupt("invalid chat channel handle registry"));
            }
            if let Some(existing) = directory.handles.insert(handle, id)
                && existing != id
            {
                return Err(LoomError::corrupt("invalid chat channel handle registry"));
            }
        }
        for channel in directory.channels.values() {
            if directory.handles.get(&channel.handle) != Some(&channel.id) {
                return Err(LoomError::corrupt(
                    "chat channel is missing canonical handle",
                ));
            }
        }
        Ok(directory)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatOperationPayload {
    MessageCreated {
        message_id: String,
        thread_id: Option<String>,
        body: Vec<u8>,
    },
    MessageEdited {
        message_id: String,
        body: Vec<u8>,
    },
    MessageRedacted {
        message_id: String,
        reason: Option<String>,
    },
    ThreadCreated {
        thread_id: String,
        parent_message_id: String,
    },
    ReactionAdded {
        message_id: String,
        kind: String,
    },
    ReactionRemoved {
        message_id: String,
        kind: String,
    },
    TaskCreated {
        task_id: String,
        message_id: Option<String>,
        title: String,
    },
    TaskClaimed {
        task_id: String,
        claim_id: String,
        claimant_principal: WorkspaceId,
        lease_token: Option<String>,
    },
    TaskCompleted {
        task_id: String,
        claim_id: String,
        result_message_id: Option<String>,
    },
    AgentInvoked {
        invocation_id: String,
        agent_principal: WorkspaceId,
        source_message_ids: Vec<String>,
        prompt: Vec<u8>,
    },
    AgentReplied {
        invocation_id: String,
        message_id: String,
    },
    HandoffRequested {
        handoff_id: String,
        from_agent_principal: WorkspaceId,
        to_principal: Option<WorkspaceId>,
        reason: Option<String>,
    },
}

impl ChatOperationPayload {
    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn operation_kind(&self) -> &'static str {
        match self {
            Self::MessageCreated { .. } => "message.created",
            Self::MessageEdited { .. } => "message.edited",
            Self::MessageRedacted { .. } => "message.redacted",
            Self::ThreadCreated { .. } => "thread.created",
            Self::ReactionAdded { .. } => "reaction.added",
            Self::ReactionRemoved { .. } => "reaction.removed",
            Self::TaskCreated { .. } => "task.created",
            Self::TaskClaimed { .. } => "task.claimed",
            Self::TaskCompleted { .. } => "task.completed",
            Self::AgentInvoked { .. } => "agent.invoked",
            Self::AgentReplied { .. } => "agent.replied",
            Self::HandoffRequested { .. } => "handoff.requested",
        }
    }

    pub fn target_entity_id(&self) -> &str {
        match self {
            Self::MessageCreated { message_id, .. }
            | Self::MessageEdited { message_id, .. }
            | Self::MessageRedacted { message_id, .. }
            | Self::ReactionAdded { message_id, .. }
            | Self::ReactionRemoved { message_id, .. } => message_id,
            Self::ThreadCreated { thread_id, .. } => thread_id,
            Self::TaskCreated { task_id, .. }
            | Self::TaskClaimed { task_id, .. }
            | Self::TaskCompleted { task_id, .. } => task_id,
            Self::AgentInvoked { invocation_id, .. } | Self::AgentReplied { invocation_id, .. } => {
                invocation_id
            }
            Self::HandoffRequested { handoff_id, .. } => handoff_id,
        }
    }

    fn to_value(&self) -> Value {
        let fields = match self {
            Self::MessageCreated {
                message_id,
                thread_id,
                body,
            } => vec![
                Value::Uint(0),
                Value::Text(message_id.clone()),
                optional_text_value(thread_id.as_deref()),
                Value::Bytes(body.clone()),
            ],
            Self::MessageEdited { message_id, body } => vec![
                Value::Uint(1),
                Value::Text(message_id.clone()),
                Value::Bytes(body.clone()),
            ],
            Self::MessageRedacted { message_id, reason } => vec![
                Value::Uint(2),
                Value::Text(message_id.clone()),
                optional_text_value(reason.as_deref()),
            ],
            Self::ThreadCreated {
                thread_id,
                parent_message_id,
            } => vec![
                Value::Uint(3),
                Value::Text(thread_id.clone()),
                Value::Text(parent_message_id.clone()),
            ],
            Self::ReactionAdded { message_id, kind } => vec![
                Value::Uint(4),
                Value::Text(message_id.clone()),
                Value::Text(kind.clone()),
            ],
            Self::ReactionRemoved { message_id, kind } => vec![
                Value::Uint(5),
                Value::Text(message_id.clone()),
                Value::Text(kind.clone()),
            ],
            Self::TaskCreated {
                task_id,
                message_id,
                title,
            } => vec![
                Value::Uint(6),
                Value::Text(task_id.clone()),
                optional_text_value(message_id.as_deref()),
                Value::Text(title.clone()),
            ],
            Self::TaskClaimed {
                task_id,
                claim_id,
                claimant_principal,
                lease_token,
            } => vec![
                Value::Uint(7),
                Value::Text(task_id.clone()),
                Value::Text(claim_id.clone()),
                Value::Text(claimant_principal.to_string()),
                optional_text_value(lease_token.as_deref()),
            ],
            Self::TaskCompleted {
                task_id,
                claim_id,
                result_message_id,
            } => vec![
                Value::Uint(8),
                Value::Text(task_id.clone()),
                Value::Text(claim_id.clone()),
                optional_text_value(result_message_id.as_deref()),
            ],
            Self::AgentInvoked {
                invocation_id,
                agent_principal,
                source_message_ids,
                prompt,
            } => vec![
                Value::Uint(9),
                Value::Text(invocation_id.clone()),
                Value::Text(agent_principal.to_string()),
                string_list_value(source_message_ids),
                Value::Bytes(prompt.clone()),
            ],
            Self::AgentReplied {
                invocation_id,
                message_id,
            } => vec![
                Value::Uint(10),
                Value::Text(invocation_id.clone()),
                Value::Text(message_id.clone()),
            ],
            Self::HandoffRequested {
                handoff_id,
                from_agent_principal,
                to_principal,
                reason,
            } => vec![
                Value::Uint(11),
                Value::Text(handoff_id.clone()),
                Value::Text(from_agent_principal.to_string()),
                optional_id_value(*to_principal),
                optional_text_value(reason.as_deref()),
            ],
        };
        Value::Array(vec![
            Value::Text(CHAT_OPERATION_PAYLOAD_SCHEMA.to_string()),
            Value::Array(fields),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "chat operation payload")?;
        outer.expect_text(CHAT_OPERATION_PAYLOAD_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("chat operation payload fields")?,
            "chat operation payload",
        )?;
        outer.end("chat operation payload")?;
        let payload = match fields.uint("operation tag")? {
            0 => Self::MessageCreated {
                message_id: fields.text("message_id")?,
                thread_id: fields.optional_text("thread_id")?,
                body: fields.bytes("body")?,
            },
            1 => Self::MessageEdited {
                message_id: fields.text("message_id")?,
                body: fields.bytes("body")?,
            },
            2 => Self::MessageRedacted {
                message_id: fields.text("message_id")?,
                reason: fields.optional_text("reason")?,
            },
            3 => Self::ThreadCreated {
                thread_id: fields.text("thread_id")?,
                parent_message_id: fields.text("parent_message_id")?,
            },
            4 => Self::ReactionAdded {
                message_id: fields.text("message_id")?,
                kind: fields.text("reaction kind")?,
            },
            5 => Self::ReactionRemoved {
                message_id: fields.text("message_id")?,
                kind: fields.text("reaction kind")?,
            },
            6 => Self::TaskCreated {
                task_id: fields.text("task_id")?,
                message_id: fields.optional_text("message_id")?,
                title: fields.text("title")?,
            },
            7 => Self::TaskClaimed {
                task_id: fields.text("task_id")?,
                claim_id: fields.text("claim_id")?,
                claimant_principal: fields.id("claimant_principal")?,
                lease_token: fields.optional_text("lease_token")?,
            },
            8 => Self::TaskCompleted {
                task_id: fields.text("task_id")?,
                claim_id: fields.text("claim_id")?,
                result_message_id: fields.optional_text("result_message_id")?,
            },
            9 => Self::AgentInvoked {
                invocation_id: fields.text("invocation_id")?,
                agent_principal: fields.id("agent_principal")?,
                source_message_ids: fields.string_array("source_message_ids")?,
                prompt: fields.bytes("prompt")?,
            },
            10 => Self::AgentReplied {
                invocation_id: fields.text("invocation_id")?,
                message_id: fields.text("message_id")?,
            },
            11 => Self::HandoffRequested {
                handoff_id: fields.text("handoff_id")?,
                from_agent_principal: fields.id("from_agent_principal")?,
                to_principal: fields.optional_id("to_principal")?,
                reason: fields.optional_text("reason")?,
            },
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown chat operation payload tag {other}"
                )));
            }
        };
        fields.end("chat operation payload")?;
        payload.validate()?;
        Ok(payload)
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::MessageCreated {
                message_id,
                thread_id,
                body,
            } => {
                validate_text("message_id", message_id)?;
                if let Some(thread_id) = thread_id {
                    validate_text("thread_id", thread_id)?;
                }
                validate_body(body)
            }
            Self::MessageEdited { message_id, body } => {
                validate_text("message_id", message_id)?;
                validate_body(body)
            }
            Self::MessageRedacted { message_id, reason } => {
                validate_text("message_id", message_id)?;
                if let Some(reason) = reason {
                    validate_text("redaction reason", reason)?;
                }
                Ok(())
            }
            Self::ThreadCreated {
                thread_id,
                parent_message_id,
            } => {
                validate_text("thread_id", thread_id)?;
                validate_text("parent_message_id", parent_message_id)
            }
            Self::ReactionAdded { message_id, kind }
            | Self::ReactionRemoved { message_id, kind } => {
                validate_text("message_id", message_id)?;
                validate_text("reaction kind", kind)
            }
            Self::TaskCreated {
                task_id,
                message_id,
                title,
            } => {
                validate_text("task_id", task_id)?;
                if let Some(message_id) = message_id {
                    validate_text("message_id", message_id)?;
                }
                validate_text("task title", title)
            }
            Self::TaskClaimed {
                task_id,
                claim_id,
                claimant_principal: _,
                lease_token,
            } => {
                validate_text("task_id", task_id)?;
                validate_text("claim_id", claim_id)?;
                if let Some(lease_token) = lease_token {
                    validate_text("lease_token", lease_token)?;
                }
                Ok(())
            }
            Self::TaskCompleted {
                task_id,
                claim_id,
                result_message_id,
            } => {
                validate_text("task_id", task_id)?;
                validate_text("claim_id", claim_id)?;
                if let Some(result_message_id) = result_message_id {
                    validate_text("result_message_id", result_message_id)?;
                }
                Ok(())
            }
            Self::AgentInvoked {
                invocation_id,
                agent_principal: _,
                source_message_ids,
                prompt,
            } => {
                validate_text("invocation_id", invocation_id)?;
                for message_id in source_message_ids {
                    validate_text("source_message_id", message_id)?;
                }
                validate_body(prompt)
            }
            Self::AgentReplied {
                invocation_id,
                message_id,
            } => {
                validate_text("invocation_id", invocation_id)?;
                validate_text("message_id", message_id)
            }
            Self::HandoffRequested {
                handoff_id,
                from_agent_principal: _,
                to_principal: _,
                reason,
            } => {
                validate_text("handoff_id", handoff_id)?;
                if let Some(reason) = reason {
                    validate_text("handoff reason", reason)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatOperationRecord {
    pub sequence: u64,
    pub operation_id: String,
    pub operation_kind: String,
    pub target_entity_id: Option<String>,
    pub root_after: Digest,
    pub envelope: Vec<u8>,
}

impl ChatOperationRecord {
    pub fn new(
        sequence: u64,
        operation_id: impl Into<String>,
        operation_kind: impl Into<String>,
        target_entity_id: Option<String>,
        root_after: Digest,
        envelope: Vec<u8>,
    ) -> Result<Self> {
        let record = Self {
            sequence,
            operation_id: operation_id.into(),
            operation_kind: operation_kind.into(),
            target_entity_id,
            root_after,
            envelope,
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> Result<()> {
        validate_text("chat operation_id", &self.operation_id)?;
        validate_text("chat operation_kind", &self.operation_kind)?;
        if let Some(target) = &self.target_entity_id {
            validate_text("chat operation target", target)?;
        }
        if self.envelope.is_empty() {
            return Err(LoomError::invalid(
                "chat operation envelope must not be empty",
            ));
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Uint(self.sequence),
            Value::Text(self.operation_id.clone()),
            Value::Text(self.operation_kind.clone()),
            optional_text_value(self.target_entity_id.as_deref()),
            Value::Text(self.root_after.to_string()),
            Value::Bytes(self.envelope.clone()),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut fields = Fields::array(value, "chat operation record")?;
        let sequence = fields.uint("sequence")?;
        let operation_id = fields.text("operation_id")?;
        let operation_kind = fields.text("operation_kind")?;
        let target_entity_id = fields.optional_text("target_entity_id")?;
        let root_after = fields.digest("root_after")?;
        let envelope = fields.bytes("envelope")?;
        fields.end("chat operation record")?;
        Self::new(
            sequence,
            operation_id,
            operation_kind,
            target_entity_id,
            root_after,
            envelope,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelOperationLog {
    pub workspace_id: String,
    pub channel_id: String,
    pub records: Vec<ChatOperationRecord>,
}

impl ChannelOperationLog {
    pub fn new(
        workspace_id: impl Into<String>,
        channel_id: impl Into<String>,
        records: Vec<ChatOperationRecord>,
    ) -> Result<Self> {
        let log = Self {
            workspace_id: workspace_id.into(),
            channel_id: channel_id.into(),
            records,
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

    pub fn project(&self) -> Result<ChannelProjection> {
        let mut projection = ChannelProjection::new(&self.workspace_id, &self.channel_id)?;
        for record in &self.records {
            let envelope = OperationEnvelope::decode(&record.envelope)?;
            if envelope.operation_kind != record.operation_kind {
                return Err(LoomError::corrupt(
                    "chat operation kind does not match envelope",
                ));
            }
            let payload = ChatOperationPayload::decode(&envelope.payload)?;
            if payload.operation_kind() != record.operation_kind {
                return Err(LoomError::corrupt(
                    "chat payload kind does not match operation",
                ));
            }
            projection.apply(record, &envelope, payload)?;
        }
        Ok(projection)
    }

    pub fn changes(
        &self,
        cursor: &OperationChangeCursor,
        max: usize,
    ) -> Result<OperationChangeBatch> {
        let expected_scope = chat_operation_cursor_scope(&self.workspace_id, &self.channel_id);
        if cursor.scope_id != expected_scope {
            return Err(LoomError::invalid(
                "operation change cursor scope does not match chat channel log",
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

    fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(CHANNEL_OPERATION_LOG_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.workspace_id.clone()),
                Value::Text(self.channel_id.clone()),
                Value::Array(
                    self.records
                        .iter()
                        .map(ChatOperationRecord::to_value)
                        .collect(),
                ),
            ]),
        ])
    }

    fn from_value(value: Value) -> Result<Self> {
        let mut outer = Fields::array(value, "chat channel operation log")?;
        outer.expect_text(CHANNEL_OPERATION_LOG_SCHEMA)?;
        let mut fields = Fields::array(
            outer.next("chat channel operation log fields")?,
            "chat channel operation log",
        )?;
        outer.end("chat channel operation log")?;
        let workspace_id = fields.text("workspace_id")?;
        let channel_id = fields.text("channel_id")?;
        let records = chat_record_list(fields.next("records")?)?;
        fields.end("chat channel operation log")?;
        Self::new(workspace_id, channel_id, records)
    }

    fn validate(&self) -> Result<()> {
        validate_text("workspace_id", &self.workspace_id)?;
        validate_text("channel_id", &self.channel_id)?;
        let mut previous = None;
        let mut ids = BTreeSet::new();
        for record in &self.records {
            record.validate()?;
            if !ids.insert(record.operation_id.clone()) {
                return Err(LoomError::invalid("chat operation ids must be unique"));
            }
            if let Some(previous) = previous
                && record.sequence <= previous
            {
                return Err(LoomError::invalid(
                    "chat operation records must be ordered by increasing sequence",
                ));
            }
            previous = Some(record.sequence);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ChannelProjection {
    pub workspace_id: String,
    pub channel_id: String,
    messages: BTreeMap<String, ChatMessage>,
    threads: BTreeMap<String, ChatThread>,
    tasks: BTreeMap<String, ChatTask>,
    agent_invocations: BTreeMap<String, ChatAgentInvocation>,
    handoffs: BTreeMap<String, ChatHandoffRequest>,
    annotations: AnnotationStore,
}

impl ChannelProjection {
    pub fn new(workspace_id: impl Into<String>, channel_id: impl Into<String>) -> Result<Self> {
        let workspace_id = workspace_id.into();
        let channel_id = channel_id.into();
        validate_text("workspace_id", &workspace_id)?;
        validate_text("channel_id", &channel_id)?;
        Ok(Self {
            workspace_id,
            channel_id,
            messages: BTreeMap::new(),
            threads: BTreeMap::new(),
            tasks: BTreeMap::new(),
            agent_invocations: BTreeMap::new(),
            handoffs: BTreeMap::new(),
            annotations: AnnotationStore::new(),
        })
    }

    pub fn messages(&self) -> Vec<ChatMessageView> {
        self.messages
            .values()
            .map(|message| {
                let annotation = self.annotations.get(&message.annotation_id);
                ChatMessageView {
                    message_id: message.message_id.clone(),
                    thread_id: message.thread_id.clone(),
                    body: message.body.clone(),
                    author_principal: message.author_principal,
                    created_at_ms: message.created_at_ms,
                    updated_at_ms: message.updated_at_ms,
                    redacted: message.redacted,
                    reactions: annotation
                        .map(|record| reaction_summaries(&record.reactions))
                        .unwrap_or_default(),
                }
            })
            .collect()
    }

    pub fn threads(&self) -> Vec<ChatThread> {
        self.threads.values().cloned().collect()
    }

    pub fn tasks(&self) -> Vec<ChatTask> {
        self.tasks.values().cloned().collect()
    }

    pub fn agent_invocations(&self) -> Vec<ChatAgentInvocation> {
        self.agent_invocations.values().cloned().collect()
    }

    pub fn handoffs(&self) -> Vec<ChatHandoffRequest> {
        self.handoffs.values().cloned().collect()
    }

    fn apply(
        &mut self,
        record: &ChatOperationRecord,
        envelope: &OperationEnvelope,
        payload: ChatOperationPayload,
    ) -> Result<()> {
        match payload {
            ChatOperationPayload::MessageCreated {
                message_id,
                thread_id,
                body,
            } => self.apply_message_created(record, envelope, message_id, thread_id, body),
            ChatOperationPayload::MessageEdited { message_id, body } => {
                let message = self.message_mut(&message_id)?;
                message.body = body;
                message.updated_at_ms = envelope.timestamp_ms;
                Ok(())
            }
            ChatOperationPayload::MessageRedacted { message_id, reason } => {
                let annotation_id = chat_message_annotation_id(&message_id);
                self.annotations.apply(AnnotationEvent::new(
                    record.operation_id.clone(),
                    annotation_id,
                    envelope.actor_principal,
                    envelope.timestamp_ms,
                    AnnotationAction::Redact { reason },
                )?)?;
                let message = self.message_mut(&message_id)?;
                message.body.clear();
                message.redacted = true;
                message.updated_at_ms = envelope.timestamp_ms;
                Ok(())
            }
            ChatOperationPayload::ThreadCreated {
                thread_id,
                parent_message_id,
            } => {
                if self.threads.contains_key(&thread_id) {
                    return Err(LoomError::new(Code::AlreadyExists, "chat thread exists"));
                }
                if !self.messages.contains_key(&parent_message_id) {
                    return Err(LoomError::not_found("parent message not found"));
                }
                self.threads.insert(
                    thread_id.clone(),
                    ChatThread {
                        thread_id,
                        parent_message_id,
                        created_at_ms: envelope.timestamp_ms,
                    },
                );
                Ok(())
            }
            ChatOperationPayload::ReactionAdded { message_id, kind } => {
                self.apply_reaction(record, envelope, message_id, kind, true)
            }
            ChatOperationPayload::ReactionRemoved { message_id, kind } => {
                self.apply_reaction(record, envelope, message_id, kind, false)
            }
            ChatOperationPayload::TaskCreated {
                task_id,
                message_id,
                title,
            } => self.apply_task_created(envelope, task_id, message_id, title),
            ChatOperationPayload::TaskClaimed {
                task_id,
                claim_id,
                claimant_principal,
                lease_token,
            } => self.apply_task_claimed(
                envelope,
                task_id,
                claim_id,
                claimant_principal,
                lease_token,
            ),
            ChatOperationPayload::TaskCompleted {
                task_id,
                claim_id,
                result_message_id,
            } => self.apply_task_completed(envelope, task_id, claim_id, result_message_id),
            ChatOperationPayload::AgentInvoked {
                invocation_id,
                agent_principal,
                source_message_ids,
                prompt,
            } => self.apply_agent_invoked(
                envelope,
                invocation_id,
                agent_principal,
                source_message_ids,
                prompt,
            ),
            ChatOperationPayload::AgentReplied {
                invocation_id,
                message_id,
            } => self.apply_agent_replied(invocation_id, message_id),
            ChatOperationPayload::HandoffRequested {
                handoff_id,
                from_agent_principal,
                to_principal,
                reason,
            } => self.apply_handoff_requested(
                envelope,
                handoff_id,
                from_agent_principal,
                to_principal,
                reason,
            ),
        }
    }

    fn apply_message_created(
        &mut self,
        record: &ChatOperationRecord,
        envelope: &OperationEnvelope,
        message_id: String,
        thread_id: Option<String>,
        body: Vec<u8>,
    ) -> Result<()> {
        if self.messages.contains_key(&message_id) {
            return Err(LoomError::new(Code::AlreadyExists, "chat message exists"));
        }
        if let Some(thread_id) = &thread_id
            && !self.threads.contains_key(thread_id)
        {
            return Err(LoomError::not_found("chat thread not found"));
        }
        let annotation_id = chat_message_annotation_id(&message_id);
        self.annotations.apply(AnnotationEvent::new(
            record.operation_id.clone(),
            annotation_id.clone(),
            envelope.actor_principal,
            envelope.timestamp_ms,
            AnnotationAction::Add {
                anchor: AnnotationAnchor::Entity {
                    entity_id: message_id.clone(),
                },
                body: "chat message".to_string(),
            },
        )?)?;
        self.messages.insert(
            message_id.clone(),
            ChatMessage {
                message_id,
                thread_id,
                body,
                annotation_id,
                author_principal: envelope.actor_principal,
                created_at_ms: envelope.timestamp_ms,
                updated_at_ms: envelope.timestamp_ms,
                redacted: false,
            },
        );
        Ok(())
    }

    fn apply_reaction(
        &mut self,
        record: &ChatOperationRecord,
        envelope: &OperationEnvelope,
        message_id: String,
        kind: String,
        add: bool,
    ) -> Result<()> {
        if !self.messages.contains_key(&message_id) {
            return Err(LoomError::not_found("chat message not found"));
        }
        let annotation_id = chat_message_annotation_id(&message_id);
        let action = if add {
            AnnotationAction::ReactionAdd { kind }
        } else {
            AnnotationAction::ReactionRemove { kind }
        };
        self.annotations.apply(AnnotationEvent::new(
            record.operation_id.clone(),
            annotation_id,
            envelope.actor_principal,
            envelope.timestamp_ms,
            action,
        )?)
    }

    fn message_mut(&mut self, message_id: &str) -> Result<&mut ChatMessage> {
        self.messages
            .get_mut(message_id)
            .ok_or_else(|| LoomError::not_found("chat message not found"))
    }

    fn apply_task_created(
        &mut self,
        envelope: &OperationEnvelope,
        task_id: String,
        message_id: Option<String>,
        title: String,
    ) -> Result<()> {
        if self.tasks.contains_key(&task_id) {
            return Err(LoomError::new(Code::AlreadyExists, "chat task exists"));
        }
        if let Some(message_id) = &message_id
            && !self.messages.contains_key(message_id)
        {
            return Err(LoomError::not_found("chat task message not found"));
        }
        self.tasks.insert(
            task_id.clone(),
            ChatTask {
                task_id,
                message_id,
                title,
                created_by: envelope.actor_principal,
                created_at_ms: envelope.timestamp_ms,
                state: ChatTaskState::Open,
            },
        );
        Ok(())
    }

    fn apply_task_claimed(
        &mut self,
        envelope: &OperationEnvelope,
        task_id: String,
        claim_id: String,
        claimant_principal: WorkspaceId,
        lease_token: Option<String>,
    ) -> Result<()> {
        let task = self.task_mut(&task_id)?;
        match task.state {
            ChatTaskState::Open => {
                task.state = ChatTaskState::Claimed {
                    claim_id,
                    claimant_principal,
                    claimed_by: envelope.actor_principal,
                    claimed_at_ms: envelope.timestamp_ms,
                    lease_token,
                };
                Ok(())
            }
            ChatTaskState::Claimed { .. } | ChatTaskState::Completed { .. } => {
                Err(LoomError::new(Code::Conflict, "chat task is not open"))
            }
        }
    }

    fn apply_task_completed(
        &mut self,
        envelope: &OperationEnvelope,
        task_id: String,
        claim_id: String,
        result_message_id: Option<String>,
    ) -> Result<()> {
        if let Some(message_id) = &result_message_id
            && !self.messages.contains_key(message_id)
        {
            return Err(LoomError::not_found("chat task result message not found"));
        }
        let task = self.task_mut(&task_id)?;
        match &task.state {
            ChatTaskState::Claimed {
                claim_id: active_claim,
                claimant_principal,
                ..
            } if active_claim == &claim_id => {
                task.state = ChatTaskState::Completed {
                    claim_id,
                    completed_by: envelope.actor_principal,
                    completed_principal: *claimant_principal,
                    completed_at_ms: envelope.timestamp_ms,
                    result_message_id,
                };
                Ok(())
            }
            ChatTaskState::Claimed { .. } => {
                Err(LoomError::new(Code::Conflict, "chat task claim mismatch"))
            }
            ChatTaskState::Open => Err(LoomError::new(Code::Conflict, "chat task is not claimed")),
            ChatTaskState::Completed { .. } => {
                Err(LoomError::new(Code::Conflict, "chat task is completed"))
            }
        }
    }

    fn apply_agent_invoked(
        &mut self,
        envelope: &OperationEnvelope,
        invocation_id: String,
        agent_principal: WorkspaceId,
        source_message_ids: Vec<String>,
        prompt: Vec<u8>,
    ) -> Result<()> {
        if self.agent_invocations.contains_key(&invocation_id) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "chat agent invocation exists",
            ));
        }
        for message_id in &source_message_ids {
            if !self.messages.contains_key(message_id) {
                return Err(LoomError::not_found("chat agent source message not found"));
            }
        }
        self.agent_invocations.insert(
            invocation_id.clone(),
            ChatAgentInvocation {
                invocation_id,
                agent_principal,
                requested_by: envelope.actor_principal,
                requested_at_ms: envelope.timestamp_ms,
                source_message_ids,
                prompt,
                reply_message_ids: Vec::new(),
            },
        );
        Ok(())
    }

    fn apply_agent_replied(&mut self, invocation_id: String, message_id: String) -> Result<()> {
        if !self.messages.contains_key(&message_id) {
            return Err(LoomError::not_found("chat agent reply message not found"));
        }
        let invocation = self
            .agent_invocations
            .get_mut(&invocation_id)
            .ok_or_else(|| LoomError::not_found("chat agent invocation not found"))?;
        if invocation.reply_message_ids.contains(&message_id) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "chat agent reply exists",
            ));
        }
        invocation.reply_message_ids.push(message_id);
        Ok(())
    }

    fn apply_handoff_requested(
        &mut self,
        envelope: &OperationEnvelope,
        handoff_id: String,
        from_agent_principal: WorkspaceId,
        to_principal: Option<WorkspaceId>,
        reason: Option<String>,
    ) -> Result<()> {
        if self.handoffs.contains_key(&handoff_id) {
            return Err(LoomError::new(Code::AlreadyExists, "chat handoff exists"));
        }
        self.handoffs.insert(
            handoff_id.clone(),
            ChatHandoffRequest {
                handoff_id,
                from_agent_principal,
                to_principal,
                requested_by: envelope.actor_principal,
                requested_at_ms: envelope.timestamp_ms,
                reason,
            },
        );
        Ok(())
    }

    fn task_mut(&mut self, task_id: &str) -> Result<&mut ChatTask> {
        self.tasks
            .get_mut(task_id)
            .ok_or_else(|| LoomError::not_found("chat task not found"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChatMessage {
    message_id: String,
    thread_id: Option<String>,
    body: Vec<u8>,
    annotation_id: String,
    author_principal: WorkspaceId,
    created_at_ms: u64,
    updated_at_ms: u64,
    redacted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessageView {
    pub message_id: String,
    pub thread_id: Option<String>,
    pub body: Vec<u8>,
    pub author_principal: WorkspaceId,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub redacted: bool,
    pub reactions: Vec<ChatReactionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatThread {
    pub thread_id: String,
    pub parent_message_id: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatReactionSummary {
    pub kind: String,
    pub principal: WorkspaceId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatTask {
    pub task_id: String,
    pub message_id: Option<String>,
    pub title: String,
    pub created_by: WorkspaceId,
    pub created_at_ms: u64,
    pub state: ChatTaskState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatTaskState {
    Open,
    Claimed {
        claim_id: String,
        claimant_principal: WorkspaceId,
        claimed_by: WorkspaceId,
        claimed_at_ms: u64,
        lease_token: Option<String>,
    },
    Completed {
        claim_id: String,
        completed_by: WorkspaceId,
        completed_principal: WorkspaceId,
        completed_at_ms: u64,
        result_message_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatAgentInvocation {
    pub invocation_id: String,
    pub agent_principal: WorkspaceId,
    pub requested_by: WorkspaceId,
    pub requested_at_ms: u64,
    pub source_message_ids: Vec<String>,
    pub prompt: Vec<u8>,
    pub reply_message_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatHandoffRequest {
    pub handoff_id: String,
    pub from_agent_principal: WorkspaceId,
    pub to_principal: Option<WorkspaceId>,
    pub requested_by: WorkspaceId,
    pub requested_at_ms: u64,
    pub reason: Option<String>,
}

pub fn chat_profile_operation_log_key(workspace_id: &str, channel_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    validate_text("channel_id", channel_id)?;
    Ok(
        format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/channels/{channel_id}/operations")
            .into_bytes(),
    )
}

pub fn chat_channel_directory_key(workspace_id: &str) -> Result<Vec<u8>> {
    validate_text("workspace_id", workspace_id)?;
    Ok(format!("{PROFILE_CONTROL_PREFIX}/{workspace_id}/channels/index.lch").into_bytes())
}

pub fn chat_operation_cursor_scope(workspace_id: &str, channel_id: &str) -> String {
    format!("chat:{workspace_id}:{channel_id}")
}

pub fn chat_message_annotation_id(message_id: &str) -> String {
    format!("message:{message_id}")
}

fn validate_body(body: &[u8]) -> Result<()> {
    if body.is_empty() {
        return Err(LoomError::invalid("chat message body must not be empty"));
    }
    Ok(())
}

fn normalize_channel_handle(value: &str) -> Result<String> {
    let value = value.to_ascii_lowercase();
    let bytes = value.as_bytes();
    if !(1..=64).contains(&bytes.len()) {
        return Err(LoomError::invalid(
            "chat channel handle must be 1 to 64 ASCII characters",
        ));
    }
    if !bytes[0].is_ascii_alphanumeric() || !bytes[bytes.len() - 1].is_ascii_alphanumeric() {
        return Err(LoomError::invalid(
            "chat channel handle must start and end with an ASCII letter or digit",
        ));
    }
    if bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(*byte, b'.' | b'_' | b'-')
    }) {
        Ok(value)
    } else {
        Err(LoomError::invalid(
            "chat channel handle must use lowercase ASCII letters, digits, '.', '_', or '-'",
        ))
    }
}

fn chat_record_list(value: Value) -> Result<Vec<ChatOperationRecord>> {
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(ChatOperationRecord::from_value)
            .collect(),
        _ => Err(LoomError::corrupt(
            "chat operation records must be an array",
        )),
    }
}

fn reaction_summaries(reactions: &BTreeSet<ReactionKey>) -> Vec<ChatReactionSummary> {
    reactions
        .iter()
        .map(|reaction| ChatReactionSummary {
            kind: reaction.kind.clone(),
            principal: reaction.principal,
        })
        .collect()
}

fn optional_text_value(value: Option<&str>) -> Value {
    match value {
        Some(value) => Value::Array(vec![Value::Uint(1), Value::Text(value.to_string())]),
        None => Value::Array(vec![Value::Uint(0)]),
    }
}

fn optional_id_value(value: Option<WorkspaceId>) -> Value {
    optional_text_value(value.map(|id| id.to_string()).as_deref())
}

fn string_list_value(values: &[String]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|value| Value::Text(value.clone()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use loom_types::Algo;

    use super::*;
    use crate::{ActorKind, OperationEnvelopeInput};

    fn actor(byte: u8) -> WorkspaceId {
        WorkspaceId::v4_from_bytes([byte; 16])
    }

    fn digest(byte: u8) -> Digest {
        Digest::hash(Algo::Blake3, &[byte])
    }

    fn record(
        sequence: u64,
        operation_id: &str,
        payload: ChatOperationPayload,
        actor: WorkspaceId,
    ) -> ChatOperationRecord {
        let payload_bytes = payload.encode().unwrap();
        let envelope = OperationEnvelope::new(
            Algo::Blake3,
            OperationEnvelopeInput {
                workspace_id: "studio",
                app_id: APP_ID,
                scope_id: "general",
                operation_id,
                operation_kind: payload.operation_kind(),
                sequence,
                actor_principal: actor,
                actor_kind: ActorKind::User,
                timestamp_ms: sequence * 100,
                idempotency_key: operation_id,
                base_root: digest(0),
                base_entity_version: None,
                target_entity_id: Some(payload.target_entity_id()),
                payload: &payload_bytes,
                policy_labels: &[],
                signature: None,
                agent: None,
            },
        )
        .unwrap();
        ChatOperationRecord::new(
            sequence,
            operation_id,
            payload.operation_kind(),
            Some(payload.target_entity_id().to_string()),
            digest(sequence as u8),
            envelope.encode().unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn chat_operation_payload_round_trips() {
        let payload = ChatOperationPayload::MessageCreated {
            message_id: "m1".to_string(),
            thread_id: None,
            body: b"hello".to_vec(),
        };
        let encoded = payload.encode().unwrap();
        assert_eq!(ChatOperationPayload::decode(&encoded).unwrap(), payload);
    }

    #[test]
    fn channel_log_replays_messages_threads_and_reactions() {
        let alice = actor(1);
        let bob = actor(2);
        let log = ChannelOperationLog::new(
            "studio",
            "general",
            vec![
                record(
                    1,
                    "op-1",
                    ChatOperationPayload::MessageCreated {
                        message_id: "m1".to_string(),
                        thread_id: None,
                        body: b"hello".to_vec(),
                    },
                    alice,
                ),
                record(
                    2,
                    "op-2",
                    ChatOperationPayload::ThreadCreated {
                        thread_id: "t1".to_string(),
                        parent_message_id: "m1".to_string(),
                    },
                    alice,
                ),
                record(
                    3,
                    "op-3",
                    ChatOperationPayload::MessageCreated {
                        message_id: "m2".to_string(),
                        thread_id: Some("t1".to_string()),
                        body: b"reply".to_vec(),
                    },
                    bob,
                ),
                record(
                    4,
                    "op-4",
                    ChatOperationPayload::ReactionAdded {
                        message_id: "m1".to_string(),
                        kind: "thumbsup".to_string(),
                    },
                    bob,
                ),
                record(
                    5,
                    "op-5",
                    ChatOperationPayload::MessageEdited {
                        message_id: "m2".to_string(),
                        body: b"reply edited".to_vec(),
                    },
                    bob,
                ),
            ],
        )
        .unwrap();

        let projection = log.project().unwrap();
        let messages = projection.messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].message_id, "m1");
        assert_eq!(messages[0].reactions.len(), 1);
        assert_eq!(messages[0].reactions[0].kind, "thumbsup");
        assert_eq!(messages[0].reactions[0].principal, bob);
        assert_eq!(messages[1].thread_id.as_deref(), Some("t1"));
        assert_eq!(messages[1].body, b"reply edited".to_vec());
        assert_eq!(projection.threads()[0].parent_message_id, "m1");

        let encoded = log.encode().unwrap();
        assert_eq!(ChannelOperationLog::decode(&encoded).unwrap(), log);
    }

    #[test]
    fn channel_log_supports_operation_change_cursors() {
        let alice = actor(1);
        let log = ChannelOperationLog::new(
            "studio",
            "general",
            vec![
                record(
                    1,
                    "op-1",
                    ChatOperationPayload::MessageCreated {
                        message_id: "m1".to_string(),
                        thread_id: None,
                        body: b"hello".to_vec(),
                    },
                    alice,
                ),
                record(
                    2,
                    "op-2",
                    ChatOperationPayload::ReactionAdded {
                        message_id: "m1".to_string(),
                        kind: "eyes".to_string(),
                    },
                    alice,
                ),
            ],
        )
        .unwrap();
        let cursor =
            OperationChangeCursor::new(chat_operation_cursor_scope("studio", "general"), 1)
                .unwrap();

        let batch = log.changes(&cursor, 1).unwrap();

        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].operation_kind, "message.created");
        assert_eq!(batch.next.encode(), "oplog:2:chat:studio:general");
        let second = log.changes(&batch.next, 10).unwrap();
        assert_eq!(second.events[0].operation_kind, "reaction.added");
        assert_eq!(second.next.encode(), "oplog:3:chat:studio:general");
    }

    #[test]
    fn channel_log_replays_tasks_agents_and_handoffs() {
        let alice = actor(1);
        let bob = actor(2);
        let agent = actor(9);
        let log = ChannelOperationLog::new(
            "studio",
            "general",
            vec![
                record(
                    1,
                    "op-1",
                    ChatOperationPayload::MessageCreated {
                        message_id: "m1".to_string(),
                        thread_id: None,
                        body: b"incident".to_vec(),
                    },
                    alice,
                ),
                record(
                    2,
                    "op-2",
                    ChatOperationPayload::TaskCreated {
                        task_id: "task-1".to_string(),
                        message_id: Some("m1".to_string()),
                        title: "investigate".to_string(),
                    },
                    alice,
                ),
                record(
                    3,
                    "op-3",
                    ChatOperationPayload::TaskClaimed {
                        task_id: "task-1".to_string(),
                        claim_id: "claim-1".to_string(),
                        claimant_principal: bob,
                        lease_token: Some("lease-1".to_string()),
                    },
                    bob,
                ),
                record(
                    4,
                    "op-4",
                    ChatOperationPayload::MessageCreated {
                        message_id: "m2".to_string(),
                        thread_id: None,
                        body: b"done".to_vec(),
                    },
                    bob,
                ),
                record(
                    5,
                    "op-5",
                    ChatOperationPayload::TaskCompleted {
                        task_id: "task-1".to_string(),
                        claim_id: "claim-1".to_string(),
                        result_message_id: Some("m2".to_string()),
                    },
                    bob,
                ),
                record(
                    6,
                    "op-6",
                    ChatOperationPayload::AgentInvoked {
                        invocation_id: "invoke-1".to_string(),
                        agent_principal: agent,
                        source_message_ids: vec!["m1".to_string()],
                        prompt: b"summarize".to_vec(),
                    },
                    alice,
                ),
                record(
                    7,
                    "op-7",
                    ChatOperationPayload::MessageCreated {
                        message_id: "m3".to_string(),
                        thread_id: None,
                        body: b"summary".to_vec(),
                    },
                    agent,
                ),
                record(
                    8,
                    "op-8",
                    ChatOperationPayload::AgentReplied {
                        invocation_id: "invoke-1".to_string(),
                        message_id: "m3".to_string(),
                    },
                    agent,
                ),
                record(
                    9,
                    "op-9",
                    ChatOperationPayload::HandoffRequested {
                        handoff_id: "handoff-1".to_string(),
                        from_agent_principal: agent,
                        to_principal: Some(alice),
                        reason: Some("needs approval".to_string()),
                    },
                    agent,
                ),
            ],
        )
        .unwrap();

        let projection = log.project().unwrap();
        let task = &projection.tasks()[0];
        assert_eq!(task.task_id, "task-1");
        assert_eq!(task.message_id.as_deref(), Some("m1"));
        assert!(matches!(
            &task.state,
            ChatTaskState::Completed {
                claim_id,
                completed_principal,
                result_message_id,
                ..
            } if claim_id == "claim-1"
                && *completed_principal == bob
                && result_message_id.as_deref() == Some("m2")
        ));
        let invocation = &projection.agent_invocations()[0];
        assert_eq!(invocation.invocation_id, "invoke-1");
        assert_eq!(invocation.agent_principal, agent);
        assert_eq!(invocation.reply_message_ids, vec!["m3".to_string()]);
        let handoff = &projection.handoffs()[0];
        assert_eq!(handoff.from_agent_principal, agent);
        assert_eq!(handoff.to_principal, Some(alice));
    }

    #[test]
    fn channel_log_rejects_duplicate_task_claims() {
        let alice = actor(1);
        let bob = actor(2);
        let log = ChannelOperationLog::new(
            "studio",
            "general",
            vec![
                record(
                    1,
                    "op-1",
                    ChatOperationPayload::TaskCreated {
                        task_id: "task-1".to_string(),
                        message_id: None,
                        title: "investigate".to_string(),
                    },
                    alice,
                ),
                record(
                    2,
                    "op-2",
                    ChatOperationPayload::TaskClaimed {
                        task_id: "task-1".to_string(),
                        claim_id: "claim-1".to_string(),
                        claimant_principal: alice,
                        lease_token: None,
                    },
                    alice,
                ),
                record(
                    3,
                    "op-3",
                    ChatOperationPayload::TaskClaimed {
                        task_id: "task-1".to_string(),
                        claim_id: "claim-2".to_string(),
                        claimant_principal: bob,
                        lease_token: None,
                    },
                    bob,
                ),
            ],
        )
        .unwrap();

        assert_eq!(log.project().unwrap_err().code, Code::Conflict);
    }

    #[test]
    fn channel_log_rejects_invalid_replay_order() {
        let alice = actor(1);
        let log = ChannelOperationLog::new(
            "studio",
            "general",
            vec![record(
                1,
                "op-1",
                ChatOperationPayload::ReactionAdded {
                    message_id: "missing".to_string(),
                    kind: "eyes".to_string(),
                },
                alice,
            )],
        )
        .unwrap();

        assert_eq!(log.project().unwrap_err().code, Code::NotFound);
    }

    #[test]
    fn channel_directory_resolves_current_and_retained_handles() {
        let mut directory = ChatChannelDirectory::new("studio").unwrap();
        let channel = directory
            .create_channel(actor(7), "general", "General discussion")
            .unwrap();
        directory.rename_channel(channel.id, "team-chat").unwrap();

        assert_eq!(
            directory.resolve("general").unwrap().unwrap().id,
            channel.id
        );
        assert_eq!(
            directory.resolve("TEAM-CHAT").unwrap().unwrap().id,
            channel.id
        );
        assert_eq!(
            directory
                .resolve(&channel.id.to_string())
                .unwrap()
                .unwrap()
                .handle,
            "team-chat"
        );
        assert_eq!(
            directory
                .create_channel(actor(8), "general", "Other")
                .unwrap_err()
                .code,
            Code::AlreadyExists
        );
        assert_eq!(
            ChatChannelDirectory::decode(&directory.encode().unwrap()).unwrap(),
            directory
        );
    }
}
