import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..", "..");

const inventory = [
  "chat_add_reaction_json",
  "chat_remove_reaction_json",
  "chat_emoji_list_json",
  "chat_emoji_register_json",
  "chat_emoji_unregister_json",
  "chat_messages_json",
  "chat_cursor_json",
  "chat_update_cursor_json",
  "chat_fetch_events_json",
];

function compact(value) {
  return value.replace(/\s+/g, " ").replace(/\s*([(),;])\s*/g, "$1").trim();
}

function bodyFor(source, name) {
  const start = source.indexOf(`pub fn ${name}(`);
  assert.notEqual(start, -1, `${name} exists`);
  const next = source.indexOf("\n#[napi]", start + 1);
  return next === -1 ? source.slice(start) : source.slice(start, next);
}

const source = readFileSync(join(here, "src/chat.rs"), "utf8");
const idl = compact(readFileSync(join(root, "idl/loom.idl"), "utf8"));
const generated = readFileSync(join(root, "crates/loom-remote-protocol/src/generated_api.rs"), "utf8");
const indexDts = readFileSync(join(here, "index.d.ts"), "utf8");

const actual = [...source.matchAll(/pub fn (chat_(?:add_reaction|remove_reaction|emoji_list|emoji_register|emoji_unregister|messages|cursor|update_cursor|fetch_events)_json)\(/g)].map(
  (match) => match[1],
);
assert.deepEqual(actual, inventory);
assert.equal(new Set(actual).size, inventory.length);

for (const name of inventory) {
  const body = bodyFor(source, name);
  assert.ok(body.includes("generated_session::open_generated_session"), name);
  assert.ok(body.includes(`>::${name}`), name);
  assert.equal(body.includes("loom_chat::"), false, name);
  assert.equal(body.includes("chat_read("), false, name);
  assert.equal(body.includes("chat_write("), false, name);
  assert.ok(generated.includes(`Generated binding for \`Chat.${name}\``), name);
}

for (const forbidden of [
  "fn to_json",
  "fn operation_batch_json",
  "fn chat_read",
  "fn chat_write",
  "OperationEventJson",
  "OperationBatchJson",
  "loom_chat::",
]) {
  assert.equal(source.includes(forbidden), false, forbidden);
}

for (const signature of [
  "string chat_add_reaction_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,string kind,optional string expected_entity_tag);",
  "string chat_remove_reaction_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,string kind,optional string expected_entity_tag);",
  "string chat_emoji_list_json(LoomSession handle,string workspace,string chat_workspace_id);",
  "string chat_emoji_register_json(LoomSession handle,string workspace,string chat_workspace_id,string kind,optional string expected_entity_tag);",
  "string chat_emoji_unregister_json(LoomSession handle,string workspace,string chat_workspace_id,string kind,optional string expected_entity_tag);",
  "string chat_messages_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id);",
  "string chat_cursor_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id);",
  "string chat_update_cursor_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,u64 next_sequence,optional string expected_entity_tag);",
  "string chat_fetch_events_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,u64 from_sequence,u64 max);",
]) {
  assert.ok(idl.includes(compact(signature)), signature);
}

for (const signature of [
  "function chatAddReactionJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, kind: string, expectedEntityTag?: string | undefined | null, passphrase?: string | undefined | null): string",
  "function chatRemoveReactionJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, kind: string, expectedEntityTag?: string | undefined | null, passphrase?: string | undefined | null): string",
  "function chatEmojiRegisterJson(loomPath: string, workspace: string, chatWorkspaceId: string, kind: string, expectedEntityTag?: string | undefined | null, passphrase?: string | undefined | null): string",
  "function chatEmojiUnregisterJson(loomPath: string, workspace: string, chatWorkspaceId: string, kind: string, expectedEntityTag?: string | undefined | null, passphrase?: string | undefined | null): string",
  "function chatUpdateCursorJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, nextSequence: bigint, expectedEntityTag?: string | undefined | null, passphrase?: string | undefined | null): string",
]) {
  assert.ok(indexDts.includes(signature), signature);
}
