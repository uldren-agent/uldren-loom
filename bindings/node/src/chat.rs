//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use futures::executor::block_on;
use loom_client::generated_api::Chat as GeneratedChat;

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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_create_channel_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            channel_handle,
            name,
            None,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_rename_channel_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            selector,
            channel_handle,
            None,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn chat_list_channels_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_list_channels_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_post_message_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            message_id,
            thread_id,
            body_text,
            None,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn chat_post_message_bytes_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    message_id: String,
    thread_id: Option<String>,
    body: Uint8Array,
    expected_entity_tag: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_post_message_bytes_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            message_id,
            thread_id,
            body.to_vec(),
            expected_entity_tag,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_edit_message_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            message_id,
            body_text,
            None,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn chat_edit_message_bytes_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    message_id: String,
    body: Uint8Array,
    expected_entity_tag: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_edit_message_bytes_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            message_id,
            body.to_vec(),
            expected_entity_tag,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_redact_message_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            message_id,
            reason_text,
            None,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_create_thread_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            thread_id,
            parent_message_id,
            None,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_create_task_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            task_id,
            message_id,
            title,
            None,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_claim_task_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            task_id,
            claim_id,
            lease_token,
            None,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_complete_task_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            task_id,
            claim_id,
            result_message_id,
            None,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_invoke_agent_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            invocation_id,
            agent_principal,
            source_message_ids_json,
            prompt_text,
            None,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn chat_invoke_agent_bytes_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    invocation_id: String,
    agent_principal: String,
    source_message_ids_json: String,
    prompt: Uint8Array,
    expected_entity_tag: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_invoke_agent_bytes_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            invocation_id,
            agent_principal,
            source_message_ids_json,
            prompt.to_vec(),
            expected_entity_tag,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_agent_reply_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            invocation_id,
            message_id,
            None,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_request_handoff_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            handoff_id,
            from_agent_principal,
            to_principal,
            reason_text,
            None,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn chat_add_reaction_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    message_id: String,
    kind: String,
    expected_entity_tag: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_add_reaction_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            message_id,
            kind,
            expected_entity_tag,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn chat_remove_reaction_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    message_id: String,
    kind: String,
    expected_entity_tag: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_remove_reaction_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            message_id,
            kind,
            expected_entity_tag,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn chat_emoji_list_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_emoji_list_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn chat_emoji_register_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    kind: String,
    expected_entity_tag: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_emoji_register_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            kind,
            expected_entity_tag,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn chat_emoji_unregister_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    kind: String,
    expected_entity_tag: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_emoji_unregister_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            kind,
            expected_entity_tag,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn chat_messages_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_messages_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn chat_cursor_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_cursor_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
        ),
    )
    .map_err(reason)
}

#[napi]
pub fn chat_update_cursor_json(
    loom_path: String,
    workspace: String,
    chat_workspace_id: String,
    channel_id: String,
    next_sequence: BigInt,
    expected_entity_tag: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let next_sequence = bigint_to_u64(next_sequence, "nextSequence")?;
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_update_cursor_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            next_sequence,
            expected_entity_tag,
        ),
    )
    .map_err(reason)
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
    let generated =
        generated_session::open_generated_session(&loom_path, passphrase.as_deref(), None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_fetch_events_json(
            &generated.client,
            generated.session.clone(),
            workspace,
            chat_workspace_id,
            channel_id,
            from_sequence,
            u64::from(max),
        ),
    )
    .map_err(reason)
}
