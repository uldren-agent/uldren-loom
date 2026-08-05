import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..", "..");

const inventory = [
  "drive_list_json",
  "drive_stat_json",
  "drive_read_file",
  "drive_list_versions_json",
  "drive_list_conflicts_json",
  "drive_list_shares_json",
  "drive_list_retention_json",
  "drive_create_folder_json",
  "drive_create_upload_json",
  "drive_upload_chunk_json",
  "drive_commit_upload_json",
  "drive_rename_json",
  "drive_move_json",
  "drive_delete_json",
  "drive_resolve_conflict_json",
  "drive_grant_share_json",
  "drive_revoke_share_json",
  "drive_apply_share_expiry_json",
  "drive_pin_retention_json",
  "drive_unpin_retention_json",
  "drive_apply_retention_json",
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

const source = readFileSync(join(here, "src/drive.rs"), "utf8");
const idl = compact(readFileSync(join(root, "idl/loom.idl"), "utf8"));
const generated = readFileSync(join(root, "crates/loom-remote-protocol/src/generated_api.rs"), "utf8");

assert.equal(source.includes("use loom_client::generated_api::Drive as GeneratedDrive;"), true);
assert.equal(source.includes("loom_drive::"), false);
assert.equal(source.includes("use loom_drive"), false);
assert.equal(source.includes("fn drive_write"), false);
assert.equal(source.includes("fn to_json"), false);
assert.equal(source.includes("fn parse_resolution"), false);
for (const name of inventory) {
  const body = bodyFor(source, name);
  assert.ok(body.includes("generated_session::open_generated_session"), name);
  assert.ok(body.includes(`>::${name}`), name);
  assert.equal(body.includes("drive_read("), false, name);
  assert.equal(body.includes("drive_write("), false, name);
  assert.ok(generated.includes(`Generated binding for \`Drive.${name}\``), name);
}

for (const signature of [
  "string drive_list_json(LoomSession handle,string workspace,string drive_workspace_id,string folder_id);",
  "string drive_stat_json(LoomSession handle,string workspace,string drive_workspace_id,string folder_id,string name);",
  "bytes drive_read_file(LoomSession handle,string workspace,string drive_workspace_id,string file_id);",
  "string drive_list_versions_json(LoomSession handle,string workspace,string drive_workspace_id,string file_id);",
  "string drive_list_conflicts_json(LoomSession handle,string workspace,string drive_workspace_id);",
  "string drive_list_shares_json(LoomSession handle,string workspace,string drive_workspace_id);",
  "string drive_list_retention_json(LoomSession handle,string workspace,string drive_workspace_id);",
  "string drive_create_folder_json(LoomSession handle,string workspace,string drive_workspace_id,string parent_folder_id,string folder_id,string name,string expected_root);",
  "string drive_create_upload_json(LoomSession handle,string workspace,string drive_workspace_id,string upload_id,string parent_folder_id,string name,string file_id,string expected_root,u64 created_at_ms,bool replace_file);",
  "string drive_upload_chunk_json(LoomSession handle,string workspace,string drive_workspace_id,string upload_id,bytes chunk);",
  "string drive_commit_upload_json(LoomSession handle,string workspace,string drive_workspace_id,string upload_id);",
  "string drive_rename_json(LoomSession handle,string workspace,string drive_workspace_id,string folder_id,string node_id,string new_name,string expected_root);",
  "string drive_move_json(LoomSession handle,string workspace,string drive_workspace_id,string source_folder_id,string target_folder_id,string node_id,string expected_root);",
  "string drive_delete_json(LoomSession handle,string workspace,string drive_workspace_id,string folder_id,string node_id,string expected_root);",
  "string drive_resolve_conflict_json(LoomSession handle,string workspace,string drive_workspace_id,string conflict_id,string resolution);",
  "string drive_grant_share_json(LoomSession handle,string workspace,string drive_workspace_id,string grant_id,string target_kind,string target_id,string principal,string role,u64 granted_at_ms,optional u64 expires_at_ms);",
  "string drive_revoke_share_json(LoomSession handle,string workspace,string drive_workspace_id,string grant_id);",
  "string drive_apply_share_expiry_json(LoomSession handle,string workspace,string drive_workspace_id,u64 now_ms);",
  "string drive_pin_retention_json(LoomSession handle,string workspace,string drive_workspace_id,string pin_id,string kind,string root,optional string target_entity_id,u64 added_at_ms,optional u64 expires_at_ms);",
  "string drive_unpin_retention_json(LoomSession handle,string workspace,string drive_workspace_id,string pin_id);",
  "string drive_apply_retention_json(LoomSession handle,string workspace,string drive_workspace_id,u64 now_ms);",
]) {
  assert.ok(idl.includes(compact(signature)), signature);
}
