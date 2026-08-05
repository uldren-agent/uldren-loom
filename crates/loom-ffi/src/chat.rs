//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use loom_client::generated_api::Chat;

unsafe fn optional_str_arg_generated<'a>(
    value: *const c_char,
    what: &str,
) -> LoomResult<Option<&'a str>> {
    if value.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|_| LoomError::invalid(format!("{what}: invalid UTF-8")))?;
    Ok(Some(value))
}

macro_rules! out_json {
    ($out:ident, $result:expr) => {
        match $result {
            Ok(s) => unsafe { ok_str($out, &s) },
            Err(e) => fail(e),
        }
    };
}

macro_rules! require_json_out {
    ($out:ident, $what:literal) => {
        if $out.is_null() {
            return fail_arg(concat!($what, ": null out"));
        }
    };
}

fn chat_generated_string(
    h: &LoomSession,
    f: impl FnOnce(&loom_client::LocalLoomClient, loom_client::types::LoomSession) -> LoomResult<String>,
) -> LoomResult<String> {
    with_generated_client(h, f)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_create_channel_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    channel_handle: *const c_char,
    name: *const c_char,
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_create_channel_json");
    let h = handle_ref!(handle, "loom_chat_create_channel_json");
    let workspace = arg_str!(workspace, "loom_chat_create_channel_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_create_channel_json");
    let channel_id = arg_str!(channel_id, "loom_chat_create_channel_json");
    let channel_handle = arg_str!(channel_handle, "loom_chat_create_channel_json");
    let name = arg_str!(name, "loom_chat_create_channel_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_create_channel_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_create_channel_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                channel_handle.to_string(),
                name.to_string(),
                expected_entity_tag.map(str::to_string),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_rename_channel_json");
    let h = handle_ref!(handle, "loom_chat_rename_channel_json");
    let workspace = arg_str!(workspace, "loom_chat_rename_channel_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_rename_channel_json");
    let selector = arg_str!(selector, "loom_chat_rename_channel_json");
    let channel_handle = arg_str!(channel_handle, "loom_chat_rename_channel_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_rename_channel_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_rename_channel_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                selector.to_string(),
                channel_handle.to_string(),
                expected_entity_tag.map(str::to_string),
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
    require_json_out!(out, "loom_chat_list_channels_json");
    let h = handle_ref!(handle, "loom_chat_list_channels_json");
    let workspace = arg_str!(workspace, "loom_chat_list_channels_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_list_channels_json");
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_list_channels_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
            ))
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_post_message_json");
    let h = handle_ref!(handle, "loom_chat_post_message_json");
    let workspace = arg_str!(workspace, "loom_chat_post_message_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_post_message_json");
    let channel_id = arg_str!(channel_id, "loom_chat_post_message_json");
    let message_id = arg_str!(message_id, "loom_chat_post_message_json");
    let thread_id =
        match unsafe { optional_str_arg_generated(thread_id, "loom_chat_post_message_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let body_text = arg_str!(body_text, "loom_chat_post_message_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_post_message_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_post_message_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                message_id.to_string(),
                thread_id.map(str::to_string),
                body_text.to_string(),
                expected_entity_tag.map(str::to_string),
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_post_message_bytes_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    message_id: *const c_char,
    thread_id: *const c_char,
    body: *const c_uchar,
    body_len: usize,
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_post_message_bytes_json");
    let h = handle_ref!(handle, "loom_chat_post_message_bytes_json");
    let workspace = arg_str!(workspace, "loom_chat_post_message_bytes_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_post_message_bytes_json");
    let channel_id = arg_str!(channel_id, "loom_chat_post_message_bytes_json");
    let message_id = arg_str!(message_id, "loom_chat_post_message_bytes_json");
    let thread_id =
        match unsafe { optional_str_arg_generated(thread_id, "loom_chat_post_message_bytes_json") }
        {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let body = unsafe { byte_slice(body, body_len) };
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_post_message_bytes_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_post_message_bytes_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                message_id.to_string(),
                thread_id.map(str::to_string),
                body.to_vec(),
                expected_entity_tag.map(str::to_string),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_edit_message_json");
    let h = handle_ref!(handle, "loom_chat_edit_message_json");
    let workspace = arg_str!(workspace, "loom_chat_edit_message_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_edit_message_json");
    let channel_id = arg_str!(channel_id, "loom_chat_edit_message_json");
    let message_id = arg_str!(message_id, "loom_chat_edit_message_json");
    let body_text = arg_str!(body_text, "loom_chat_edit_message_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_edit_message_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_edit_message_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                message_id.to_string(),
                body_text.to_string(),
                expected_entity_tag.map(str::to_string),
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_edit_message_bytes_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    message_id: *const c_char,
    body: *const c_uchar,
    body_len: usize,
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_edit_message_bytes_json");
    let h = handle_ref!(handle, "loom_chat_edit_message_bytes_json");
    let workspace = arg_str!(workspace, "loom_chat_edit_message_bytes_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_edit_message_bytes_json");
    let channel_id = arg_str!(channel_id, "loom_chat_edit_message_bytes_json");
    let message_id = arg_str!(message_id, "loom_chat_edit_message_bytes_json");
    let body = unsafe { byte_slice(body, body_len) };
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_edit_message_bytes_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_edit_message_bytes_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                message_id.to_string(),
                body.to_vec(),
                expected_entity_tag.map(str::to_string),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_redact_message_json");
    let h = handle_ref!(handle, "loom_chat_redact_message_json");
    let workspace = arg_str!(workspace, "loom_chat_redact_message_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_redact_message_json");
    let channel_id = arg_str!(channel_id, "loom_chat_redact_message_json");
    let message_id = arg_str!(message_id, "loom_chat_redact_message_json");
    let reason =
        match unsafe { optional_str_arg_generated(reason, "loom_chat_redact_message_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_redact_message_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_redact_message_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                message_id.to_string(),
                reason.map(str::to_string),
                expected_entity_tag.map(str::to_string),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_create_thread_json");
    let h = handle_ref!(handle, "loom_chat_create_thread_json");
    let workspace = arg_str!(workspace, "loom_chat_create_thread_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_create_thread_json");
    let channel_id = arg_str!(channel_id, "loom_chat_create_thread_json");
    let thread_id = arg_str!(thread_id, "loom_chat_create_thread_json");
    let parent_message_id = arg_str!(parent_message_id, "loom_chat_create_thread_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_create_thread_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_create_thread_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                thread_id.to_string(),
                parent_message_id.to_string(),
                expected_entity_tag.map(str::to_string),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_create_task_json");
    let h = handle_ref!(handle, "loom_chat_create_task_json");
    let workspace = arg_str!(workspace, "loom_chat_create_task_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_create_task_json");
    let channel_id = arg_str!(channel_id, "loom_chat_create_task_json");
    let task_id = arg_str!(task_id, "loom_chat_create_task_json");
    let message_id =
        match unsafe { optional_str_arg_generated(message_id, "loom_chat_create_task_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let title = arg_str!(title, "loom_chat_create_task_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_create_task_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_create_task_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                task_id.to_string(),
                message_id.map(str::to_string),
                title.to_string(),
                expected_entity_tag.map(str::to_string),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_claim_task_json");
    let h = handle_ref!(handle, "loom_chat_claim_task_json");
    let workspace = arg_str!(workspace, "loom_chat_claim_task_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_claim_task_json");
    let channel_id = arg_str!(channel_id, "loom_chat_claim_task_json");
    let task_id = arg_str!(task_id, "loom_chat_claim_task_json");
    let claim_id = arg_str!(claim_id, "loom_chat_claim_task_json");
    let lease_token =
        match unsafe { optional_str_arg_generated(lease_token, "loom_chat_claim_task_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_claim_task_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_claim_task_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                task_id.to_string(),
                claim_id.to_string(),
                lease_token.map(str::to_string),
                expected_entity_tag.map(str::to_string),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_complete_task_json");
    let h = handle_ref!(handle, "loom_chat_complete_task_json");
    let workspace = arg_str!(workspace, "loom_chat_complete_task_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_complete_task_json");
    let channel_id = arg_str!(channel_id, "loom_chat_complete_task_json");
    let task_id = arg_str!(task_id, "loom_chat_complete_task_json");
    let claim_id = arg_str!(claim_id, "loom_chat_complete_task_json");
    let result_message_id = match unsafe {
        optional_str_arg_generated(result_message_id, "loom_chat_complete_task_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_complete_task_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_complete_task_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                task_id.to_string(),
                claim_id.to_string(),
                result_message_id.map(str::to_string),
                expected_entity_tag.map(str::to_string),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_invoke_agent_json");
    let h = handle_ref!(handle, "loom_chat_invoke_agent_json");
    let workspace = arg_str!(workspace, "loom_chat_invoke_agent_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_invoke_agent_json");
    let channel_id = arg_str!(channel_id, "loom_chat_invoke_agent_json");
    let invocation_id = arg_str!(invocation_id, "loom_chat_invoke_agent_json");
    let agent_principal = arg_str!(agent_principal, "loom_chat_invoke_agent_json");
    let source_message_ids_json = arg_str!(source_message_ids_json, "loom_chat_invoke_agent_json");
    let prompt_text = arg_str!(prompt_text, "loom_chat_invoke_agent_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_invoke_agent_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_invoke_agent_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                invocation_id.to_string(),
                agent_principal.to_string(),
                source_message_ids_json.to_string(),
                prompt_text.to_string(),
                expected_entity_tag.map(str::to_string),
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_invoke_agent_bytes_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    channel_id: *const c_char,
    invocation_id: *const c_char,
    agent_principal: *const c_char,
    source_message_ids_json: *const c_char,
    prompt: *const c_uchar,
    prompt_len: usize,
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_invoke_agent_bytes_json");
    let h = handle_ref!(handle, "loom_chat_invoke_agent_bytes_json");
    let workspace = arg_str!(workspace, "loom_chat_invoke_agent_bytes_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_invoke_agent_bytes_json");
    let channel_id = arg_str!(channel_id, "loom_chat_invoke_agent_bytes_json");
    let invocation_id = arg_str!(invocation_id, "loom_chat_invoke_agent_bytes_json");
    let agent_principal = arg_str!(agent_principal, "loom_chat_invoke_agent_bytes_json");
    let source_message_ids_json =
        arg_str!(source_message_ids_json, "loom_chat_invoke_agent_bytes_json");
    let prompt = unsafe { byte_slice(prompt, prompt_len) };
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_invoke_agent_bytes_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_invoke_agent_bytes_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                invocation_id.to_string(),
                agent_principal.to_string(),
                source_message_ids_json.to_string(),
                prompt.to_vec(),
                expected_entity_tag.map(str::to_string),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_agent_reply_json");
    let h = handle_ref!(handle, "loom_chat_agent_reply_json");
    let workspace = arg_str!(workspace, "loom_chat_agent_reply_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_agent_reply_json");
    let channel_id = arg_str!(channel_id, "loom_chat_agent_reply_json");
    let invocation_id = arg_str!(invocation_id, "loom_chat_agent_reply_json");
    let message_id = arg_str!(message_id, "loom_chat_agent_reply_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_agent_reply_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_agent_reply_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                invocation_id.to_string(),
                message_id.to_string(),
                expected_entity_tag.map(str::to_string),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_request_handoff_json");
    let h = handle_ref!(handle, "loom_chat_request_handoff_json");
    let workspace = arg_str!(workspace, "loom_chat_request_handoff_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_request_handoff_json");
    let channel_id = arg_str!(channel_id, "loom_chat_request_handoff_json");
    let handoff_id = arg_str!(handoff_id, "loom_chat_request_handoff_json");
    let from_agent_principal = arg_str!(from_agent_principal, "loom_chat_request_handoff_json");
    let to_principal =
        match unsafe { optional_str_arg_generated(to_principal, "loom_chat_request_handoff_json") }
        {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let reason =
        match unsafe { optional_str_arg_generated(reason, "loom_chat_request_handoff_json") } {
            Ok(value) => value,
            Err(e) => return fail(e),
        };
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_request_handoff_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_request_handoff_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                handoff_id.to_string(),
                from_agent_principal.to_string(),
                to_principal.map(str::to_string),
                reason.map(str::to_string),
                expected_entity_tag.map(str::to_string),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_add_reaction_json");
    let h = handle_ref!(handle, "loom_chat_add_reaction_json");
    let workspace = arg_str!(workspace, "loom_chat_add_reaction_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_add_reaction_json");
    let channel_id = arg_str!(channel_id, "loom_chat_add_reaction_json");
    let message_id = arg_str!(message_id, "loom_chat_add_reaction_json");
    let kind = arg_str!(kind, "loom_chat_add_reaction_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_add_reaction_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_add_reaction_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                message_id.to_string(),
                kind.to_string(),
                expected_entity_tag.map(str::to_string),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_remove_reaction_json");
    let h = handle_ref!(handle, "loom_chat_remove_reaction_json");
    let workspace = arg_str!(workspace, "loom_chat_remove_reaction_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_remove_reaction_json");
    let channel_id = arg_str!(channel_id, "loom_chat_remove_reaction_json");
    let message_id = arg_str!(message_id, "loom_chat_remove_reaction_json");
    let kind = arg_str!(kind, "loom_chat_remove_reaction_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_remove_reaction_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_remove_reaction_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                message_id.to_string(),
                kind.to_string(),
                expected_entity_tag.map(str::to_string),
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
    require_json_out!(out, "loom_chat_emoji_list_json");
    let h = handle_ref!(handle, "loom_chat_emoji_list_json");
    let workspace = arg_str!(workspace, "loom_chat_emoji_list_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_emoji_list_json");
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_emoji_list_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_emoji_register_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    kind: *const c_char,
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_emoji_register_json");
    let h = handle_ref!(handle, "loom_chat_emoji_register_json");
    let workspace = arg_str!(workspace, "loom_chat_emoji_register_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_emoji_register_json");
    let kind = arg_str!(kind, "loom_chat_emoji_register_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_emoji_register_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_emoji_register_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                kind.to_string(),
                expected_entity_tag.map(str::to_string),
            ))
        })
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chat_emoji_unregister_json(
    handle: *mut LoomSession,
    workspace: *const c_char,
    chat_workspace_id: *const c_char,
    kind: *const c_char,
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_emoji_unregister_json");
    let h = handle_ref!(handle, "loom_chat_emoji_unregister_json");
    let workspace = arg_str!(workspace, "loom_chat_emoji_unregister_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_emoji_unregister_json");
    let kind = arg_str!(kind, "loom_chat_emoji_unregister_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_emoji_unregister_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_emoji_unregister_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                kind.to_string(),
                expected_entity_tag.map(str::to_string),
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
    require_json_out!(out, "loom_chat_messages_json");
    let h = handle_ref!(handle, "loom_chat_messages_json");
    let workspace = arg_str!(workspace, "loom_chat_messages_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_messages_json");
    let channel_id = arg_str!(channel_id, "loom_chat_messages_json");
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_messages_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
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
    require_json_out!(out, "loom_chat_cursor_json");
    let h = handle_ref!(handle, "loom_chat_cursor_json");
    let workspace = arg_str!(workspace, "loom_chat_cursor_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_cursor_json");
    let channel_id = arg_str!(channel_id, "loom_chat_cursor_json");
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_cursor_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
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
    expected_entity_tag: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_update_cursor_json");
    let h = handle_ref!(handle, "loom_chat_update_cursor_json");
    let workspace = arg_str!(workspace, "loom_chat_update_cursor_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_update_cursor_json");
    let channel_id = arg_str!(channel_id, "loom_chat_update_cursor_json");
    let expected_entity_tag = match unsafe {
        optional_str_arg_generated(expected_entity_tag, "loom_chat_update_cursor_json")
    } {
        Ok(value) => value,
        Err(e) => return fail(e),
    };
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_update_cursor_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                next_sequence,
                expected_entity_tag.map(str::to_string),
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
    max: u64,
    out: *mut *mut c_char,
) -> i32 {
    clear_error();
    require_json_out!(out, "loom_chat_fetch_events_json");
    let h = handle_ref!(handle, "loom_chat_fetch_events_json");
    let workspace = arg_str!(workspace, "loom_chat_fetch_events_json");
    let chat_workspace_id = arg_str!(chat_workspace_id, "loom_chat_fetch_events_json");
    let channel_id = arg_str!(channel_id, "loom_chat_fetch_events_json");
    out_json!(
        out,
        chat_generated_string(h, |client, session| {
            crate::generated_local::block_generated(Chat::chat_fetch_events_json(
                client,
                session,
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.to_string(),
                from_sequence,
                max,
            ))
        })
    )
}
