//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use serde::Serialize;

#[derive(Serialize)]
struct ChatOperationEventJson {
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
struct ChatOperationBatchJson {
    events: Vec<ChatOperationEventJson>,
    next: String,
}

unsafe fn optional_str_arg<'a>(value: *const c_char, what: &str) -> LoomResult<Option<&'a str>> {
    if value.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|_| LoomError::invalid(format!("{what}: invalid UTF-8")))?;
    Ok((!value.is_empty()).then_some(value))
}

fn json_string<T: Serialize>(value: &T) -> LoomResult<String> {
    serde_json::to_string(value).map_err(|err| LoomError::invalid(err.to_string()))
}

fn parse_string_list(value: &str, what: &str) -> LoomResult<Vec<String>> {
    serde_json::from_str(value).map_err(|err| LoomError::invalid(format!("{what}: {err}")))
}

fn parse_workspace_id(value: &str, what: &str) -> LoomResult<WorkspaceId> {
    WorkspaceId::parse(value).map_err(|err| LoomError::invalid(format!("{what}: {}", err.message)))
}

fn chat_read_loom<T>(
    h: &LoomSession,
    workspace: &str,
    f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> LoomResult<T>,
) -> LoomResult<T> {
    let loom = open_h_read(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    f(&loom, ns)
}

fn chat_write_loom<T>(
    h: &LoomSession,
    workspace: &str,
    f: impl FnOnce(&mut Loom<FileStore>, WorkspaceId) -> LoomResult<T>,
) -> LoomResult<T> {
    let mut loom = open_h_write(h)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let result = f(&mut loom, ns)?;
    save_loom(&mut loom)?;
    Ok(result)
}

fn json_result<T: Serialize>(result: LoomResult<T>) -> LoomResult<String> {
    json_string(&result?)
}

fn operation_batch_json(
    result: LoomResult<loom_substrate::changes::OperationChangeBatch>,
) -> LoomResult<String> {
    let batch = result?;
    json_string(&ChatOperationBatchJson {
        events: batch
            .events
            .into_iter()
            .map(|event| ChatOperationEventJson {
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
    })
}

macro_rules! out_json {
    ($out:ident, $result:expr) => {
        match $result {
            Ok(s) => unsafe { ok_str($out, &s) },
            Err(e) => fail(e),
        }
    };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_create_channel_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    channel_handle: *const c_char,
    name: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_create_channel_json");
    let workspace = arg_str!(workspace, "loom_chat_create_channel_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_create_channel_json");
    let channel_id = arg_str!(channel_id, "loom_chat_create_channel_json");
    let channel_handle = arg_str!(channel_handle, "loom_chat_create_channel_json");
    let name = arg_str!(name, "loom_chat_create_channel_json");
    let channel_id = match parse_workspace_id(channel_id, "loom_chat_create_channel_json") {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::ensure_channel(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                channel_handle,
                name,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_rename_channel_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    selector: *const c_char,
    channel_handle: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_rename_channel_json");
    let workspace = arg_str!(workspace, "loom_chat_rename_channel_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_rename_channel_json");
    let selector = arg_str!(selector, "loom_chat_rename_channel_json");
    let channel_handle = arg_str!(channel_handle, "loom_chat_rename_channel_json");
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::rename_channel(
                loom,
                ns,
                chat_workspace_id,
                selector,
                channel_handle,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_list_channels_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_list_channels_json");
    let workspace = arg_str!(workspace, "loom_chat_list_channels_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_list_channels_json");
    out_json!(
        out,
        chat_read_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::list_channels(loom, ns, chat_workspace_id))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_post_message_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    message_id: *const c_char,
    thread_id: *const c_char,
    body_text: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_post_message_json");
    let workspace = arg_str!(workspace, "loom_chat_post_message_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_post_message_json");
    let channel_id = arg_str!(channel_id, "loom_chat_post_message_json");
    let message_id = arg_str!(message_id, "loom_chat_post_message_json");
    let thread_id = match unsafe { optional_str_arg(thread_id, "loom_chat_post_message_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let body = arg_str!(body_text, "loom_chat_post_message_json")
        .as_bytes()
        .to_vec();
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::post_message(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                message_id,
                thread_id,
                body,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_edit_message_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    message_id: *const c_char,
    body_text: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_edit_message_json");
    let workspace = arg_str!(workspace, "loom_chat_edit_message_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_edit_message_json");
    let channel_id = arg_str!(channel_id, "loom_chat_edit_message_json");
    let message_id = arg_str!(message_id, "loom_chat_edit_message_json");
    let body = arg_str!(body_text, "loom_chat_edit_message_json")
        .as_bytes()
        .to_vec();
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::edit_message(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                message_id,
                body,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_redact_message_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    message_id: *const c_char,
    reason: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_redact_message_json");
    let workspace = arg_str!(workspace, "loom_chat_redact_message_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_redact_message_json");
    let channel_id = arg_str!(channel_id, "loom_chat_redact_message_json");
    let message_id = arg_str!(message_id, "loom_chat_redact_message_json");
    let reason = match unsafe { optional_str_arg(reason, "loom_chat_redact_message_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::redact_message(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                message_id,
                reason,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_create_thread_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    thread_id: *const c_char,
    parent_message_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_create_thread_json");
    let workspace = arg_str!(workspace, "loom_chat_create_thread_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_create_thread_json");
    let channel_id = arg_str!(channel_id, "loom_chat_create_thread_json");
    let thread_id = arg_str!(thread_id, "loom_chat_create_thread_json");
    let parent_message_id = arg_str!(parent_message_id, "loom_chat_create_thread_json");
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::create_thread(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                thread_id,
                parent_message_id,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_create_task_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    task_id: *const c_char,
    message_id: *const c_char,
    title: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_create_task_json");
    let workspace = arg_str!(workspace, "loom_chat_create_task_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_create_task_json");
    let channel_id = arg_str!(channel_id, "loom_chat_create_task_json");
    let task_id = arg_str!(task_id, "loom_chat_create_task_json");
    let message_id = match unsafe { optional_str_arg(message_id, "loom_chat_create_task_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let title = arg_str!(title, "loom_chat_create_task_json");
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::create_task(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                task_id,
                message_id,
                title,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_claim_task_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    task_id: *const c_char,
    claim_id: *const c_char,
    lease_token: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_claim_task_json");
    let workspace = arg_str!(workspace, "loom_chat_claim_task_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_claim_task_json");
    let channel_id = arg_str!(channel_id, "loom_chat_claim_task_json");
    let task_id = arg_str!(task_id, "loom_chat_claim_task_json");
    let claim_id = arg_str!(claim_id, "loom_chat_claim_task_json");
    let lease_token = match unsafe { optional_str_arg(lease_token, "loom_chat_claim_task_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::claim_task(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                task_id,
                claim_id,
                lease_token,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_complete_task_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    task_id: *const c_char,
    claim_id: *const c_char,
    result_message_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_complete_task_json");
    let workspace = arg_str!(workspace, "loom_chat_complete_task_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_complete_task_json");
    let channel_id = arg_str!(channel_id, "loom_chat_complete_task_json");
    let task_id = arg_str!(task_id, "loom_chat_complete_task_json");
    let claim_id = arg_str!(claim_id, "loom_chat_complete_task_json");
    let result_message_id =
        match unsafe { optional_str_arg(result_message_id, "loom_chat_complete_task_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::complete_task(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                task_id,
                claim_id,
                result_message_id,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_invoke_agent_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    invocation_id: *const c_char,
    agent_principal: *const c_char,
    source_message_ids_json: *const c_char,
    prompt_text: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_invoke_agent_json");
    let workspace = arg_str!(workspace, "loom_chat_invoke_agent_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_invoke_agent_json");
    let channel_id = arg_str!(channel_id, "loom_chat_invoke_agent_json");
    let invocation_id = arg_str!(invocation_id, "loom_chat_invoke_agent_json");
    let agent_principal = arg_str!(agent_principal, "loom_chat_invoke_agent_json");
    let agent_principal = match parse_workspace_id(agent_principal, "loom_chat_invoke_agent_json") {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let source_message_ids_json = arg_str!(source_message_ids_json, "loom_chat_invoke_agent_json");
    let source_message_ids =
        match parse_string_list(source_message_ids_json, "loom_chat_invoke_agent_json") {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let prompt = arg_str!(prompt_text, "loom_chat_invoke_agent_json")
        .as_bytes()
        .to_vec();
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::invoke_agent(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                invocation_id,
                agent_principal,
                source_message_ids,
                prompt,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_agent_reply_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    invocation_id: *const c_char,
    message_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_agent_reply_json");
    let workspace = arg_str!(workspace, "loom_chat_agent_reply_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_agent_reply_json");
    let channel_id = arg_str!(channel_id, "loom_chat_agent_reply_json");
    let invocation_id = arg_str!(invocation_id, "loom_chat_agent_reply_json");
    let message_id = arg_str!(message_id, "loom_chat_agent_reply_json");
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::agent_reply(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                invocation_id,
                message_id,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_request_handoff_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    handoff_id: *const c_char,
    from_agent_principal: *const c_char,
    to_principal: *const c_char,
    reason: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_request_handoff_json");
    let workspace = arg_str!(workspace, "loom_chat_request_handoff_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_request_handoff_json");
    let channel_id = arg_str!(channel_id, "loom_chat_request_handoff_json");
    let handoff_id = arg_str!(handoff_id, "loom_chat_request_handoff_json");
    let from_agent_principal = arg_str!(from_agent_principal, "loom_chat_request_handoff_json");
    let from_agent_principal =
        match parse_workspace_id(from_agent_principal, "loom_chat_request_handoff_json") {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let to_principal =
        match unsafe { optional_str_arg(to_principal, "loom_chat_request_handoff_json") } {
            Ok(Some(value)) => match parse_workspace_id(value, "loom_chat_request_handoff_json") {
                Ok(value) => Some(value),
                Err(e) => return fail(e),
            },
            Ok(None) => None,
            Err(e) => return fail(e),
        };
    let reason = match unsafe { optional_str_arg(reason, "loom_chat_request_handoff_json") } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::request_handoff(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                handoff_id,
                from_agent_principal,
                to_principal,
                reason,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_add_reaction_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    message_id: *const c_char,
    kind: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_add_reaction_json");
    let workspace = arg_str!(workspace, "loom_chat_add_reaction_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_add_reaction_json");
    let channel_id = arg_str!(channel_id, "loom_chat_add_reaction_json");
    let message_id = arg_str!(message_id, "loom_chat_add_reaction_json");
    let kind = arg_str!(kind, "loom_chat_add_reaction_json");
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::add_reaction(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                message_id,
                kind,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_remove_reaction_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    message_id: *const c_char,
    kind: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_remove_reaction_json");
    let workspace = arg_str!(workspace, "loom_chat_remove_reaction_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_remove_reaction_json");
    let channel_id = arg_str!(channel_id, "loom_chat_remove_reaction_json");
    let message_id = arg_str!(message_id, "loom_chat_remove_reaction_json");
    let kind = arg_str!(kind, "loom_chat_remove_reaction_json");
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::remove_reaction(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                message_id,
                kind,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_emoji_list_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_emoji_list_json");
    let workspace = arg_str!(workspace, "loom_chat_emoji_list_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_emoji_list_json");
    out_json!(
        out,
        chat_read_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::emoji_registry(loom, ns, chat_workspace_id))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_emoji_register_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    kind: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_emoji_register_json");
    let workspace = arg_str!(workspace, "loom_chat_emoji_register_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_emoji_register_json");
    let kind = arg_str!(kind, "loom_chat_emoji_register_json");
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::register_emoji(loom, ns, chat_workspace_id, kind))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_emoji_unregister_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    kind: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_emoji_unregister_json");
    let workspace = arg_str!(workspace, "loom_chat_emoji_unregister_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_emoji_unregister_json");
    let kind = arg_str!(kind, "loom_chat_emoji_unregister_json");
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::unregister_emoji(
                loom,
                ns,
                chat_workspace_id,
                kind,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_messages_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_messages_json");
    let workspace = arg_str!(workspace, "loom_chat_messages_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_messages_json");
    let channel_id = arg_str!(channel_id, "loom_chat_messages_json");
    out_json!(
        out,
        chat_read_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::channel_projection(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_cursor_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_cursor_json");
    let workspace = arg_str!(workspace, "loom_chat_cursor_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_cursor_json");
    let channel_id = arg_str!(channel_id, "loom_chat_cursor_json");
    out_json!(
        out,
        chat_read_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::read_cursor(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_update_cursor_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    next_sequence: u64,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_update_cursor_json");
    let workspace = arg_str!(workspace, "loom_chat_update_cursor_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_update_cursor_json");
    let channel_id = arg_str!(channel_id, "loom_chat_update_cursor_json");
    out_json!(
        out,
        chat_write_loom(h, workspace, |loom, ns| {
            json_result(loom_chat::update_cursor(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                next_sequence,
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_fetch_events_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    from_sequence: u64,
    max: usize,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    let h = handle_ref!(handle, "loom_chat_fetch_events_json");
    let workspace = arg_str!(workspace, "loom_chat_fetch_events_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_fetch_events_json");
    let channel_id = arg_str!(channel_id, "loom_chat_fetch_events_json");
    out_json!(
        out,
        chat_read_loom(h, workspace, |loom, ns| {
            operation_batch_json(loom_chat::operation_changes(
                loom,
                ns,
                chat_workspace_id,
                channel_id,
                from_sequence,
                max,
            ))
        })
    )
}
