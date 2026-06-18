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

fn to_json<T: Serialize>(value: loom_core::error::Result<T>) -> PyResult<String> {
    let value = value.map_err(py_err)?;
    serde_json::to_string(&value).map_err(|error| PyRuntimeError::new_err(error.to_string()))
}

fn operation_batch_json(
    value: loom_core::error::Result<loom_substrate::changes::OperationChangeBatch>,
) -> PyResult<String> {
    let batch = value.map_err(py_err)?;
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

fn parse_workspace_id(value: &str) -> PyResult<WorkspaceId> {
    WorkspaceId::parse(value).map_err(py_err)
}

fn parse_string_list(value: &str) -> PyResult<Vec<String>> {
    serde_json::from_str(value).map_err(|error| PyRuntimeError::new_err(error.to_string()))
}

fn chat_read<T>(
    path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    f: impl FnOnce(&Loom<FileStore>, WorkspaceId) -> PyResult<T>,
) -> PyResult<T> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    f(&loom, workspace_id)
}

fn chat_write<T>(
    path: &str,
    workspace: &str,
    passphrase: Option<&str>,
    f: impl FnOnce(&mut Loom<FileStore>, WorkspaceId) -> PyResult<T>,
) -> PyResult<T> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let workspace_id = resolve_workspace_arg(&loom, workspace)?;
    let out = f(&mut loom, workspace_id)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(out)
}

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
    let channel_id = parse_workspace_id(channel_id)?;
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::ensure_channel(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            channel_handle,
            name,
        ))
    })
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
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::rename_channel(
            loom,
            ns,
            chat_workspace_id,
            selector,
            channel_handle,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, passphrase=None))]
pub(crate) fn chat_list_channels_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    chat_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::list_channels(loom, ns, chat_workspace_id))
    })
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
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::post_message(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            message_id,
            thread_id,
            body_text.as_bytes().to_vec(),
        ))
    })
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
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::edit_message(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            message_id,
            body_text.as_bytes().to_vec(),
        ))
    })
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
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::redact_message(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            message_id,
            reason,
        ))
    })
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
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::create_thread(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            thread_id,
            parent_message_id,
        ))
    })
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
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::create_task(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            task_id,
            message_id,
            title,
        ))
    })
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
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::claim_task(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            task_id,
            claim_id,
            lease_token,
        ))
    })
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
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::complete_task(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            task_id,
            claim_id,
            result_message_id,
        ))
    })
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
    let agent_principal = parse_workspace_id(agent_principal)?;
    let source_message_ids = parse_string_list(source_message_ids_json)?;
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::invoke_agent(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            invocation_id,
            agent_principal,
            source_message_ids,
            prompt_text.as_bytes().to_vec(),
        ))
    })
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
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::agent_reply(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            invocation_id,
            message_id,
        ))
    })
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
    let from_agent_principal = parse_workspace_id(from_agent_principal)?;
    let to_principal = to_principal.map(parse_workspace_id).transpose()?;
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::request_handoff(
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
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, message_id, kind, passphrase=None))]
pub(crate) fn chat_add_reaction_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    kind: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::add_reaction(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            message_id,
            kind,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, message_id, kind, passphrase=None))]
pub(crate) fn chat_remove_reaction_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    message_id: &str,
    kind: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::remove_reaction(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            message_id,
            kind,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, passphrase=None))]
pub(crate) fn chat_emoji_list_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    chat_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::emoji_registry(loom, ns, chat_workspace_id))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, kind, passphrase=None))]
pub(crate) fn chat_emoji_register_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    kind: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::register_emoji(loom, ns, chat_workspace_id, kind))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, kind, passphrase=None))]
pub(crate) fn chat_emoji_unregister_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    kind: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::unregister_emoji(
            loom,
            ns,
            chat_workspace_id,
            kind,
        ))
    })
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
    chat_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::channel_projection(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
        ))
    })
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
    chat_read(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::read_cursor(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (path, workspace, chat_workspace_id, channel_id, next_sequence, passphrase=None))]
pub(crate) fn chat_update_cursor_json(
    path: &str,
    workspace: &str,
    chat_workspace_id: &str,
    channel_id: &str,
    next_sequence: u64,
    passphrase: Option<&str>,
) -> PyResult<String> {
    chat_write(path, workspace, passphrase, |loom, ns| {
        to_json(loom_chat::update_cursor(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            next_sequence,
        ))
    })
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
    chat_read(path, workspace, passphrase, |loom, ns| {
        operation_batch_json(loom_chat::operation_changes(
            loom,
            ns,
            chat_workspace_id,
            channel_id,
            from_sequence,
            max,
        ))
    })
}
