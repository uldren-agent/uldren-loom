//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use serde::Serialize;

#[derive(Serialize)]
struct OperationEventJson {
    workspace_id: String,
    app_id: String,
    scope_id: String,
    operation_id: String,
    operation_kind: String,
    sequence: u64,
    actor_principal: String,
    timestamp_ms: u64,
    root_after: String,
    payload_digest: String,
    policy_labels: Vec<String>,
}

#[derive(Serialize)]
struct OperationBatchJson {
    events: Vec<OperationEventJson>,
    next: String,
}

fn to_json<T: Serialize>(value: loom_core::error::Result<T>) -> napi::Result<String> {
    let value = value.map_err(reason)?;
    serde_json::to_string(&value).map_err(|error| napi::Error::from_reason(error.to_string()))
}

fn operation_batch_json(
    value: loom_core::error::Result<loom_substrate::changes::OperationChangeBatch>,
) -> napi::Result<String> {
    let batch = value.map_err(reason)?;
    to_json(Ok(OperationBatchJson {
        events: batch
            .events
            .into_iter()
            .map(|event| OperationEventJson {
                workspace_id: event.workspace_id,
                app_id: event.app_id,
                scope_id: event.scope_id,
                operation_id: event.operation_id,
                operation_kind: event.operation_kind,
                sequence: event.sequence,
                actor_principal: event.actor_principal,
                timestamp_ms: event.timestamp_ms,
                root_after: event.root_after.to_string(),
                payload_digest: event.payload_digest.to_string(),
                policy_labels: event.policy_labels,
            })
            .collect(),
        next: batch.next.encode(),
    }))
}

fn parse_workspace_id(value: &str) -> napi::Result<WorkspaceId> {
    WorkspaceId::parse(value).map_err(reason)
}

fn parse_string_list(value: &str) -> napi::Result<Vec<String>> {
    serde_json::from_str(value).map_err(|error| napi::Error::from_reason(error.to_string()))
}

fn chat_read<T>(
    loom_path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> napi::Result<T>,
) -> napi::Result<T> {
    let loom = open_loom_read_unlocked(loom_path, key_spec(passphrase).as_ref()).map_err(reason)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    f(&loom, workspace_id)
}

fn chat_write<T>(
    loom_path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    f: impl FnOnce(&mut Loom<FileStore>, WorkspaceId) -> napi::Result<T>,
) -> napi::Result<T> {
    let mut loom = open_loom_unlocked(loom_path, key_spec(passphrase).as_ref()).map_err(reason)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    let out = f(&mut loom, workspace_id)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(out)
}

#[napi]
pub fn chat_create_channel_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    channel_handle: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let channel_id = parse_workspace_id(&channel_id)?;
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::ensure_channel(
            loom,
            ns,
            &chat_workspace_id,
            channel_id,
            &channel_handle,
            &name,
        ))
    })
}

#[napi]
pub fn chat_rename_channel_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    selector: String,
    channel_handle: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::rename_channel(
            loom,
            ns,
            &chat_workspace_id,
            &selector,
            &channel_handle,
        ))
    })
}

#[napi]
pub fn chat_list_channels_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::list_channels(loom, ns, &chat_workspace_id))
    })
}

#[napi]
pub fn chat_post_message_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    message_id: String,
    thread_id: Option<String>,
    body_text: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::post_message(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &message_id,
            thread_id.as_deref(),
            body_text.into_bytes(),
        ))
    })
}

#[napi]
pub fn chat_edit_message_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    message_id: String,
    body_text: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::edit_message(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &message_id,
            body_text.into_bytes(),
        ))
    })
}

#[napi]
pub fn chat_redact_message_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    message_id: String,
    reason_text: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::redact_message(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &message_id,
            reason_text.as_deref(),
        ))
    })
}

#[napi]
pub fn chat_create_thread_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    thread_id: String,
    parent_message_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::create_thread(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &thread_id,
            &parent_message_id,
        ))
    })
}

#[napi]
pub fn chat_create_task_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    task_id: String,
    message_id: Option<String>,
    title: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::create_task(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &task_id,
            message_id.as_deref(),
            &title,
        ))
    })
}

#[napi]
pub fn chat_claim_task_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    task_id: String,
    claim_id: String,
    lease_token: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::claim_task(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &task_id,
            &claim_id,
            lease_token.as_deref(),
        ))
    })
}

#[napi]
pub fn chat_complete_task_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    task_id: String,
    claim_id: String,
    result_message_id: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::complete_task(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &task_id,
            &claim_id,
            result_message_id.as_deref(),
        ))
    })
}

#[napi]
pub fn chat_invoke_agent_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    invocation_id: String,
    agent_principal: String,
    source_message_ids_json: String,
    prompt_text: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let agent_principal = parse_workspace_id(&agent_principal)?;
    let source_message_ids = parse_string_list(&source_message_ids_json)?;
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::invoke_agent(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &invocation_id,
            agent_principal,
            source_message_ids,
            prompt_text.into_bytes(),
        ))
    })
}

#[napi]
pub fn chat_agent_reply_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    invocation_id: String,
    message_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::agent_reply(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &invocation_id,
            &message_id,
        ))
    })
}

#[napi]
pub fn chat_request_handoff_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    handoff_id: String,
    from_agent_principal: String,
    to_principal: Option<String>,
    reason_text: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let from_agent_principal = parse_workspace_id(&from_agent_principal)?;
    let to_principal = to_principal
        .as_deref()
        .map(parse_workspace_id)
        .transpose()?;
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::request_handoff(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &handoff_id,
            from_agent_principal,
            to_principal,
            reason_text.as_deref(),
        ))
    })
}

#[napi]
pub fn chat_add_reaction_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    message_id: String,
    kind: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::add_reaction(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &message_id,
            &kind,
        ))
    })
}

#[napi]
pub fn chat_remove_reaction_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    message_id: String,
    kind: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::remove_reaction(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &message_id,
            &kind,
        ))
    })
}

#[napi]
pub fn chat_emoji_list_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::emoji_registry(loom, ns, &chat_workspace_id))
    })
}

#[napi]
pub fn chat_emoji_register_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    kind: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::register_emoji(
            loom,
            ns,
            &chat_workspace_id,
            &kind,
        ))
    })
}

#[napi]
pub fn chat_emoji_unregister_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    kind: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::unregister_emoji(
            loom,
            ns,
            &chat_workspace_id,
            &kind,
        ))
    })
}

#[napi]
pub fn chat_messages_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::channel_projection(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
        ))
    })
}

#[napi]
pub fn chat_cursor_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    chat_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::read_cursor(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
        ))
    })
}

#[napi]
pub fn chat_update_cursor_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    next_sequence: BigInt,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let next_sequence = bigint_to_u64(next_sequence, "nextSequence")?;
    chat_write(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        to_json(loom_chat::update_cursor(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            next_sequence,
        ))
    })
}

#[napi]
pub fn chat_fetch_events_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    from_sequence: BigInt,
    max: u32,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let from_sequence = bigint_to_u64(from_sequence, "fromSequence")?;
    chat_read(&loom_path, &workspace, passphrase.as_deref(), |loom, ns| {
        operation_batch_json(loom_chat::operation_changes(
            loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            from_sequence,
            max as usize,
        ))
    })
}
