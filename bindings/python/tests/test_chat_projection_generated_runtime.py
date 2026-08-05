import json
import re

import pytest
import uldrenai_loom


def _parse(value: str):
    return json.loads(value)


def test_chat_projection_generated_wrappers_round_trip(tmp_path):
    path = str(tmp_path / "chat.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    identity = _parse(uldrenai_loom.identity_list_json(path))
    authenticated_principal = identity["root"]
    assert isinstance(authenticated_principal, str)
    uldrenai_loom.workspace_create(path, "repo", "vcs")

    channel_id = "22222222-2222-4222-8222-222222222222"
    uldrenai_loom.chat_create_channel_json(path, "repo", "studio", channel_id, "team", "Team")
    posted = _parse(uldrenai_loom.chat_post_message_json(path, "repo", "studio", "team", "m1", None, "hello"))
    assert posted["operation_kind"] == "message.created"

    emoji = _parse(uldrenai_loom.chat_emoji_register_json(path, "repo", "studio", "thumbs_up"))
    assert emoji["custom"] == ["thumbs_up"]
    assert _parse(uldrenai_loom.chat_emoji_list_json(path, "repo", "studio"))["custom"] == ["thumbs_up"]

    reacted = _parse(
        uldrenai_loom.chat_add_reaction_json(
            path, "repo", "studio", "team", "m1", "thumbs_up", posted["entity_tag"]
        )
    )
    assert reacted["operation_kind"] == "reaction.added"
    with_reaction = _parse(uldrenai_loom.chat_messages_json(path, "repo", "studio", "team"))
    assert with_reaction["messages"][0]["reactions"][0]["kind"] == "thumbs_up"
    assert with_reaction["messages"][0]["reactions"][0]["principal"] == authenticated_principal

    removed = _parse(
        uldrenai_loom.chat_remove_reaction_json(
            path, "repo", "studio", "team", "m1", "thumbs_up", reacted["entity_tag"]
        )
    )
    assert removed["operation_kind"] == "reaction.removed"
    with pytest.raises(RuntimeError, match=re.compile("CONFLICT", re.I)):
        uldrenai_loom.chat_add_reaction_json(
            path, "repo", "studio", "team", "m1", "thumbs_up", reacted["entity_tag"]
        )

    cursor = _parse(uldrenai_loom.chat_cursor_json(path, "repo", "studio", "team"))
    assert cursor["next_sequence"] == 0
    advanced = _parse(
        uldrenai_loom.chat_update_cursor_json(
            path, "repo", "studio", "team", 1, cursor["entity_tag"]
        )
    )
    assert advanced["next_sequence"] == 1
    with pytest.raises(RuntimeError, match=re.compile("CONFLICT", re.I)):
        uldrenai_loom.chat_update_cursor_json(path, "repo", "studio", "team", 0, cursor["entity_tag"])

    batch = _parse(uldrenai_loom.chat_fetch_events_json(path, "repo", "studio", "team", 1, 2))
    assert len(batch["events"]) == 2
    assert [event["operation_kind"] for event in batch["events"]] == [
        "message.created",
        "reaction.added",
    ]
    assert isinstance(batch["next"], str)

    unregistered = _parse(
        uldrenai_loom.chat_emoji_unregister_json(path, "repo", "studio", "thumbs_up", emoji["entity_tag"])
    )
    assert unregistered["custom"] == []

    reopened_messages = _parse(uldrenai_loom.chat_messages_json(path, "repo", "studio", "team"))
    assert reopened_messages["messages"][0]["reactions"] == []
    reopened_cursor = _parse(uldrenai_loom.chat_cursor_json(path, "repo", "studio", "team"))
    assert reopened_cursor["next_sequence"] == 1
    assert _parse(uldrenai_loom.chat_emoji_list_json(path, "repo", "studio"))["custom"] == []
