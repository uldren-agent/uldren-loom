import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const loom = require("./index.js");

function parse(json) {
  return JSON.parse(json);
}

const path = join(mkdtempSync(join(tmpdir(), "loom-chat-projection-")), "chat.loom");
loom.createLoom(path, "default", null, null);
const identity = parse(loom.identityListJson(path));
const authenticatedPrincipal = identity.root;
assert.equal(typeof authenticatedPrincipal, "string");
loom.workspaceCreate(path, "repo", "vcs");

const channelId = "22222222-2222-4222-8222-222222222222";
loom.chatCreateChannelJson(path, "repo", "studio", channelId, "team", "Team");
const posted = parse(loom.chatPostMessageJson(path, "repo", "studio", "team", "m1", null, "hello"));
assert.equal(posted.operation_kind, "message.created");

const emoji = parse(loom.chatEmojiRegisterJson(path, "repo", "studio", "thumbs_up", null));
assert.deepEqual(emoji.custom, ["thumbs_up"]);
assert.deepEqual(parse(loom.chatEmojiListJson(path, "repo", "studio")).custom, ["thumbs_up"]);

const reacted = parse(
  loom.chatAddReactionJson(path, "repo", "studio", "team", "m1", "thumbs_up", posted.entity_tag),
);
assert.equal(reacted.operation_kind, "reaction.added");
const withReaction = parse(loom.chatMessagesJson(path, "repo", "studio", "team"));
assert.equal(withReaction.messages[0].reactions[0].kind, "thumbs_up");
assert.equal(withReaction.messages[0].reactions[0].principal, authenticatedPrincipal);

const removed = parse(
  loom.chatRemoveReactionJson(path, "repo", "studio", "team", "m1", "thumbs_up", reacted.entity_tag),
);
assert.equal(removed.operation_kind, "reaction.removed");
assert.throws(
  () => loom.chatAddReactionJson(path, "repo", "studio", "team", "m1", "thumbs_up", reacted.entity_tag),
  /CONFLICT/i,
);

const cursor = parse(loom.chatCursorJson(path, "repo", "studio", "team"));
assert.equal(cursor.next_sequence, 0);
const advanced = parse(
  loom.chatUpdateCursorJson(path, "repo", "studio", "team", 1n, cursor.entity_tag),
);
assert.equal(advanced.next_sequence, 1);
assert.throws(
  () => loom.chatUpdateCursorJson(path, "repo", "studio", "team", 0n, cursor.entity_tag),
  /CONFLICT/i,
);

const batch = parse(loom.chatFetchEventsJson(path, "repo", "studio", "team", 1n, 2));
assert.equal(batch.events.length, 2);
assert.deepEqual(
  batch.events.map((event) => event.operation_kind),
  ["message.created", "reaction.added"],
);
assert.equal(typeof batch.next, "string");

const unregistered = parse(loom.chatEmojiUnregisterJson(path, "repo", "studio", "thumbs_up", emoji.entity_tag));
assert.deepEqual(unregistered.custom, []);

const reopenedMessages = parse(loom.chatMessagesJson(path, "repo", "studio", "team"));
assert.deepEqual(reopenedMessages.messages[0].reactions, []);
const reopenedCursor = parse(loom.chatCursorJson(path, "repo", "studio", "team"));
assert.equal(reopenedCursor.next_sequence, 1);
assert.deepEqual(parse(loom.chatEmojiListJson(path, "repo", "studio")).custom, []);
