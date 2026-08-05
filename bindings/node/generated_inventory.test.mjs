import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const require = createRequire(import.meta.url);
const loom = require("./index.js");
const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..", "..");

const inventory = [
  {
    exportName: "lifecycleDefineStandardJson",
    idl: "string lifecycle_define_standard_json(LoomSession handle, string workspace, string kind, string version, string completion_predicate_digest);",
    ts: "export declare function lifecycleDefineStandardJson(loomPath: string, workspace: string, kind: string, version: string, completionPredicateDigest: string, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): string",
    result: "string",
    owner: "GeneratedLifecycle",
    traitCall: "lifecycle_define_standard_json",
    source: "src/lifecycle_refs.rs",
  },
  {
    exportName: "lifecycleDefineJson",
    idl: "string lifecycle_define_json(LoomSession handle, string workspace, bytes definition);",
    ts: "export declare function lifecycleDefineJson(loomPath: string, workspace: string, definition: Uint8Array, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): string",
    result: "string",
    owner: "GeneratedLifecycle",
    traitCall: "lifecycle_define_json",
    source: "src/lifecycle_refs.rs",
  },
  {
    exportName: "lifecycleInstantiateJson",
    idl: "string lifecycle_instantiate_json(LoomSession handle, string workspace, string instance_id, string definition_id, list<string> subject_refs);",
    ts: "export declare function lifecycleInstantiateJson(loomPath: string, workspace: string, instanceId: string, definitionId: string, subjectRefs: Array<string>, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): string",
    result: "string",
    owner: "GeneratedLifecycle",
    traitCall: "lifecycle_instantiate_json",
    source: "src/lifecycle_refs.rs",
  },
  {
    exportName: "lifecycleTransitionJson",
    idl: "string lifecycle_transition_json(LoomSession handle, string workspace, string instance_id, string transition_id, string to_stage_id, optional string actor_principal_id, string gate_evaluations_json, optional string snapshot_digest);",
    ts: "export declare function lifecycleTransitionJson(loomPath: string, workspace: string, instanceId: string, transitionId: string, toStageId: string, actorPrincipalId: string | undefined | null, gateEvaluationsJson: string, snapshotDigest?: string | undefined | null, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): string",
    result: "string",
    owner: "GeneratedLifecycle",
    traitCall: "lifecycle_transition_json",
    source: "src/lifecycle_refs.rs",
  },
  {
    exportName: "refsReconcileJson",
    idl: "string refs_reconcile_json(LoomSession handle, string workspace, u64 max);",
    ts: "export declare function refsReconcileJson(loomPath: string, workspace: string, max: bigint, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): string",
    result: "string",
    owner: "GeneratedRefs",
    traitCall: "refs_reconcile_json",
    source: "src/lifecycle_refs.rs",
  },
  {
    exportName: "applyCbor",
    idl: "bytes apply_cbor(LoomSession handle, bytes request);",
    ts: "export declare function applyCbor(loomPath: string, request: Uint8Array, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): Uint8Array",
    result: "bytes",
    owner: "GeneratedExec",
    traitCall: "apply_cbor",
    source: "src/exec_generated.rs",
  },
  {
    exportName: "meetingsImportSnapshot",
    idl: "string meetings_import_snapshot(LoomSession handle, string workspace, string input_profile, bytes snapshot, bool dry_run );",
    ts: "export declare function meetingsImportSnapshot(loomPath: string, workspace: string, inputProfile: string, snapshot: Uint8Array, dryRun: boolean, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): string",
    result: "string",
    owner: "GeneratedMeetings",
    traitCall: "meetings_import_snapshot",
    source: "src/meetings.rs",
  },
  {
    exportName: "sqlExecResult",
    idl: "bytes sql_exec_result(LoomSession handle, string workspace, string db, string sql);",
    ts: "export declare function sqlExecResult(loomPath: string, workspace: string, db: string, sql: string, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): Uint8Array",
    result: "bytes",
    owner: "GeneratedSql",
    traitCall: "sql_exec_result",
    source: "src/sql_generated.rs",
  },
];

const idl = readFileSync(join(root, "idl/loom.idl"), "utf8").replace(/\s+/g, " ");
const generatedApi = readFileSync(join(root, "crates/loom-remote-protocol/src/generated_api.rs"), "utf8");
const indexJs = readFileSync(join(here, "index.js"), "utf8");
const dts = readFileSync(join(here, "index.d.ts"), "utf8");

function compactIdl(value) {
  return value.replace(/\s+/g, " ").replace(/\s*([(),;])\s*/g, "$1").trim();
}

function snakeToCamel(value) {
  return value.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
}

function sortedUnique(values) {
  return [...new Set(values)].sort();
}

function extractRustWrappers(sourceFile) {
  const source = readFileSync(join(here, sourceFile), "utf8");
  return [...source.matchAll(/#\[napi\]\s+pub fn ([a-z0-9_]+)\(/g)].map((match) => match[1]);
}

function isGeneratedGroupPublicName(name) {
  return (
    /^lifecycle[A-Z].*Json$/.test(name) ||
    name === "refsReconcileJson" ||
    name === "applyCbor" ||
    name === "meetingsImportSnapshot" ||
    name === "sqlExecResult"
  );
}

assert.deepEqual(inventory.map((entry) => entry.exportName), [
  "lifecycleDefineStandardJson",
  "lifecycleDefineJson",
  "lifecycleInstantiateJson",
  "lifecycleTransitionJson",
  "refsReconcileJson",
  "applyCbor",
  "meetingsImportSnapshot",
  "sqlExecResult",
]);

const expectedRustNames = inventory.map((entry) => entry.traitCall);
const actualRustNames = [
  ...extractRustWrappers("src/lifecycle_refs.rs"),
  ...extractRustWrappers("src/exec_generated.rs"),
  ...extractRustWrappers("src/meetings.rs").filter((name) => name !== "meetings_source_read"),
  ...extractRustWrappers("src/sql_generated.rs"),
];
assert.equal(actualRustNames.length, inventory.length, "generated-group raw Rust wrapper count");
assert.equal(new Set(actualRustNames).size, inventory.length, "generated-group unique Rust wrapper count");
assert.deepEqual(actualRustNames, expectedRustNames, "generated-group Rust wrapper sequence");

const expectedPublicNames = actualRustNames.map(snakeToCamel).sort();
assert.deepEqual(expectedPublicNames, inventory.map((entry) => entry.exportName).sort());

const actualRuntimePublicNames = Object.keys(loom).filter(isGeneratedGroupPublicName).sort();
assert.equal(actualRuntimePublicNames.length, inventory.length, "Node runtime generated-group count");
assert.deepEqual(actualRuntimePublicNames, expectedPublicNames, "Node runtime generated-group set");

const actualIndexPublicNames = sortedUnique(
  [...indexJs.matchAll(/module\.exports\.([A-Za-z0-9_]+) = nativeBinding\.\1/g)]
    .map((match) => match[1])
    .filter(isGeneratedGroupPublicName),
);
assert.equal(actualIndexPublicNames.length, inventory.length, "Node index.js generated-group count");
assert.deepEqual(actualIndexPublicNames, expectedPublicNames, "Node index.js generated-group set");

const actualDeclarationNames = sortedUnique(
  [...dts.matchAll(/export declare function ([A-Za-z0-9_]+)\(/g)]
    .map((match) => match[1])
    .filter(isGeneratedGroupPublicName),
);
assert.equal(actualDeclarationNames.length, inventory.length, "Node declaration generated-group count");
assert.deepEqual(actualDeclarationNames, expectedPublicNames, "Node declaration generated-group set");

for (const entry of inventory) {
  assert.equal(typeof loom[entry.exportName], "function", `${entry.exportName} runtime export`);
  assert.ok(indexJs.includes(`module.exports.${entry.exportName} = nativeBinding.${entry.exportName}`));
  assert.ok(dts.includes(entry.ts), `${entry.exportName} declaration and result kind`);
  assert.ok(compactIdl(idl).includes(compactIdl(entry.idl)), `${entry.exportName} IDL argument order`);
  assert.ok(generatedApi.includes(`Generated binding for \`${entry.owner.replace("Generated", "")}.${entry.traitCall}\``));
  const source = readFileSync(join(here, entry.source), "utf8");
  assert.ok(source.includes(`as ${entry.owner}`), `${entry.exportName} generated trait owner`);
  assert.ok(source.includes(`generated_session::open_generated_session`), `${entry.exportName} shared helper`);
  assert.ok(source.includes(`as ${entry.owner}>::${entry.traitCall}`), `${entry.exportName} trait call`);
  assert.equal(source.includes("LocalLoomClient::new"), false, `${entry.exportName} has no duplicate opener`);
  assert.equal(source.includes(".close("), false, `${entry.exportName} has no manual close`);
}

for (const binding of ["node", "python"]) {
  const generatedSession = readFileSync(join(root, `bindings/${binding}/src/generated_session.rs`), "utf8");
  assert.ok(generatedSession.includes("pub(crate) fn open_generated_session"));
  assert.ok(generatedSession.includes("authenticate_passphrase"));
  assert.ok(generatedSession.includes("impl Drop for GeneratedSession"));

  const securityAdmin = readFileSync(join(root, `bindings/${binding}/src/security_admin.rs`), "utf8");
  assert.ok(securityAdmin.includes("generated_session::open_generated_session"));
  assert.equal(securityAdmin.includes("LocalLoomClient::new"), false);
  assert.equal(securityAdmin.includes(".close("), false);
  assert.equal(securityAdmin.includes("authenticate_passphrase"), false);
}
