use std::collections::BTreeMap;
use std::sync::Mutex;

use loom_core::error::{Code, LoomError};
use loom_core::workspace::{AclDomain, WorkspaceId};
use loom_core::{AclRight, Digest, Loom};
use loom_store::FileStore;
use loom_substrate::annotation::{EMOJI_REGISTRY_DIR, EmojiRegistry, emoji_registry_path};
use loom_substrate::changes::{OperationChangeBatch, OperationChangeCursor};
use loom_substrate::chat::{
    APP_ID, ChannelOperationLog, ChatAgentInvocation, ChatChannelDirectory, ChatHandoffRequest,
    ChatMessageView, ChatOperationPayload, ChatOperationRecord, ChatReactionSummary, ChatTask,
    ChatTaskState, ChatThread, chat_channel_directory_key, chat_profile_operation_log_key,
};
use loom_substrate::refs::{EntityRef, MarkdownReferenceKind, ReferenceSource};
use loom_substrate::versioning::{
    BodyRef, ProfileRevisionUpdate, ProfileTransaction, ProfileTransactionState,
    REVISION_INDEX_DIR, RevisionIndex, revision_index_path,
};
use loom_substrate::{ActorKind, OperationEnvelope, OperationEnvelopeInput};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedChatMessage {
    pub message_id: String,
    pub thread_id: Option<String>,
    pub body: Vec<u8>,
    pub author_principal: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub redacted: bool,
    pub reactions: Vec<HostedChatReaction>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedChatReaction {
    pub kind: String,
    pub principal: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedChatThread {
    pub thread_id: String,
    pub parent_message_id: String,
    pub created_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedChatChannel {
    pub workspace_id: String,
    pub channel_id: String,
    pub messages: Vec<HostedChatMessage>,
    pub threads: Vec<HostedChatThread>,
    pub tasks: Vec<HostedChatTask>,
    pub agent_invocations: Vec<HostedChatAgentInvocation>,
    pub handoffs: Vec<HostedChatHandoff>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedChatTask {
    pub task_id: String,
    pub message_id: Option<String>,
    pub title: String,
    pub created_by: String,
    pub created_at_ms: u64,
    pub state: HostedChatTaskState,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind")]
pub enum HostedChatTaskState {
    Open,
    Claimed {
        claim_id: String,
        claimant_principal: String,
        claimed_by: String,
        claimed_at_ms: u64,
        lease_token: Option<String>,
    },
    Completed {
        claim_id: String,
        completed_by: String,
        completed_principal: String,
        completed_at_ms: u64,
        result_message_id: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedChatAgentInvocation {
    pub invocation_id: String,
    pub agent_principal: String,
    pub requested_by: String,
    pub requested_at_ms: u64,
    pub source_message_ids: Vec<String>,
    pub prompt: Vec<u8>,
    pub reply_message_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedChatHandoff {
    pub handoff_id: String,
    pub from_agent_principal: String,
    pub to_principal: Option<String>,
    pub requested_by: String,
    pub requested_at_ms: u64,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedChatWrite {
    pub workspace_id: String,
    pub channel_id: String,
    pub operation_id: String,
    pub operation_kind: String,
    pub sequence: u64,
    pub root_after: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedChatCursor {
    pub workspace_id: String,
    pub channel_id: String,
    pub principal: String,
    pub next_sequence: u64,
    pub head_sequence: u64,
    pub unread_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedChatPresence {
    pub workspace_id: String,
    pub channel_id: String,
    pub principal: String,
    pub status: String,
    pub expires_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedChatEmojiRegistry {
    pub workspace_id: String,
    pub custom: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedChatChannelSummary {
    pub workspace_id: String,
    pub channel_id: String,
    pub handle: String,
    pub name: String,
}

#[derive(Default)]
pub struct HostedChatPresenceStore {
    entries: Mutex<BTreeMap<PresenceKey, HostedChatPresence>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PresenceKey {
    workspace: WorkspaceId,
    workspace_id: String,
    channel_id: String,
    principal: String,
}

impl HostedChatPresenceStore {
    pub fn set(
        &self,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        principal: &str,
        status: &str,
        ttl_ms: u64,
        now_ms: u64,
    ) -> loom_core::Result<HostedChatPresence> {
        if status.is_empty() || status.chars().any(char::is_control) {
            return Err(LoomError::invalid("invalid chat presence status"));
        }
        if ttl_ms == 0 || ttl_ms > 300_000 {
            return Err(LoomError::invalid(
                "chat presence ttl must be between 1 and 300000 ms",
            ));
        }
        let entry = HostedChatPresence {
            workspace_id: workspace_id.to_string(),
            channel_id: channel_id.to_string(),
            principal: principal.to_string(),
            status: status.to_string(),
            expires_at_ms: now_ms.saturating_add(ttl_ms),
        };
        let key = PresenceKey {
            workspace,
            workspace_id: workspace_id.to_string(),
            channel_id: channel_id.to_string(),
            principal: principal.to_string(),
        };
        let mut entries = self.entries.lock().expect("presence lock");
        entries.retain(|_, value| value.expires_at_ms > now_ms);
        entries.insert(key, entry.clone());
        Ok(entry)
    }

    pub fn list(
        &self,
        workspace: WorkspaceId,
        workspace_id: &str,
        channel_id: &str,
        now_ms: u64,
    ) -> Vec<HostedChatPresence> {
        let mut entries = self.entries.lock().expect("presence lock");
        entries.retain(|_, value| value.expires_at_ms > now_ms);
        entries
            .iter()
            .filter(|(key, _)| {
                key.workspace == workspace
                    && key.workspace_id == workspace_id
                    && key.channel_id == channel_id
            })
            .map(|(_, value)| value.clone())
            .collect()
    }
}

pub fn ensure_channel(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: WorkspaceId,
    handle: &str,
    name: &str,
) -> loom_core::Result<HostedChatChannelSummary> {
    loom.authorize_domain(workspace, AclDomain::Chat, AclRight::Write)?;
    let mut directory = load_channel_directory(loom, workspace, workspace_id)?;
    if let Some(channel) = directory.resolve(&channel_id.to_string())? {
        return Ok(channel_summary(workspace_id, channel));
    }
    match directory.create_channel(channel_id, handle, name) {
        Ok(channel) => {
            let summary = channel_summary(workspace_id, &channel);
            save_channel_directory(loom, workspace, workspace_id, &directory)?;
            Ok(summary)
        }
        Err(error) if error.code == Code::AlreadyExists => {
            let channel = directory
                .resolve(handle)?
                .ok_or_else(|| LoomError::corrupt("chat channel directory conflict"))?;
            Ok(channel_summary(workspace_id, channel))
        }
        Err(error) => Err(error),
    }
}

pub fn rename_channel(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    selector: &str,
    handle: &str,
) -> loom_core::Result<HostedChatChannelSummary> {
    loom.authorize_domain(workspace, AclDomain::Chat, AclRight::Write)?;
    let mut directory = load_channel_directory(loom, workspace, workspace_id)?;
    let id = directory
        .resolve(selector)?
        .ok_or_else(|| LoomError::not_found("chat channel not found"))?
        .id;
    directory.rename_channel(id, handle)?;
    let channel = directory
        .resolve(&id.to_string())?
        .ok_or_else(|| LoomError::corrupt("renamed chat channel is missing"))?
        .clone();
    save_channel_directory(loom, workspace, workspace_id, &directory)?;
    Ok(channel_summary(workspace_id, &channel))
}

pub fn list_channels(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> loom_core::Result<Vec<HostedChatChannelSummary>> {
    loom.authorize_domain(workspace, AclDomain::Chat, AclRight::Read)?;
    let directory = load_channel_directory(loom, workspace, workspace_id)?;
    Ok(directory
        .channels()
        .map(|channel| channel_summary(workspace_id, channel))
        .collect())
}

pub fn post_message(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    thread_id: Option<&str>,
    body: Vec<u8>,
) -> loom_core::Result<HostedChatWrite> {
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    append_payload(
        loom,
        workspace,
        workspace_id,
        &channel_id,
        ChatOperationPayload::MessageCreated {
            message_id: message_id.to_string(),
            thread_id: thread_id.map(str::to_string),
            body,
        },
    )
}

pub fn edit_message(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    body: Vec<u8>,
) -> loom_core::Result<HostedChatWrite> {
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    append_payload(
        loom,
        workspace,
        workspace_id,
        &channel_id,
        ChatOperationPayload::MessageEdited {
            message_id: message_id.to_string(),
            body,
        },
    )
}

pub fn redact_message(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    reason: Option<&str>,
) -> loom_core::Result<HostedChatWrite> {
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    append_payload(
        loom,
        workspace,
        workspace_id,
        &channel_id,
        ChatOperationPayload::MessageRedacted {
            message_id: message_id.to_string(),
            reason: reason.map(str::to_string),
        },
    )
}

pub fn create_thread(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    thread_id: &str,
    parent_message_id: &str,
) -> loom_core::Result<HostedChatWrite> {
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    append_payload(
        loom,
        workspace,
        workspace_id,
        &channel_id,
        ChatOperationPayload::ThreadCreated {
            thread_id: thread_id.to_string(),
            parent_message_id: parent_message_id.to_string(),
        },
    )
}

pub fn create_task(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    task_id: &str,
    message_id: Option<&str>,
    title: &str,
) -> loom_core::Result<HostedChatWrite> {
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    append_payload(
        loom,
        workspace,
        workspace_id,
        &channel_id,
        ChatOperationPayload::TaskCreated {
            task_id: task_id.to_string(),
            message_id: message_id.map(str::to_string),
            title: title.to_string(),
        },
    )
}

pub fn claim_task(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    task_id: &str,
    claim_id: &str,
    lease_token: Option<&str>,
) -> loom_core::Result<HostedChatWrite> {
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    let claimant_principal = loom.effective_principal()?.unwrap_or(workspace);
    append_payload(
        loom,
        workspace,
        workspace_id,
        &channel_id,
        ChatOperationPayload::TaskClaimed {
            task_id: task_id.to_string(),
            claim_id: claim_id.to_string(),
            claimant_principal,
            lease_token: lease_token.map(str::to_string),
        },
    )
}

pub fn complete_task(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    task_id: &str,
    claim_id: &str,
    result_message_id: Option<&str>,
) -> loom_core::Result<HostedChatWrite> {
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    append_payload(
        loom,
        workspace,
        workspace_id,
        &channel_id,
        ChatOperationPayload::TaskCompleted {
            task_id: task_id.to_string(),
            claim_id: claim_id.to_string(),
            result_message_id: result_message_id.map(str::to_string),
        },
    )
}

pub fn invoke_agent(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    invocation_id: &str,
    agent_principal: WorkspaceId,
    source_message_ids: Vec<String>,
    prompt: Vec<u8>,
) -> loom_core::Result<HostedChatWrite> {
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    append_payload(
        loom,
        workspace,
        workspace_id,
        &channel_id,
        ChatOperationPayload::AgentInvoked {
            invocation_id: invocation_id.to_string(),
            agent_principal,
            source_message_ids,
            prompt,
        },
    )
}

pub fn agent_reply(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    invocation_id: &str,
    message_id: &str,
) -> loom_core::Result<HostedChatWrite> {
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    append_payload(
        loom,
        workspace,
        workspace_id,
        &channel_id,
        ChatOperationPayload::AgentReplied {
            invocation_id: invocation_id.to_string(),
            message_id: message_id.to_string(),
        },
    )
}

pub fn request_handoff(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    handoff_id: &str,
    from_agent_principal: WorkspaceId,
    to_principal: Option<WorkspaceId>,
    reason: Option<&str>,
) -> loom_core::Result<HostedChatWrite> {
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    append_payload(
        loom,
        workspace,
        workspace_id,
        &channel_id,
        ChatOperationPayload::HandoffRequested {
            handoff_id: handoff_id.to_string(),
            from_agent_principal,
            to_principal,
            reason: reason.map(str::to_string),
        },
    )
}

pub fn add_reaction(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    kind: &str,
) -> loom_core::Result<HostedChatWrite> {
    ensure_reaction_kind(loom, workspace, workspace_id, kind)?;
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    append_payload(
        loom,
        workspace,
        workspace_id,
        &channel_id,
        ChatOperationPayload::ReactionAdded {
            message_id: message_id.to_string(),
            kind: kind.to_string(),
        },
    )
}

pub fn remove_reaction(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    kind: &str,
) -> loom_core::Result<HostedChatWrite> {
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    append_payload(
        loom,
        workspace,
        workspace_id,
        &channel_id,
        ChatOperationPayload::ReactionRemoved {
            message_id: message_id.to_string(),
            kind: kind.to_string(),
        },
    )
}

pub fn emoji_registry(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> loom_core::Result<HostedChatEmojiRegistry> {
    loom.authorize_domain(workspace, AclDomain::Chat, AclRight::Read)?;
    emoji_registry_summary(
        workspace_id,
        &load_emoji_registry(loom, workspace, workspace_id)?,
    )
}

pub fn register_emoji(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    kind: &str,
) -> loom_core::Result<HostedChatEmojiRegistry> {
    loom.authorize_domain(workspace, AclDomain::Chat, AclRight::Admin)?;
    let mut registry = load_emoji_registry(loom, workspace, workspace_id)?;
    registry.register(kind)?;
    save_emoji_registry(loom, workspace, workspace_id, &registry)?;
    emoji_registry_summary(workspace_id, &registry)
}

pub fn unregister_emoji(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    kind: &str,
) -> loom_core::Result<HostedChatEmojiRegistry> {
    loom.authorize_domain(workspace, AclDomain::Chat, AclRight::Admin)?;
    let mut registry = load_emoji_registry(loom, workspace, workspace_id)?;
    registry.unregister(kind);
    save_emoji_registry(loom, workspace, workspace_id, &registry)?;
    emoji_registry_summary(workspace_id, &registry)
}

pub fn channel_projection(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
) -> loom_core::Result<HostedChatChannel> {
    loom.authorize_domain(workspace, AclDomain::Chat, AclRight::Read)?;
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    let projection = load_log(loom, workspace, workspace_id, &channel_id)?.project()?;
    let messages = projection
        .messages()
        .into_iter()
        .map(message_summary)
        .collect();
    let threads = projection
        .threads()
        .into_iter()
        .map(thread_summary)
        .collect();
    let tasks = projection.tasks().into_iter().map(task_summary).collect();
    let agent_invocations = projection
        .agent_invocations()
        .into_iter()
        .map(agent_invocation_summary)
        .collect();
    let handoffs = projection
        .handoffs()
        .into_iter()
        .map(handoff_summary)
        .collect();
    Ok(HostedChatChannel {
        workspace_id: projection.workspace_id,
        channel_id: projection.channel_id,
        messages,
        threads,
        tasks,
        agent_invocations,
        handoffs,
    })
}

pub fn read_cursor(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
) -> loom_core::Result<HostedChatCursor> {
    loom.authorize_domain(workspace, AclDomain::Chat, AclRight::Read)?;
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    let stream = chat_stream_name(workspace_id, &channel_id)?;
    let principal = chat_consumer_id(loom, workspace)?;
    let head_sequence = stream_len_or_zero(loom, workspace, &stream)? as u64;
    let next_sequence = loom.consumer_position_internal(workspace, &stream, &principal)?;
    Ok(HostedChatCursor {
        workspace_id: workspace_id.to_string(),
        channel_id: channel_id.to_string(),
        principal,
        next_sequence,
        head_sequence,
        unread_count: head_sequence.saturating_sub(next_sequence),
    })
}

pub fn update_cursor(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    next_sequence: u64,
) -> loom_core::Result<HostedChatCursor> {
    loom.authorize_domain(workspace, AclDomain::Chat, AclRight::Advance)?;
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    let stream = chat_stream_name(workspace_id, &channel_id)?;
    let head_sequence = stream_len_or_zero(loom, workspace, &stream)? as u64;
    if next_sequence > head_sequence {
        return Err(LoomError::invalid(format!(
            "chat cursor {next_sequence} is past channel head {head_sequence}"
        )));
    }
    if head_sequence > 0 || next_sequence > 0 {
        let principal = chat_consumer_id(loom, workspace)?;
        loom.consumer_advance_internal(workspace, &stream, &principal, next_sequence)?;
    }
    read_cursor(loom, workspace, workspace_id, &channel_id)
}

pub fn operation_changes(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    from_sequence: u64,
    max: usize,
) -> loom_core::Result<OperationChangeBatch> {
    loom.authorize_domain(workspace, AclDomain::Chat, AclRight::Read)?;
    let channel_id = resolve_channel_id(loom, workspace, workspace_id, channel_id)?;
    let cursor =
        OperationChangeCursor::new(format!("chat:{workspace_id}:{channel_id}"), from_sequence)?;
    load_log(loom, workspace, workspace_id, &channel_id)?.changes(&cursor, max)
}

pub fn resolve_channel_id(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    selector: &str,
) -> loom_core::Result<String> {
    let directory = load_channel_directory(loom, workspace, workspace_id)?;
    directory
        .resolve(selector)?
        .map(|channel| channel.id.to_string())
        .ok_or_else(|| LoomError::not_found("chat channel not found"))
}

fn load_channel_directory(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> loom_core::Result<ChatChannelDirectory> {
    let path = String::from_utf8(chat_channel_directory_key(workspace_id)?)
        .map_err(|_| LoomError::corrupt("chat channel directory path is not utf-8"))?;
    loom.authorize_file_path(workspace, &path, AclRight::Read)?;
    match loom.read_file_reserved(workspace, &path) {
        Ok(bytes) => {
            let directory = ChatChannelDirectory::decode(&bytes)?;
            if directory.workspace_id != workspace_id {
                return Err(LoomError::corrupt(
                    "chat channel directory workspace mismatch",
                ));
            }
            Ok(directory)
        }
        Err(error) if error.code == Code::NotFound => ChatChannelDirectory::new(workspace_id),
        Err(error) => Err(error),
    }
}

fn save_channel_directory(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    directory: &ChatChannelDirectory,
) -> loom_core::Result<()> {
    let path = String::from_utf8(chat_channel_directory_key(workspace_id)?)
        .map_err(|_| LoomError::corrupt("chat channel directory path is not utf-8"))?;
    let dir = format!("profile/chat/v1/{workspace_id}/channels");
    loom.create_directory_reserved(workspace, &dir, true)?;
    loom.write_file_reserved(workspace, &path, &directory.encode()?, 0o100644)
}

fn channel_summary(
    workspace_id: &str,
    channel: &loom_substrate::chat::ChatChannel,
) -> HostedChatChannelSummary {
    HostedChatChannelSummary {
        workspace_id: workspace_id.to_string(),
        channel_id: channel.id.to_string(),
        handle: channel.handle.clone(),
        name: channel.name.clone(),
    }
}

fn load_log(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
) -> loom_core::Result<ChannelOperationLog> {
    let stream = chat_stream_name(workspace_id, channel_id)?;
    let len = match loom.stream_len(workspace, &stream) {
        Ok(len) => len,
        Err(err) if err.code == Code::NotFound => {
            return ChannelOperationLog::new(workspace_id, channel_id, Vec::new());
        }
        Err(err) => return Err(err),
    };
    let records = loom
        .stream_range(workspace, &stream, 0, len)?
        .into_iter()
        .map(|entry| ChatOperationRecord::decode(&entry))
        .collect::<loom_core::Result<Vec<_>>>()?;
    ChannelOperationLog::new(workspace_id, channel_id, records)
}

fn append_payload(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    payload: ChatOperationPayload,
) -> loom_core::Result<HostedChatWrite> {
    loom.authorize_domain(workspace, AclDomain::Chat, AclRight::Write)?;
    let mut log = load_log(loom, workspace, workspace_id, channel_id)?;
    let previous = log.encode()?;
    let sequence = log
        .records
        .last()
        .map(|record| record.sequence.saturating_add(1))
        .unwrap_or(1);
    let payload_bytes = payload.encode()?;
    let root_after = Digest::hash(loom.store().digest_algo(), &payload_bytes);
    let operation_id = format!("{workspace_id}:{channel_id}:{sequence}");
    let actor_principal = loom.effective_principal()?.unwrap_or(workspace);
    let envelope = OperationEnvelope::new(
        loom.store().digest_algo(),
        OperationEnvelopeInput {
            workspace_id,
            app_id: APP_ID,
            scope_id: channel_id,
            operation_id: &operation_id,
            operation_kind: payload.operation_kind(),
            sequence,
            actor_principal,
            actor_kind: ActorKind::User,
            timestamp_ms: now_ms(),
            idempotency_key: &operation_id,
            base_root: Digest::hash(loom.store().digest_algo(), &previous),
            base_entity_version: None,
            target_entity_id: Some(payload.target_entity_id()),
            payload: &payload_bytes,
            policy_labels: &[],
            signature: None,
            agent: None,
        },
    )?;
    let record = ChatOperationRecord::new(
        sequence,
        operation_id,
        payload.operation_kind(),
        Some(payload.target_entity_id().to_string()),
        root_after,
        envelope.encode()?,
    )?;
    log.records.push(record.clone());
    let projected = log.project()?;
    append_record(loom, workspace, workspace_id, channel_id, &record)?;
    update_message_revision_index(
        loom,
        workspace,
        workspace_id,
        channel_id,
        &payload,
        &record,
        &payload_bytes,
    )?;
    match &payload {
        ChatOperationPayload::MessageCreated {
            message_id, body, ..
        }
        | ChatOperationPayload::MessageEdited { message_id, body } => {
            update_message_refs(MessageRefUpdate {
                loom,
                workspace,
                workspace_id,
                channel_id,
                message_id,
                operation_id: &record.operation_id,
                source_root: record.root_after,
                body,
                now_ms: now_ms(),
            })?;
        }
        _ => {}
    }
    Ok(HostedChatWrite {
        workspace_id: projected.workspace_id,
        channel_id: projected.channel_id,
        operation_id: record.operation_id,
        operation_kind: record.operation_kind,
        sequence: record.sequence,
        root_after: record.root_after.to_string(),
    })
}

fn update_message_revision_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    payload: &ChatOperationPayload,
    record: &ChatOperationRecord,
    payload_bytes: &[u8],
) -> loom_core::Result<()> {
    let message_id = match payload {
        ChatOperationPayload::MessageCreated { message_id, .. }
        | ChatOperationPayload::MessageEdited { message_id, .. }
        | ChatOperationPayload::MessageRedacted { message_id, .. } => message_id,
        _ => return Ok(()),
    };
    let index_path = revision_index_path(workspace_id)?;
    let index = match loom.read_file_reserved(workspace, &index_path) {
        Ok(bytes) => RevisionIndex::decode(&bytes)?,
        Err(err) if err.code == Code::NotFound => RevisionIndex::new(),
        Err(err) => return Err(err),
    };
    let envelope = OperationEnvelope::decode(&record.envelope)?;
    let entity_id = format!("chat:{channel_id}:message:{message_id}");
    let expected_latest_revision = index
        .latest(&entity_id)
        .map(|entry| entry.revision)
        .unwrap_or(0);
    let mut state = ProfileTransactionState::new(record.root_after, index);
    let update = ProfileRevisionUpdate::new(
        entity_id,
        record.operation_id.clone(),
        BodyRef::new(
            Digest::hash(loom.store().digest_algo(), payload_bytes),
            payload_bytes.len() as u64,
            "application/vnd.uldren.loom.chat.operation+cbor",
        )?,
        envelope.timestamp_ms,
        format!("{channel_id}:{message_id}:{}", record.sequence),
        Some(expected_latest_revision),
    )?;
    state.apply(ProfileTransaction::new(
        workspace_id,
        None,
        record.root_after,
        vec![update],
    )?)?;
    let index = state.into_revision_index();
    loom.create_directory_reserved(workspace, REVISION_INDEX_DIR, true)?;
    loom.write_file_reserved(workspace, &index_path, &index.encode()?, 0o100644)
}

fn append_record(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    channel_id: &str,
    record: &ChatOperationRecord,
) -> loom_core::Result<()> {
    let seq = loom.stream_append(
        workspace,
        &chat_stream_name(workspace_id, channel_id)?,
        &record.encode()?,
    )?;
    let expected = usize::try_from(record.sequence.saturating_sub(1))
        .map_err(|_| LoomError::invalid("chat sequence is too large"))?;
    if seq != expected {
        return Err(LoomError::new(
            Code::Conflict,
            "chat stream sequence does not match operation sequence",
        ));
    }
    Ok(())
}

struct MessageRefUpdate<'a> {
    loom: &'a mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &'a str,
    channel_id: &'a str,
    message_id: &'a str,
    operation_id: &'a str,
    source_root: Digest,
    body: &'a [u8],
    now_ms: u64,
}

fn update_message_refs(update: MessageRefUpdate<'_>) -> loom_core::Result<()> {
    let workspace = update.workspace;
    let workspace_id = update.workspace_id;
    let source = ReferenceSource::new(
        "chat",
        format!("{}:{}", workspace_id, update.channel_id),
        update.message_id,
        "body",
    )?;
    let mut index = match loom_reference::load_index(update.loom, workspace)? {
        Some(index) => index,
        None => loom_substrate::refs::ReferenceIndex::new(),
    };
    index = loom_reference::update_markdown_references(
        update.loom,
        index,
        loom_reference::MarkdownReferenceUpdate {
            workspace,
            source,
            operation_id: update.operation_id,
            source_root: update.source_root,
            body: update.body,
            now_ms: update.now_ms,
            relation: "refers_to",
        },
        |loom, candidate| resolve_reference_candidate(loom, workspace, workspace_id, candidate),
    )?;
    loom_reference::save_index(update.loom, workspace, &index)
}

fn resolve_reference_candidate(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    candidate: &loom_substrate::refs::MarkdownReferenceCandidate,
) -> loom_core::Result<Option<EntityRef>> {
    match candidate.kind {
        MarkdownReferenceKind::Typed => {
            let Some(target_text) = candidate.text.strip_prefix('!') else {
                return Ok(None);
            };
            if let Some(key) = target_text.strip_prefix("ticket:") {
                let Ok(key) = loom_tickets::TicketKey::parse(key) else {
                    return Ok(None);
                };
                let key = key.canonical();
                let Some(profile) =
                    loom_tickets::TicketProfileReader::open(loom, workspace, workspace_id)?
                else {
                    return Ok(None);
                };
                profile
                    .resolve_ticket_key(&key)?
                    .map(|resolution| EntityRef::parse(&format!("ticket:{}", resolution.ticket_id)))
                    .transpose()
            } else {
                Ok(EntityRef::parse(target_text).ok())
            }
        }
        MarkdownReferenceKind::PrincipalHandle => loom
            .identity_store()
            .map(|identity| identity.resolve_handle(&candidate.text[1..]))
            .transpose()?
            .flatten()
            .map(|principal| EntityRef::parse(&format!("principal:{principal}")))
            .transpose(),
        MarkdownReferenceKind::ChannelHandle => {
            resolve_channel_id(loom, workspace, workspace_id, &candidate.text[1..])
                .ok()
                .map(|channel| EntityRef::parse(&format!("channel:{channel}")))
                .transpose()
        }
    }
}

fn chat_stream_name(workspace_id: &str, channel_id: &str) -> loom_core::Result<String> {
    chat_queue_stream_name(workspace_id, channel_id)
}

pub fn chat_queue_stream_name(workspace_id: &str, channel_id: &str) -> loom_core::Result<String> {
    chat_profile_operation_log_key(workspace_id, channel_id)?;
    let mut name = String::with_capacity(27 + (workspace_id.len() * 2) + (channel_id.len() * 2));
    name.push_str("profile.chat.v1.");
    push_hex_segment(&mut name, workspace_id.as_bytes());
    name.push_str(".channels.");
    push_hex_segment(&mut name, channel_id.as_bytes());
    name.push_str(".operations");
    Ok(name)
}

fn push_hex_segment(output: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
}

fn chat_consumer_id(loom: &Loom<FileStore>, workspace: WorkspaceId) -> loom_core::Result<String> {
    Ok(loom.effective_principal()?.unwrap_or(workspace).to_string())
}

fn stream_len_or_zero(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    stream: &str,
) -> loom_core::Result<usize> {
    match loom.stream_len(workspace, stream) {
        Ok(len) => Ok(len),
        Err(err) if err.code == Code::NotFound => Ok(0),
        Err(err) => Err(err),
    }
}

fn message_summary(message: ChatMessageView) -> HostedChatMessage {
    HostedChatMessage {
        message_id: message.message_id,
        thread_id: message.thread_id,
        body: message.body,
        author_principal: message.author_principal.to_string(),
        created_at_ms: message.created_at_ms,
        updated_at_ms: message.updated_at_ms,
        redacted: message.redacted,
        reactions: message
            .reactions
            .into_iter()
            .map(reaction_summary)
            .collect(),
    }
}

fn reaction_summary(reaction: ChatReactionSummary) -> HostedChatReaction {
    HostedChatReaction {
        kind: reaction.kind,
        principal: reaction.principal.to_string(),
    }
}

fn ensure_reaction_kind(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    kind: &str,
) -> loom_core::Result<()> {
    let registry = load_emoji_registry(loom, workspace, workspace_id)?;
    if registry.contains(kind) {
        Ok(())
    } else {
        Err(LoomError::new(
            Code::InvalidArgument,
            "chat reaction kind is not registered",
        ))
    }
}

fn load_emoji_registry(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> loom_core::Result<EmojiRegistry> {
    let path = emoji_registry_path(workspace_id)?;
    match loom.read_file_reserved(workspace, &path) {
        Ok(bytes) => EmojiRegistry::decode(&bytes),
        Err(error) if error.code == Code::NotFound => Ok(EmojiRegistry::default()),
        Err(error) => Err(error),
    }
}

fn save_emoji_registry(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    registry: &EmojiRegistry,
) -> loom_core::Result<()> {
    let path = emoji_registry_path(workspace_id)?;
    loom.create_directory_reserved(workspace, EMOJI_REGISTRY_DIR, true)?;
    loom.write_file_reserved(workspace, &path, &registry.encode()?, 0o100644)
}

fn emoji_registry_summary(
    workspace_id: &str,
    registry: &EmojiRegistry,
) -> loom_core::Result<HostedChatEmojiRegistry> {
    Ok(HostedChatEmojiRegistry {
        workspace_id: workspace_id.to_string(),
        custom: registry.custom().map(str::to_string).collect(),
    })
}

fn thread_summary(thread: ChatThread) -> HostedChatThread {
    HostedChatThread {
        thread_id: thread.thread_id,
        parent_message_id: thread.parent_message_id,
        created_at_ms: thread.created_at_ms,
    }
}

fn task_summary(task: ChatTask) -> HostedChatTask {
    HostedChatTask {
        task_id: task.task_id,
        message_id: task.message_id,
        title: task.title,
        created_by: task.created_by.to_string(),
        created_at_ms: task.created_at_ms,
        state: match task.state {
            ChatTaskState::Open => HostedChatTaskState::Open,
            ChatTaskState::Claimed {
                claim_id,
                claimant_principal,
                claimed_by,
                claimed_at_ms,
                lease_token,
            } => HostedChatTaskState::Claimed {
                claim_id,
                claimant_principal: claimant_principal.to_string(),
                claimed_by: claimed_by.to_string(),
                claimed_at_ms,
                lease_token,
            },
            ChatTaskState::Completed {
                claim_id,
                completed_by,
                completed_principal,
                completed_at_ms,
                result_message_id,
            } => HostedChatTaskState::Completed {
                claim_id,
                completed_by: completed_by.to_string(),
                completed_principal: completed_principal.to_string(),
                completed_at_ms,
                result_message_id,
            },
        },
    }
}

fn agent_invocation_summary(invocation: ChatAgentInvocation) -> HostedChatAgentInvocation {
    HostedChatAgentInvocation {
        invocation_id: invocation.invocation_id,
        agent_principal: invocation.agent_principal.to_string(),
        requested_by: invocation.requested_by.to_string(),
        requested_at_ms: invocation.requested_at_ms,
        source_message_ids: invocation.source_message_ids,
        prompt: invocation.prompt,
        reply_message_ids: invocation.reply_message_ids,
    }
}

fn handoff_summary(handoff: ChatHandoffRequest) -> HostedChatHandoff {
    HostedChatHandoff {
        handoff_id: handoff.handoff_id,
        from_agent_principal: handoff.from_agent_principal.to_string(),
        to_principal: handoff.to_principal.map(|principal| principal.to_string()),
        requested_by: handoff.requested_by.to_string(),
        requested_at_ms: handoff.requested_at_ms,
        reason: handoff.reason,
    }
}

pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return 0;
    };
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}
