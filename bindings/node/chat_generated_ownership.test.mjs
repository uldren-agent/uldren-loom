import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..", "..");

const inventory = [
  "chat_create_channel_json",
  "chat_rename_channel_json",
  "chat_list_channels_json",
  "chat_post_message_json",
  "chat_post_message_bytes_json",
  "chat_edit_message_json",
  "chat_edit_message_bytes_json",
  "chat_redact_message_json",
  "chat_create_thread_json",
];

function bodyFor(source, name) {
  const start = source.indexOf(`pub fn ${name}(`);
  assert.notEqual(start, -1, `${name} exists`);
  const next = source.indexOf("\n#[napi]", start + 1);
  return next === -1 ? source.slice(start) : source.slice(start, next);
}

function compact(value) {
  return value.replace(/\s+/g, " ").replace(/\s*([(),;])\s*/g, "$1").trim();
}

const source = readFileSync(join(here, "src/chat.rs"), "utf8");
const idl = compact(readFileSync(join(root, "idl/loom.idl"), "utf8"));
const generated = readFileSync(join(root, "crates/loom-remote-protocol/src/generated_api.rs"), "utf8");
const indexJs = readFileSync(join(here, "index.js"), "utf8");
const indexDts = readFileSync(join(here, "index.d.ts"), "utf8");

assert.equal(source.includes("use loom_client::generated_api::Chat as GeneratedChat;"), true);
for (const name of inventory) {
  const body = bodyFor(source, name);
  assert.ok(body.includes("generated_session::open_generated_session"), name);
  assert.ok(body.includes(`>::${name}`), name);
  assert.equal(body.includes("loom_chat::"), false, name);
  assert.equal(body.includes("chat_read("), false, name);
  assert.equal(body.includes("chat_write("), false, name);
  assert.ok(generated.includes(`Generated binding for \`Chat.${name}\``), name);
}

for (const exportName of [
  "chatPostMessageBytesJson",
  "chatEditMessageBytesJson",
]) {
  assert.ok(indexJs.includes(`module.exports.${exportName} = nativeBinding.${exportName}`), exportName);
  assert.ok(indexDts.includes(`function ${exportName}(`), exportName);
}

for (const signature of [
  "string chat_create_channel_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string channel_handle,string name,optional string expected_entity_tag);",
  "string chat_rename_channel_json(LoomSession handle,string workspace,string chat_workspace_id,string selector,string channel_handle,optional string expected_entity_tag);",
  "string chat_list_channels_json(LoomSession handle,string workspace,string chat_workspace_id);",
  "string chat_post_message_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,optional string thread_id,string body_text,optional string expected_entity_tag);",
  "string chat_post_message_bytes_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,optional string thread_id,bytes body,optional string expected_entity_tag);",
  "string chat_edit_message_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,string body_text,optional string expected_entity_tag);",
  "string chat_edit_message_bytes_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,bytes body,optional string expected_entity_tag);",
  "string chat_redact_message_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string message_id,optional string reason,optional string expected_entity_tag);",
  "string chat_create_thread_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string thread_id,string parent_message_id,optional string expected_entity_tag);",
]) {
  assert.ok(idl.includes(compact(signature)), signature);
}
