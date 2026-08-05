import json
import re

import pytest
import uldrenai_loom


def _tag(value: str) -> str:
    return json.loads(value)["entity_tag"]


def test_chat_generated_channel_message_wrappers_round_trip(tmp_path):
    path = str(tmp_path / "chat.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.workspace_create(path, "repo", "vcs")

    channel_id = "11111111-1111-4111-8111-111111111111"
    created = json.loads(
        uldrenai_loom.chat_create_channel_json(path, "repo", "studio", channel_id, "general", "General")
    )
    assert created["channel_id"] == channel_id
    assert len(json.loads(uldrenai_loom.chat_list_channels_json(path, "repo", "studio"))) == 1

    renamed = json.loads(uldrenai_loom.chat_rename_channel_json(path, "repo", "studio", "general", "team"))
    assert renamed["channel_id"] == channel_id
    assert renamed["handle"] == "team"

    text_post = json.loads(
        uldrenai_loom.chat_post_message_json(path, "repo", "studio", "team", "m-text", None, "hello")
    )
    assert text_post["operation_kind"] == "message.created"
    thread = json.loads(uldrenai_loom.chat_create_thread_json(path, "repo", "studio", "team", "thread-1", "m-text"))
    assert thread["operation_kind"] == "thread.created"
    edited_text = json.loads(
        uldrenai_loom.chat_edit_message_json(path, "repo", "studio", "team", "m-text", "hello edited")
    )
    assert edited_text["operation_kind"] == "message.edited"

    byte_body = bytes([0, 0xFF, 0x68, 0xFE, 0x69])
    byte_post_json = uldrenai_loom.chat_post_message_bytes_json(
        path, "repo", "studio", "team", "m-bytes", "thread-1", byte_body
    )
    byte_post = json.loads(byte_post_json)
    assert byte_post["operation_kind"] == "message.created"
    byte_tag = _tag(byte_post_json)
    edited_body = bytes([0xF0, 0x28, 0x8C, 0x28, 0x21])
    edited_bytes = json.loads(
        uldrenai_loom.chat_edit_message_bytes_json(
            path, "repo", "studio", "team", "m-bytes", edited_body, byte_tag
        )
    )
    assert edited_bytes["operation_kind"] == "message.edited"
    with pytest.raises(RuntimeError, match=re.compile("CONFLICT", re.I)):
        uldrenai_loom.chat_edit_message_bytes_json(
            path, "repo", "studio", "team", "m-bytes", b"stale", byte_tag
        )

    redacted = json.loads(uldrenai_loom.chat_redact_message_json(path, "repo", "studio", "team", "m-text", "cleanup"))
    assert redacted["operation_kind"] == "message.redacted"

    reopened = json.loads(uldrenai_loom.chat_messages_json(path, "repo", "studio", "team"))
    byte_message = next(message for message in reopened["messages"] if message["message_id"] == "m-bytes")
    assert byte_message["body"] == list(edited_body)
    redacted_message = next(message for message in reopened["messages"] if message["message_id"] == "m-text")
    assert redacted_message["redacted"] is True
