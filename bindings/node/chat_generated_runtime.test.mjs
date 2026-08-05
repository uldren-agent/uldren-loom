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

const path = join(mkdtempSync(join(tmpdir(), "loom-chat-generated-")), "chat.loom");
loom.createLoom(path, "default", null, null);
loom.workspaceCreate(path, "repo", "vcs");

const channelId = "11111111-1111-4111-8111-111111111111";
const created = JSON.parse(
  loom.chatCreateChannelJson(path, "repo", "studio", channelId, "general", "General"),
);
assert.equal(created.channel_id, channelId);
assert.equal(JSON.parse(loom.chatListChannelsJson(path, "repo", "studio")).length, 1);

const renamed = JSON.parse(loom.chatRenameChannelJson(path, "repo", "studio", "general", "team"));
assert.equal(renamed.channel_id, channelId);
assert.equal(renamed.handle, "team");

const textPost = JSON.parse(
  loom.chatPostMessageJson(path, "repo", "studio", "team", "m-text", null, "hello"),
);
assert.equal(textPost.operation_kind, "message.created");
const thread = JSON.parse(loom.chatCreateThreadJson(path, "repo", "studio", "team", "thread-1", "m-text"));
assert.equal(thread.operation_kind, "thread.created");
const editedText = JSON.parse(loom.chatEditMessageJson(path, "repo", "studio", "team", "m-text", "hello edited"));
assert.equal(editedText.operation_kind, "message.edited");

const byteBody = Buffer.from([0, 0xff, 0x68, 0xfe, 0x69]);
const bytePostJson = loom.chatPostMessageBytesJson(
  path,
  "repo",
  "studio",
  "team",
  "m-bytes",
  "thread-1",
  byteBody,
  null,
);
const bytePost = JSON.parse(bytePostJson);
assert.equal(bytePost.operation_kind, "message.created");
const byteTag = entityTag(bytePostJson);
const editedBody = Buffer.from([0xf0, 0x28, 0x8c, 0x28, 0x21]);
const editedBytes = JSON.parse(
  loom.chatEditMessageBytesJson(path, "repo", "studio", "team", "m-bytes", editedBody, byteTag),
);
assert.equal(editedBytes.operation_kind, "message.edited");
assert.throws(
  () => loom.chatEditMessageBytesJson(path, "repo", "studio", "team", "m-bytes", Buffer.from("stale"), byteTag),
  /CONFLICT/i,
);

const redacted = JSON.parse(
  loom.chatRedactMessageJson(path, "repo", "studio", "team", "m-text", "cleanup"),
);
assert.equal(redacted.operation_kind, "message.redacted");

const reopened = JSON.parse(loom.chatMessagesJson(path, "repo", "studio", "team"));
const byteMessage = reopened.messages.find((message) => message.message_id === "m-bytes");
assert.deepEqual(byteMessage.body, [...editedBody]);
const redactedMessage = reopened.messages.find((message) => message.message_id === "m-text");
assert.equal(redactedMessage.redacted, true);
