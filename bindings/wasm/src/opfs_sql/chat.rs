use loom_core::workspace::WorkspaceId;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use super::{LoomStore, le, resolve_workspace_arg, save_loom};

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

fn to_json<T: Serialize>(value: loom_core::Result<T>) -> Result<String, JsError> {
    let value = value.map_err(le)?;
    serde_json::to_string(&value).map_err(|error| JsError::new(&error.to_string()))
}

fn operation_batch_json(
    value: loom_core::Result<loom_substrate::changes::OperationChangeBatch>,
) -> Result<String, JsError> {
    let batch = value.map_err(le)?;
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

fn parse_workspace_id(value: &str) -> Result<WorkspaceId, JsError> {
    WorkspaceId::parse(value).map_err(le)
}

fn parse_string_list(value: &str) -> Result<Vec<String>, JsError> {
    serde_json::from_str(value).map_err(|error| JsError::new(&error.to_string()))
}

#[wasm_bindgen]
impl LoomStore {
    pub fn chat_create_channel_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        channel_handle: String,
        name: String,
    ) -> Result<String, JsError> {
        let channel_id = parse_workspace_id(&channel_id)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::ensure_channel(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            channel_id,
            &channel_handle,
            &name,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_rename_channel_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        selector: String,
        channel_handle: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::rename_channel(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &selector,
            &channel_handle,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_list_channels_json(
        &self,
        workspace: String,
        chat_workspace_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_chat::list_channels(
            &self.loom,
            ns,
            &chat_workspace_id,
        ))
    }

    pub fn chat_post_message_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        message_id: String,
        thread_id: Option<String>,
        body_text: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::post_message(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &message_id,
            thread_id.as_deref(),
            body_text.into_bytes(),
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_edit_message_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        message_id: String,
        body_text: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::edit_message(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &message_id,
            body_text.into_bytes(),
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_redact_message_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        message_id: String,
        reason_text: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::redact_message(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &message_id,
            reason_text.as_deref(),
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_create_thread_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        thread_id: String,
        parent_message_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::create_thread(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &thread_id,
            &parent_message_id,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_create_task_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        task_id: String,
        message_id: Option<String>,
        title: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::create_task(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &task_id,
            message_id.as_deref(),
            &title,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_claim_task_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        task_id: String,
        claim_id: String,
        lease_token: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::claim_task(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &task_id,
            &claim_id,
            lease_token.as_deref(),
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_complete_task_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        task_id: String,
        claim_id: String,
        result_message_id: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::complete_task(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &task_id,
            &claim_id,
            result_message_id.as_deref(),
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_invoke_agent_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        invocation_id: String,
        agent_principal: String,
        source_message_ids_json: String,
        prompt_text: String,
    ) -> Result<String, JsError> {
        let agent_principal = parse_workspace_id(&agent_principal)?;
        let source_message_ids = parse_string_list(&source_message_ids_json)?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::invoke_agent(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &invocation_id,
            agent_principal,
            source_message_ids,
            prompt_text.into_bytes(),
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_agent_reply_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        invocation_id: String,
        message_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::agent_reply(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &invocation_id,
            &message_id,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_request_handoff_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        handoff_id: String,
        from_agent_principal: String,
        to_principal: Option<String>,
        reason_text: Option<String>,
    ) -> Result<String, JsError> {
        let from_agent_principal = parse_workspace_id(&from_agent_principal)?;
        let to_principal = to_principal
            .as_deref()
            .map(parse_workspace_id)
            .transpose()?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::request_handoff(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &handoff_id,
            from_agent_principal,
            to_principal,
            reason_text.as_deref(),
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_add_reaction_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        message_id: String,
        kind: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::add_reaction(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &message_id,
            &kind,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_remove_reaction_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        message_id: String,
        kind: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::remove_reaction(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            &message_id,
            &kind,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_emoji_list_json(
        &self,
        workspace: String,
        chat_workspace_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_chat::emoji_registry(
            &self.loom,
            ns,
            &chat_workspace_id,
        ))
    }

    pub fn chat_emoji_register_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        kind: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::register_emoji(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &kind,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_emoji_unregister_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        kind: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::unregister_emoji(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &kind,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_messages_json(
        &self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_chat::channel_projection(
            &self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
        ))
    }

    pub fn chat_cursor_json(
        &self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        to_json(loom_chat::read_cursor(
            &self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
        ))
    }

    pub fn chat_update_cursor_json(
        &mut self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        next_sequence: u64,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let out = to_json(loom_chat::update_cursor(
            &mut self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            next_sequence,
        ))?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }

    pub fn chat_fetch_events_json(
        &self,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        from_sequence: u64,
        max: u32,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        operation_batch_json(loom_chat::operation_changes(
            &self.loom,
            ns,
            &chat_workspace_id,
            &channel_id,
            from_sequence,
            max as usize,
        ))
    }
}
