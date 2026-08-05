import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const loom = require("./index.js");

function entityTag(json) {
  return JSON.parse(json).entity_tag;
}

const path = join(mkdtempSync(join(tmpdir(), "loom-chat-task-agent-generated-")), "chat.loom");
loom.createLoom(path, "default", null, null);
loom.workspaceCreate(path, "repo", "vcs");

const channelId = "21111111-1111-4111-8111-111111111111";
const agent = "22222222-2222-4222-8222-222222222222";
const recipient = "23333333-3333-4333-8333-333333333333";
loom.chatCreateChannelJson(path, "repo", "studio", channelId, "team", "Team");
const sourcePost = loom.chatPostMessageJson(path, "repo", "studio", "team", "m-source", null, "source");
const sourceTag = entityTag(sourcePost);

const task = JSON.parse(loom.chatCreateTaskJson(path, "repo", "studio", "team", "task-1", "m-source", "Do it"));
assert.equal(task.operation_kind, "task.created");
const taskTag = task.entity_tag;
const claim = JSON.parse(loom.chatClaimTaskJson(path, "repo", "studio", "team", "task-1", "claim-1", "lease-1"));
assert.equal(claim.operation_kind, "task.claimed");
loom.chatPostMessageJson(path, "repo", "studio", "team", "m-result", null, "done");
const complete = JSON.parse(loom.chatCompleteTaskJson(path, "repo", "studio", "team", "task-1", "claim-1", "m-result"));
assert.equal(complete.operation_kind, "task.completed");

const textInvoke = JSON.parse(
  loom.chatInvokeAgentJson(path, "repo", "studio", "team", "inv-text", agent, "[\"m-source\"]", "prompt"),
);
assert.equal(textInvoke.operation_kind, "agent.invoked");
const reply = JSON.parse(loom.chatAgentReplyJson(path, "repo", "studio", "team", "inv-text", "m-result"));
assert.equal(reply.operation_kind, "agent.replied");

const bytePrompt = Buffer.from([0, 0xff, 0x61, 0xfe, 0x62]);
const byteInvoke = JSON.parse(
  loom.chatInvokeAgentBytesJson(
    path,
    "repo",
    "studio",
    "team",
    "inv-bytes",
    agent,
    "[\"m-source\"]",
    bytePrompt,
    null,
  ),
);
assert.equal(byteInvoke.operation_kind, "agent.invoked");
assert.throws(
  () =>
    loom.chatInvokeAgentBytesJson(
      path,
      "repo",
      "studio",
      "team",
      "inv-stale",
      agent,
      "[\"m-source\"]",
      Buffer.from("stale"),
      sourceTag,
    ),
  /CONFLICT/i,
);

const handoffAbsent = JSON.parse(
  loom.chatRequestHandoffJson(path, "repo", "studio", "team", "handoff-absent", agent),
);
assert.equal(handoffAbsent.operation_kind, "handoff.requested");
const handoffPresent = JSON.parse(
  loom.chatRequestHandoffJson(path, "repo", "studio", "team", "handoff-present", agent, recipient, "please take it"),
);
assert.equal(handoffPresent.operation_kind, "handoff.requested");

const reopened = JSON.parse(loom.chatMessagesJson(path, "repo", "studio", "team"));
const reopenedTask = reopened.tasks.find((item) => item.task_id === "task-1");
assert.equal(reopenedTask.state.result_message_id, "m-result");
const invocation = reopened.agent_invocations.find((item) => item.invocation_id === "inv-bytes");
assert.deepEqual(invocation.prompt, [...bytePrompt]);
assert.ok(invocation.reply_message_ids.length === 0);
assert.ok(reopened.handoffs.some((item) => item.handoff_id === "handoff-absent" && item.to_principal === null));
assert.ok(
  reopened.handoffs.some(
    (item) =>
      item.handoff_id === "handoff-present" &&
      item.to_principal === recipient &&
      item.reason === "please take it",
  ),
);
assert.equal(taskTag.startsWith("entity-tag:"), true);
