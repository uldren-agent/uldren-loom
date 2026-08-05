from pathlib import Path


ROOT = Path(__file__).parents[3]

INVENTORY = [
    "chat_add_reaction_json",
    "chat_remove_reaction_json",
    "chat_emoji_list_json",
    "chat_emoji_register_json",
    "chat_emoji_unregister_json",
    "chat_messages_json",
    "chat_cursor_json",
    "chat_update_cursor_json",
    "chat_fetch_events_json",
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


def test_chat_projection_generated_ownership_matches_source_boundaries():
    source = (ROOT / "bindings" / "python" / "src" / "chat.rs").read_text()
    idl = _compact((ROOT / "idl" / "loom.idl").read_text())
    generated = (ROOT / "crates" / "loom-remote-protocol" / "src" / "generated_api.rs").read_text()
    stub = (ROOT / "bindings" / "python" / "python" / "uldrenai_loom" / "__init__.pyi").read_text()

    actual = [
        name
        for name in INVENTORY
        if f"pub(crate) fn {name}" in source
    ]
    assert actual == INVENTORY
    assert len(actual) == len(set(actual)) == 9

    for name in INVENTORY:
        body = _body_for(source, name)
        assert "generated_session::open_generated_session" in body
        assert f">::{name}" in body
        assert "loom_chat::" not in body
        assert "chat_read(" not in body
        assert "chat_write(" not in body
        assert f"Generated binding for `Chat.{name}`" in generated

    for forbidden in [
        "fn to_json",
        "fn operation_batch_json",
        "fn chat_read",
        "fn chat_write",
        "OperationEventJson",
        "OperationBatchJson",
        "loom_chat::",
    ]:
        assert forbidden not in source

    for signature in [
        "string chat_add_reaction_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,string kind,optional string expected_entity_tag);",
        "string chat_remove_reaction_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,string kind,optional string expected_entity_tag);",
        "string chat_emoji_list_json(LoomSession handle,string workspace,string chat_workspace_id);",
        "string chat_emoji_register_json(LoomSession handle,string workspace,string chat_workspace_id,string kind,optional string expected_entity_tag);",
        "string chat_emoji_unregister_json(LoomSession handle,string workspace,string chat_workspace_id,string kind,optional string expected_entity_tag);",
        "string chat_messages_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id);",
        "string chat_cursor_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id);",
        "string chat_update_cursor_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,u64 next_sequence,optional string expected_entity_tag);",
        "string chat_fetch_events_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,u64 from_sequence,u64 max);",
    ]:
        assert _compact(signature) in idl

    for signature in [
        "def chat_add_reaction_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, message_id: str, kind: str, expected_entity_tag: str | None = None, passphrase: str | None = None) -> str",
        "def chat_remove_reaction_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, message_id: str, kind: str, expected_entity_tag: str | None = None, passphrase: str | None = None) -> str",
        "def chat_emoji_register_json(path: str, workspace: str, chat_workspace_id: str, kind: str, expected_entity_tag: str | None = None, passphrase: str | None = None) -> str",
        "def chat_emoji_unregister_json(path: str, workspace: str, chat_workspace_id: str, kind: str, expected_entity_tag: str | None = None, passphrase: str | None = None) -> str",
        "def chat_update_cursor_json(path: str, workspace: str, chat_workspace_id: str, channel_id: str, next_sequence: int, expected_entity_tag: str | None = None, passphrase: str | None = None) -> str",
    ]:
        assert signature in stub
