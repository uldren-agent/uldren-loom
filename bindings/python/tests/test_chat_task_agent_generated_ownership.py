from pathlib import Path


ROOT = Path(__file__).parents[3]

INVENTORY = [
    "chat_create_task_json",
    "chat_claim_task_json",
    "chat_complete_task_json",
    "chat_invoke_agent_json",
    "chat_invoke_agent_bytes_json",
    "chat_agent_reply_json",
    "chat_request_handoff_json",
]


def _compact(value: str) -> str:
    compact = " ".join(value.split())
    for token in ("(", ")", ",", ";"):
        compact = compact.replace(f" {token}", token).replace(f"{token} ", token)
    return compact


def _body_for(source: str, name: str) -> str:
    start = source.index(f"pub(crate) fn {name}")
    next_start = source.find("\n#[pyfunction]", start + 1)
    return source[start:] if next_start == -1 else source[start:next_start]


def test_chat_task_agent_generated_ownership_matches_source_boundaries():
    source = (ROOT / "bindings" / "python" / "src" / "chat.rs").read_text()
    idl = _compact((ROOT / "idl" / "loom.idl").read_text())
    generated = (ROOT / "crates" / "loom-remote-protocol" / "src" / "generated_api.rs").read_text()
    init_py = (ROOT / "bindings" / "python" / "python" / "uldrenai_loom" / "__init__.py").read_text()
    stub = (ROOT / "bindings" / "python" / "python" / "uldrenai_loom" / "__init__.pyi").read_text()

    assert "use loom_client::generated_api::Chat as GeneratedChat;" in source
    for name in INVENTORY:
        body = _body_for(source, name)
        assert "generated_session::open_generated_session" in body
        assert f">::{name}" in body
        assert "loom_chat::" not in body
        assert "chat_read(" not in body
        assert "chat_write(" not in body
        assert f"Generated binding for `Chat.{name}`" in generated

    assert "chat_invoke_agent_bytes_json" in init_py
    assert "def chat_invoke_agent_bytes_json(" in stub

    for signature in [
        "string chat_create_task_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string task_id,optional string message_id,string title,optional string expected_entity_tag);",
        "string chat_claim_task_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string task_id,string claim_id,optional string lease_token,optional string expected_entity_tag);",
        "string chat_complete_task_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string task_id,string claim_id,optional string result_message_id,optional string expected_entity_tag);",
        "string chat_invoke_agent_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string invocation_id,string agent_principal,string source_message_ids_json,string prompt_text,optional string expected_entity_tag);",
        "string chat_invoke_agent_bytes_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string invocation_id,string agent_principal,string source_message_ids_json,bytes prompt,optional string expected_entity_tag);",
        "string chat_agent_reply_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string invocation_id,string message_id,optional string expected_entity_tag);",
        "string chat_request_handoff_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string handoff_id,string from_agent_principal,optional string to_principal,optional string reason,optional string expected_entity_tag);",
    ]:
        assert _compact(signature) in idl
