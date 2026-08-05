//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

use futures::executor::block_on;
use loom_client::generated_api::Chat as GeneratedChat;

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, channel_handle, name, passphrase=None))]
pub(crate) fn chat_create_channel_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    channel_handle: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_create_channel_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            channel_handle.to_string(),
            name.to_string(),
            None,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, selector, channel_handle, passphrase=None))]
pub(crate) fn chat_rename_channel_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    selector: &str,
    channel_handle: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_rename_channel_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            selector.to_string(),
            channel_handle.to_string(),
            None,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, passphrase=None))]
pub(crate) fn chat_list_channels_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_list_channels_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, message_id, thread_id, body_text, passphrase=None))]
pub(crate) fn chat_post_message_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    thread_id: Option<&str>,
    body_text: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_post_message_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            message_id.to_string(),
            thread_id.map(str::to_string),
            body_text.to_string(),
            None,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, message_id, thread_id, body, expected_entity_tag=None, passphrase=None))]
pub(crate) fn chat_post_message_bytes_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    thread_id: Option<&str>,
    body: &[u8],
    expected_entity_tag: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_post_message_bytes_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            message_id.to_string(),
            thread_id.map(str::to_string),
            body.to_vec(),
            expected_entity_tag.map(str::to_string),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, message_id, body_text, passphrase=None))]
pub(crate) fn chat_edit_message_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    body_text: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_edit_message_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            message_id.to_string(),
            body_text.to_string(),
            None,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, message_id, body, expected_entity_tag=None, passphrase=None))]
pub(crate) fn chat_edit_message_bytes_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    body: &[u8],
    expected_entity_tag: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_edit_message_bytes_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            message_id.to_string(),
            body.to_vec(),
            expected_entity_tag.map(str::to_string),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, message_id, reason=None, passphrase=None))]
pub(crate) fn chat_redact_message_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    reason: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_redact_message_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            message_id.to_string(),
            reason.map(str::to_string),
            None,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, thread_id, parent_message_id, passphrase=None))]
pub(crate) fn chat_create_thread_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    thread_id: &str,
    parent_message_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_create_thread_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            thread_id.to_string(),
            parent_message_id.to_string(),
            None,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, task_id, message_id, title, passphrase=None))]
pub(crate) fn chat_create_task_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    task_id: &str,
    message_id: Option<&str>,
    title: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_create_task_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            task_id.to_string(),
            message_id.map(str::to_string),
            title.to_string(),
            None,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, task_id, claim_id, lease_token=None, passphrase=None))]
pub(crate) fn chat_claim_task_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    task_id: &str,
    claim_id: &str,
    lease_token: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_claim_task_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            task_id.to_string(),
            claim_id.to_string(),
            lease_token.map(str::to_string),
            None,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, task_id, claim_id, result_message_id=None, passphrase=None))]
pub(crate) fn chat_complete_task_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    task_id: &str,
    claim_id: &str,
    result_message_id: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_complete_task_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            task_id.to_string(),
            claim_id.to_string(),
            result_message_id.map(str::to_string),
            None,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, invocation_id, agent_principal, source_message_ids_json, prompt_text, passphrase=None))]
pub(crate) fn chat_invoke_agent_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    invocation_id: &str,
    agent_principal: &str,
    source_message_ids_json: &str,
    prompt_text: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_invoke_agent_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            invocation_id.to_string(),
            agent_principal.to_string(),
            source_message_ids_json.to_string(),
            prompt_text.to_string(),
            None,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, invocation_id, agent_principal, source_message_ids_json, prompt, expected_entity_tag=None, passphrase=None))]
pub(crate) fn chat_invoke_agent_bytes_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    invocation_id: &str,
    agent_principal: &str,
    source_message_ids_json: &str,
    prompt: &[u8],
    expected_entity_tag: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_invoke_agent_bytes_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            invocation_id.to_string(),
            agent_principal.to_string(),
            source_message_ids_json.to_string(),
            prompt.to_vec(),
            expected_entity_tag.map(str::to_string),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, invocation_id, message_id, passphrase=None))]
pub(crate) fn chat_agent_reply_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    invocation_id: &str,
    message_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_agent_reply_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            invocation_id.to_string(),
            message_id.to_string(),
            None,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, handoff_id, from_agent_principal, to_principal=None, reason=None, passphrase=None))]
pub(crate) fn chat_request_handoff_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    handoff_id: &str,
    from_agent_principal: &str,
    to_principal: Option<&str>,
    reason: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_request_handoff_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            handoff_id.to_string(),
            from_agent_principal.to_string(),
            to_principal.map(str::to_string),
            reason.map(str::to_string),
            None,
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, message_id, kind, expected_entity_tag=None, passphrase=None))]
pub(crate) fn chat_add_reaction_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    kind: &str,
    expected_entity_tag: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_add_reaction_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            message_id.to_string(),
            kind.to_string(),
            expected_entity_tag.map(str::to_string),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, message_id, kind, expected_entity_tag=None, passphrase=None))]
pub(crate) fn chat_remove_reaction_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    kind: &str,
    expected_entity_tag: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_remove_reaction_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            message_id.to_string(),
            kind.to_string(),
            expected_entity_tag.map(str::to_string),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, passphrase=None))]
pub(crate) fn chat_emoji_list_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_emoji_list_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, kind, expected_entity_tag=None, passphrase=None))]
pub(crate) fn chat_emoji_register_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    kind: &str,
    expected_entity_tag: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_emoji_register_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            kind.to_string(),
            expected_entity_tag.map(str::to_string),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, kind, expected_entity_tag=None, passphrase=None))]
pub(crate) fn chat_emoji_unregister_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    kind: &str,
    expected_entity_tag: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_emoji_unregister_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            kind.to_string(),
            expected_entity_tag.map(str::to_string),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, passphrase=None))]
pub(crate) fn chat_messages_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_messages_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, passphrase=None))]
pub(crate) fn chat_cursor_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_cursor_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, next_sequence, expected_entity_tag=None, passphrase=None))]
pub(crate) fn chat_update_cursor_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    next_sequence: u64,
    expected_entity_tag: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_update_cursor_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            next_sequence,
            expected_entity_tag.map(str::to_string),
        ),
    )
    .map_err(py_err)
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, from_sequence, max, passphrase=None))]
pub(crate) fn chat_fetch_events_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    from_sequence: u64,
    max: usize,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let max = u64::try_from(max).map_err(|_| PyRuntimeError::new_err("max exceeds u64"))?;
    let generated = generated_session::open_generated_session(path, passphrase, None, None)?;
    block_on(
        <loom_client::LocalLoomClient as GeneratedChat>::chat_fetch_events_json(
            &generated.client,
            generated.session.clone(),
            workspace.to_string(),
            chat_workspace_id.to_string(),
            channel_id.to_string(),
            from_sequence,
            max,
        ),
    )
    .map_err(py_err)
}
