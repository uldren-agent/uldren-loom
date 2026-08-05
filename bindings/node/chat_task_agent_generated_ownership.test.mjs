import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..", "..");

const inventory = [
  "chat_create_task_json",
  "chat_claim_task_json",
  "chat_complete_task_json",
  "chat_invoke_agent_json",
  "chat_invoke_agent_bytes_json",
  "chat_agent_reply_json",
  "chat_request_handoff_json",
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

assert.ok(indexJs.includes("module.exports.chatInvokeAgentBytesJson = nativeBinding.chatInvokeAgentBytesJson"));
assert.ok(indexDts.includes("function chatInvokeAgentBytesJson("));

for (const signature of [
  "string chat_create_task_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string task_id,optional string message_id,string title,optional string expected_entity_tag);",
  "string chat_claim_task_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string task_id,string claim_id,optional string lease_token,optional string expected_entity_tag);",
  "string chat_complete_task_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string task_id,string claim_id,optional string result_message_id,optional string expected_entity_tag);",
  "string chat_invoke_agent_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string invocation_id,string agent_principal,string source_message_ids_json,string prompt_text,optional string expected_entity_tag);",
  "string chat_invoke_agent_bytes_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string invocation_id,string agent_principal,string source_message_ids_json,bytes prompt,optional string expected_entity_tag);",
  "string chat_agent_reply_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string invocation_id,string message_id,optional string expected_entity_tag);",
  "string chat_request_handoff_json(LoomSession handle,string workspace,string chat_workspace_id,string channel_id,string handoff_id,string from_agent_principal,optional string to_principal,optional string reason,optional string expected_entity_tag);",
]) {
  assert.ok(idl.includes(compact(signature)), signature);
}
