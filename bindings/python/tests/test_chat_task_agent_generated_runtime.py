import json
import re

import pytest
import uldrenai_loom


def _tag(value: str) -> str:
    return json.loads(value)["entity_tag"]


def test_chat_task_agent_generated_wrappers_round_trip(tmp_path):
    path = str(tmp_path / "chat.loom")
    uldrenai_loom.create_loom(path, "default", None, None)
    uldrenai_loom.workspace_create(path, "repo", "vcs")

    channel_id = "21111111-1111-4111-8111-111111111111"
    agent = "22222222-2222-4222-8222-222222222222"
    recipient = "23333333-3333-4333-8333-333333333333"
    uldrenai_loom.chat_create_channel_json(path, "repo", "studio", channel_id, "team", "Team")
    source_post = uldrenai_loom.chat_post_message_json(path, "repo", "studio", "team", "m-source", None, "source")
    source_tag = _tag(source_post)

    task = json.loads(uldrenai_loom.chat_create_task_json(path, "repo", "studio", "team", "task-1", "m-source", "Do it"))
    assert task["operation_kind"] == "task.created"
    task_tag = task["entity_tag"]
    claim = json.loads(uldrenai_loom.chat_claim_task_json(path, "repo", "studio", "team", "task-1", "claim-1", "lease-1"))
    assert claim["operation_kind"] == "task.claimed"
    uldrenai_loom.chat_post_message_json(path, "repo", "studio", "team", "m-result", None, "done")
    complete = json.loads(uldrenai_loom.chat_complete_task_json(path, "repo", "studio", "team", "task-1", "claim-1", "m-result"))
    assert complete["operation_kind"] == "task.completed"

    text_invoke = json.loads(
        uldrenai_loom.chat_invoke_agent_json(path, "repo", "studio", "team", "inv-text", agent, "[\"m-source\"]", "prompt")
    )
    assert text_invoke["operation_kind"] == "agent.invoked"
    reply = json.loads(uldrenai_loom.chat_agent_reply_json(path, "repo", "studio", "team", "inv-text", "m-result"))
    assert reply["operation_kind"] == "agent.replied"

    byte_prompt = bytes([0, 0xFF, 0x61, 0xFE, 0x62])
    byte_invoke = json.loads(
        uldrenai_loom.chat_invoke_agent_bytes_json(
            path,
            "repo",
            "studio",
            "team",
            "inv-bytes",
            agent,
            "[\"m-source\"]",
            byte_prompt,
        )
    )
    assert byte_invoke["operation_kind"] == "agent.invoked"
    with pytest.raises(RuntimeError, match=re.compile("CONFLICT", re.I)):
        uldrenai_loom.chat_invoke_agent_bytes_json(
            path,
            "repo",
            "studio",
            "team",
            "inv-stale",
            agent,
            "[\"m-source\"]",
            b"stale",
            source_tag,
        )

    handoff_absent = json.loads(uldrenai_loom.chat_request_handoff_json(path, "repo", "studio", "team", "handoff-absent", agent))
    assert handoff_absent["operation_kind"] == "handoff.requested"
    handoff_present = json.loads(
        uldrenai_loom.chat_request_handoff_json(path, "repo", "studio", "team", "handoff-present", agent, recipient, "please take it")
    )
    assert handoff_present["operation_kind"] == "handoff.requested"

    reopened = json.loads(uldrenai_loom.chat_messages_json(path, "repo", "studio", "team"))
    reopened_task = next(item for item in reopened["tasks"] if item["task_id"] == "task-1")
    assert reopened_task["state"]["result_message_id"] == "m-result"
    invocation = next(item for item in reopened["agent_invocations"] if item["invocation_id"] == "inv-bytes")
    assert invocation["prompt"] == list(byte_prompt)
    assert invocation["reply_message_ids"] == []
    assert any(item["handoff_id"] == "handoff-absent" and item["to_principal"] is None for item in reopened["handoffs"])
    assert any(
        item["handoff_id"] == "handoff-present" and item["to_principal"] == recipient and item["reason"] == "please take it"
        for item in reopened["handoffs"]
    )
    assert task_tag.startswith("entity-tag:")
