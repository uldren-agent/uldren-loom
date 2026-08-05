from pathlib import Path


ROOT = Path(__file__).parents[3]

INVENTORY = [
    "chat_create_channel_json",
    "chat_rename_channel_json",
    "chat_list_channels_json",
    "chat_post_message_json",
    "chat_post_message_bytes_json",
    "chat_edit_message_json",
    "chat_edit_message_bytes_json",
    "chat_redact_message_json",
    "chat_create_thread_json",
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


def test_chat_generated_ownership_matches_source_boundaries():
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

    for name in ("chat_post_message_bytes_json", "chat_edit_message_bytes_json"):
        assert name in init_py
        assert f"def {name}(" in stub

    for signature in [
        "string chat_create_channel_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string channel_handle,string name,optional string expected_entity_tag);",
        "string chat_rename_channel_json(LoomSession handle,string workspace,string chat_workspace_id,string selector,string channel_handle,optional string expected_entity_tag);",
        "string chat_list_channels_json(LoomSession handle,string workspace,string chat_workspace_id);",
        "string chat_post_message_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,optional string thread_id,string body_text,optional string expected_entity_tag);",
        "string chat_post_message_bytes_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,optional string thread_id,bytes body,optional string expected_entity_tag);",
        "string chat_edit_message_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,string body_text,optional string expected_entity_tag);",
        "string chat_edit_message_bytes_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,bytes body,optional string expected_entity_tag);",
        "string chat_redact_message_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,optional string reason,optional string expected_entity_tag);",
        "string chat_create_thread_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string thread_id,string parent_message_id,optional string expected_entity_tag);",
    ]:
        assert _compact(signature) in idl
