import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const loom = require("./index.js");
const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..", "..");

const inventory = [
  {
    exportName: "importTableCsv",
    rustName: "import_table_csv",
    nodeParams: [
      ["loom_path", "String"],
      ["workspace", "String"],
      ["source_scope", "String"],
      ["csv_payload", "Uint8Array"],
      ["database", "String"],
      ["table", "String"],
      ["schema", "String"],
      ["primary_key", "String"],
      ["mode", "String"],
      ["commit", "bool"],
      ["author", "Option<String>"],
      ["message", "Option<String>"],
      ["dry_run", "bool"],
      ["store_passphrase", "Option<String>"],
      ["auth_principal", "Option<String>"],
      ["auth_passphrase", "Option<String>"],
    ],
    idl: "bytes import_table_csv(LoomSession handle, string workspace, string source_scope, bytes csv_payload, string database, string table, string schema, string primary_key, string mode, bool commit, optional string author, optional string message, bool dry_run);",
    ts: "export declare function importTableCsv(loomPath: string, workspace: string, sourceScope: string, csvPayload: Uint8Array, database: string, table: string, schema: string, primaryKey: string, mode: string, commit: boolean, author: string | undefined | null, message: string | undefined | null, dryRun: boolean, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): Uint8Array",
  },
  {
    exportName: "importRedmine",
    rustName: "import_redmine",
    nodeParams: "profileFieldPolicy",
    idl: "bytes import_redmine(LoomSession handle, string workspace, string profile, string source_scope, bytes snapshot_payload, string field_policy, bool dry_run);",
    ts: "export declare function importRedmine(loomPath: string, workspace: string, profile: string, sourceScope: string, snapshotPayload: Uint8Array, fieldPolicy: string, dryRun: boolean, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): Uint8Array",
  },
  {
    exportName: "importAsana",
    rustName: "import_asana",
    nodeParams: "profileFieldPolicy",
    idl: "bytes import_asana(LoomSession handle, string workspace, string profile, string source_scope, bytes snapshot_payload, string field_policy, bool dry_run);",
    ts: "export declare function importAsana(loomPath: string, workspace: string, profile: string, sourceScope: string, snapshotPayload: Uint8Array, fieldPolicy: string, dryRun: boolean, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): Uint8Array",
  },
  {
    exportName: "importJira",
    rustName: "import_jira",
    nodeParams: "profileFieldPolicy",
    idl: "bytes import_jira(LoomSession handle, string workspace, string profile, string source_scope, bytes snapshot_payload, string field_policy, bool dry_run);",
    ts: "export declare function importJira(loomPath: string, workspace: string, profile: string, sourceScope: string, snapshotPayload: Uint8Array, fieldPolicy: string, dryRun: boolean, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): Uint8Array",
  },
  {
    exportName: "importConfluence",
    rustName: "import_confluence",
    nodeParams: "profileDefaultSpace",
    idl: "bytes import_confluence(LoomSession handle, string workspace, string profile, string source_scope, bytes snapshot_payload, string default_space, bool dry_run);",
    ts: "export declare function importConfluence(loomPath: string, workspace: string, profile: string, sourceScope: string, snapshotPayload: Uint8Array, defaultSpace: string, dryRun: boolean, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): Uint8Array",
  },
  {
    exportName: "importSlack",
    rustName: "import_slack",
    nodeParams: "profilePayloadOnly",
    idl: "bytes import_slack(LoomSession handle, string workspace, string profile, string source_scope, bytes snapshot_payload, bool dry_run);",
    ts: "export declare function importSlack(loomPath: string, workspace: string, profile: string, sourceScope: string, snapshotPayload: Uint8Array, dryRun: boolean, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): Uint8Array",
  },
  {
    exportName: "importDrive",
    rustName: "import_drive",
    nodeParams: "profileArchiveOnly",
    idl: "bytes import_drive(LoomSession handle, string workspace, string profile, string source_scope, bytes archive_payload, bool dry_run);",
    ts: "export declare function importDrive(loomPath: string, workspace: string, profile: string, sourceScope: string, archivePayload: Uint8Array, dryRun: boolean, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): Uint8Array",
  },
  {
    exportName: "importMarkdown",
    rustName: "import_markdown",
    nodeParams: "profileSpace",
    idl: "bytes import_markdown(LoomSession handle, string workspace, string profile, string source_scope, bytes archive_payload, string space, bool dry_run);",
    ts: "export declare function importMarkdown(loomPath: string, workspace: string, profile: string, sourceScope: string, archivePayload: Uint8Array, space: string, dryRun: boolean, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): Uint8Array",
  },
  {
    exportName: "importNotion",
    rustName: "import_notion",
    nodeParams: "profileDefaultSpace",
    idl: "bytes import_notion(LoomSession handle, string workspace, string profile, string source_scope, bytes snapshot_payload, string default_space, bool dry_run);",
    ts: "export declare function importNotion(loomPath: string, workspace: string, profile: string, sourceScope: string, snapshotPayload: Uint8Array, defaultSpace: string, dryRun: boolean, storePassphrase?: string | undefined | null, authPrincipal?: string | undefined | null, authPassphrase?: string | undefined | null): Uint8Array",
  },
];

const nodeParamGroups = {
  profileFieldPolicy: [
    ["loom_path", "String"],
    ["workspace", "String"],
    ["profile", "String"],
    ["source_scope", "String"],
    ["snapshot_payload", "Uint8Array"],
    ["field_policy", "String"],
    ["dry_run", "bool"],
    ["store_passphrase", "Option<String>"],
    ["auth_principal", "Option<String>"],
    ["auth_passphrase", "Option<String>"],
  ],
  profileDefaultSpace: [
    ["loom_path", "String"],
    ["workspace", "String"],
    ["profile", "String"],
    ["source_scope", "String"],
    ["snapshot_payload", "Uint8Array"],
    ["default_space", "String"],
    ["dry_run", "bool"],
    ["store_passphrase", "Option<String>"],
    ["auth_principal", "Option<String>"],
    ["auth_passphrase", "Option<String>"],
  ],
  profilePayloadOnly: [
    ["loom_path", "String"],
    ["workspace", "String"],
    ["profile", "String"],
    ["source_scope", "String"],
    ["snapshot_payload", "Uint8Array"],
    ["dry_run", "bool"],
    ["store_passphrase", "Option<String>"],
    ["auth_principal", "Option<String>"],
    ["auth_passphrase", "Option<String>"],
  ],
  profileArchiveOnly: [
    ["loom_path", "String"],
    ["workspace", "String"],
    ["profile", "String"],
    ["source_scope", "String"],
    ["archive_payload", "Uint8Array"],
    ["dry_run", "bool"],
    ["store_passphrase", "Option<String>"],
    ["auth_principal", "Option<String>"],
    ["auth_passphrase", "Option<String>"],
  ],
  profileSpace: [
    ["loom_path", "String"],
    ["workspace", "String"],
    ["profile", "String"],
    ["source_scope", "String"],
    ["archive_payload", "Uint8Array"],
    ["space", "String"],
    ["dry_run", "bool"],
    ["store_passphrase", "Option<String>"],
    ["auth_principal", "Option<String>"],
    ["auth_passphrase", "Option<String>"],
  ],
};

function compactIdl(value) {
  return value.replace(/\s+/g, " ").replace(/\s*([(),;])\s*/g, "$1").trim();
}

function snakeToCamel(value) {
  return value.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
}

function extractWrappers() {
  const source = readFileSync(join(here, "src/interchange_profiles.rs"), "utf8");
  return [...source.matchAll(/#\[napi\]\s+pub fn ([a-z0-9_]+)\(/g)].map((match) => match[1]);
}

function rustSignature(source, name) {
  const start = source.indexOf(`pub fn ${name}(`);
  assert.notEqual(start, -1, `${name} wrapper exists`);
  const paramsStart = source.indexOf("(", start);
  let depth = 0;
  for (let index = paramsStart; index < source.length; index += 1) {
    if (source[index] === "(") depth += 1;
    if (source[index] === ")") depth -= 1;
    if (depth === 0) {
      const params = source
        .slice(paramsStart + 1, index)
        .split(",")
        .map((part) => part.trim())
        .filter(Boolean)
        .map((part) => {
          const [paramName, ...rest] = part.split(":");
          return [paramName.trim(), rest.join(":").trim()];
        });
      const returnType = source.slice(index + 1, source.indexOf("{", index)).trim().replace(/\s+/g, " ");
      return { params, returnType };
    }
  }
  throw new Error(`unterminated signature for ${name}`);
}

function assertStableError(fn, token) {
  assert.throws(fn, (error) => {
    assert.match(String(error.message), token);
    return true;
  });
}

function cborReadUint(bytes, state, info) {
  if (info < 24) {
    return info;
  }
  if (info === 24) {
    return bytes[state.offset++];
  }
  if (info === 25) {
    const value = (bytes[state.offset] << 8) | bytes[state.offset + 1];
    state.offset += 2;
    return value;
  }
  if (info === 26) {
    const value =
      bytes[state.offset] * 0x1000000 +
      ((bytes[state.offset + 1] << 16) | (bytes[state.offset + 2] << 8) | bytes[state.offset + 3]);
    state.offset += 4;
    return value;
  }
  throw new Error(`unsupported uint width ${info}`);
}

function cborValue(bytes, state = { offset: 0 }) {
  const head = bytes[state.offset++];
  const major = head >> 5;
  const info = head & 0x1f;
  if (major === 0) {
    return cborReadUint(bytes, state, info);
  }
  if (major === 3) {
    const len = cborReadUint(bytes, state, info);
    const text = Buffer.from(bytes.slice(state.offset, state.offset + len)).toString("utf8");
    state.offset += len;
    return text;
  }
  if (major === 4) {
    const len = cborReadUint(bytes, state, info);
    return Array.from({ length: len }, () => cborValue(bytes, state));
  }
  if (major === 7) {
    if (info === 20) return false;
    if (info === 21) return true;
    if (info === 22) return null;
  }
  throw new Error(`unsupported cbor major ${major}`);
}

function report(bytes) {
  const value = cborValue(bytes);
  return {
    profile: value[0],
    sourceScope: value[1],
    commit: value[2],
    bytesIn: value[4],
    rowsImported: value[6],
    operationsPlanned: value[8],
    operationsApplied: value[9],
    dryRun: value[10],
  };
}

function tempStore(name) {
  return join(tmpdir(), `loom-${name}-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.loom`);
}

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function u16(value) {
  const out = Buffer.alloc(2);
  out.writeUInt16LE(value);
  return out;
}

function u32(value) {
  const out = Buffer.alloc(4);
  out.writeUInt32LE(value >>> 0);
  return out;
}

function zipBytes(entries) {
  const localParts = [];
  const centralParts = [];
  let offset = 0;
  for (const [name, content] of entries) {
    const nameBytes = Buffer.from(name);
    const data = Buffer.from(content);
    const crc = crc32(data);
    const local = Buffer.concat([
      u32(0x04034b50),
      u16(20),
      u16(0),
      u16(0),
      u16(0),
      u16(0),
      u32(crc),
      u32(data.length),
      u32(data.length),
      u16(nameBytes.length),
      u16(0),
      nameBytes,
      data,
    ]);
    const central = Buffer.concat([
      u32(0x02014b50),
      u16(20),
      u16(20),
      u16(0),
      u16(0),
      u16(0),
      u16(0),
      u32(crc),
      u32(data.length),
      u32(data.length),
      u16(nameBytes.length),
      u16(0),
      u16(0),
      u16(0),
      u16(0),
      u32(0),
      u32(offset),
      nameBytes,
    ]);
    localParts.push(local);
    centralParts.push(central);
    offset += local.length;
  }
  const central = Buffer.concat(centralParts);
  const end = Buffer.concat([
    u32(0x06054b50),
    u16(0),
    u16(0),
    u16(entries.length),
    u16(entries.length),
    u32(central.length),
    u32(offset),
    u16(0),
  ]);
  return Buffer.concat([...localParts, central, end]);
}

{
  const idl = compactIdl(readFileSync(join(root, "idl/loom.idl"), "utf8"));
  const generatedApi = readFileSync(join(root, "crates/loom-remote-protocol/src/generated_api.rs"), "utf8");
  const source = readFileSync(join(here, "src/interchange_profiles.rs"), "utf8");
  const indexJs = readFileSync(join(here, "index.js"), "utf8");
  const dts = readFileSync(join(here, "index.d.ts"), "utf8");
  const rustNames = extractWrappers();
  const expectedRustNames = inventory.map((entry) => entry.rustName);
  const publicNames = rustNames.map(snakeToCamel);
  const expectedPublicNames = inventory.map((entry) => entry.exportName);

  assert.deepEqual(rustNames, expectedRustNames);
  assert.equal(rustNames.length, 9);
  assert.equal(new Set(rustNames).size, 9);
  assert.deepEqual(publicNames, expectedPublicNames);
  assert.equal(new Set(publicNames).size, 9);
  const runtimeImports = Object.keys(loom).filter((name) => /^import[A-Z]/.test(name));
  assert.equal(runtimeImports.length, expectedPublicNames.length);
  assert.deepEqual([...runtimeImports].sort(), [...expectedPublicNames].sort());
  assert.deepEqual(
    [...indexJs.matchAll(/module\.exports\.(import[A-Za-z0-9]+) = nativeBinding\.\1/g)].map((match) => match[1]),
    expectedPublicNames,
  );
  assert.deepEqual(
    [...dts.matchAll(/export declare function (import[A-Za-z0-9]+)\(/g)].map((match) => match[1]),
    expectedPublicNames,
  );

  for (const entry of inventory) {
    const signature = rustSignature(source, entry.rustName);
    const expectedParams = Array.isArray(entry.nodeParams) ? entry.nodeParams : nodeParamGroups[entry.nodeParams];
    assert.deepEqual(signature.params, expectedParams, `${entry.exportName} Rust parameter order`);
    assert.equal(signature.returnType, "-> napi::Result<Uint8Array>", `${entry.exportName} Rust result`);
    assert.equal(typeof loom[entry.exportName], "function");
    assert.ok(indexJs.includes(`module.exports.${entry.exportName} = nativeBinding.${entry.exportName}`));
    assert.ok(dts.includes(entry.ts), `${entry.exportName} declaration`);
    assert.ok(idl.includes(compactIdl(entry.idl)), `${entry.exportName} IDL order`);
    assert.ok(generatedApi.includes(`Generated binding for \`InterchangeProfiles.${entry.rustName}\``));
    assert.ok(source.includes(`as GeneratedInterchangeProfiles>::${entry.rustName}`));
  }
  assert.ok(source.includes("generated_session::open_generated_session"));
  assert.equal(source.includes("LocalLoomClient::new"), false);
  assert.equal(source.includes(".close("), false);
}

{
  const store = tempStore("node-interchange-table");
  loom.createLoom(store, "default", null, null);
  loom.workspaceCreate(store, "main", "sql", null);
  const payload = Buffer.from('id,name,note\n1,alpha,"nul\u0000byte"\n');
  const dry = loom.importTableCsv(
    store,
    "main",
    "memory://items-dry.csv",
    payload,
    "app",
    "items",
    "id:int,name:text,note:text",
    "id",
    "snapshot",
    false,
    null,
    null,
    true,
    null,
    null,
    null,
  );
  assert.ok(dry instanceof Uint8Array);
  const dryReport = report(dry);
  assert.equal(dryReport.profile, "table-csv");
  assert.equal(dryReport.sourceScope, "memory://items-dry.csv");
  assert.equal(dryReport.bytesIn, payload.length);
  assert.equal(dryReport.rowsImported, 1);
  assert.equal(dryReport.operationsApplied, 0);
  assert.equal(dryReport.dryRun, true);
  assert.equal(dryReport.commit, null);
  const firstByte = dry[0];
  dry[0] = 0;
  const fresh = loom.importTableCsv(
    store,
    "main",
    "memory://items-dry.csv",
    payload,
    "app",
    "items",
    "id:int,name:text,note:text",
    "id",
    "snapshot",
    false,
    "Author",
    "Message",
    true,
    null,
    null,
    null,
  );
  assert.equal(fresh[0], firstByte);
  assertStableError(() => loom.sqlReadTable(store, "main", ".loom/facets/sql/app/tables/items", null), /NOT_FOUND|not found|unknown|no such/i);

  const written = loom.importTableCsv(
    store,
    "main",
    "memory://items-write.csv",
    Buffer.from("id,name,note\n1,alpha,persisted\n"),
    "app",
    "items",
    "id:int,name:text,note:text",
    "id",
    "snapshot",
    true,
    "Author",
    "Message",
    false,
    null,
    null,
    null,
  );
  const writeReport = report(written);
  assert.equal(writeReport.operationsApplied, 1);
  assert.equal(writeReport.dryRun, false);
  assert.match(writeReport.commit, /^blake3:/);
  assert.ok(loom.sqlReadTable(store, "main", ".loom/facets/sql/app/tables/items", null).byteLength > 0);
  assert.deepEqual(JSON.parse(loom.statusJson(store, "sql", "main", null)).untracked, []);
}

{
  const store = tempStore("node-interchange-table-no-commit");
  loom.createLoom(store, "default", null, null);
  loom.workspaceCreate(store, "main", "sql", null);
  const written = loom.importTableCsv(
    store,
    "main",
    "memory://items-write-no-commit.csv",
    Buffer.from("id,name,note\n1,alpha,published\n"),
    "app",
    "items",
    "id:int,name:text,note:text",
    "id",
    "snapshot",
    false,
    "Author",
    "Message",
    false,
    null,
    null,
    null,
  );
  const writeReport = report(written);
  assert.equal(writeReport.commit, null);
  assert.equal(writeReport.operationsApplied, 1);
  assert.ok(loom.sqlReadTable(store, "main", ".loom/facets/sql/app/tables/items", null).byteLength > 0);
  assert.deepEqual(JSON.parse(loom.statusJson(store, "sql", "main", null)).untracked, [".loom/facets/sql/app/tables/items"]);
}

{
  const store = tempStore("node-interchange-profile-success");
  loom.createLoom(store, "default", null, null);
  loom.workspaceCreate(store, "main", null, null);
  const cases = [
    [
      "redmine",
      Buffer.from(
        '{"projects":[{"id":1,"identifier":"core","key_prefix":"CORE","name":"Core"}],"issues":[{"id":42,"project_identifier":"core","tracker":"Bug","subject":"Login fails","description":"Fails","status":"New","priority":"High","custom_fields":{"severity":"critical"}}]}',
      ),
      () => loom.importRedmine(store, "main", "redmine", "redmine://fixture", cases[0][1], "infer", true, null, null, null),
      1,
    ],
    [
      "asana",
      Buffer.from(
        '{"projects":[{"gid":"p1","key_prefix":"AS","name":"Project"}],"tasks":[{"gid":"t1","project_gid":"p1","name":"Task","notes":"Notes","resource_subtype":"default_task","completed":false,"custom_fields":{"size":"M"}}]}',
      ),
      () => loom.importAsana(store, "main", "asana", "asana://fixture", cases[1][1], "infer", true, null, null, null),
      1,
    ],
    [
      "jira",
      Buffer.from(
        '{"projects":[{"id":10001,"key":"CORE","name":"Core"}],"issues":[{"id":10042,"key":"CORE-42","project_key":"CORE","issue_type":"Bug","summary":"Login fails","description":"Fails","status":"To Do","priority":"High","custom_fields":{"severity":"critical"}}]}',
      ),
      () => loom.importJira(store, "main", "jira", "jira://fixture", cases[2][1], "infer", true, null, null, null),
      1,
    ],
    [
      "confluence",
      Buffer.from('{"pages":[{"id":"home","title":"Home","text":"Hello"}]}'),
      () => loom.importConfluence(store, "main", "confluence", "confluence://fixture", cases[3][1], "wiki", true, null, null, null),
      1,
    ],
    [
      "slack",
      Buffer.from('{"channels":[{"id":"C1","name":"general","messages":[{"ts":"1710000000.000100","user":"U1","text":"Hello"}]}]}'),
      () => loom.importSlack(store, "main", "slack", "slack://fixture", cases[4][1], true, null, null, null),
      1,
    ],
    [
      "drive",
      zipBytes([["manifest.json", '{"files":[{"id":"readme","name":"README.md","text":"Inline text"}]}']]),
      () => loom.importDrive(store, "main", "drive", "drive://fixture", cases[5][1], true, null, null, null),
      1,
    ],
    [
      "markdown",
      zipBytes([["Intro.md", "# Intro\nHello\n"]]),
      () => loom.importMarkdown(store, "main", "markdown", "markdown://fixture", cases[6][1], "docs", true, null, null, null),
      1,
    ],
    [
      "notion",
      Buffer.from('{"pages":[{"id":"intro","title":"Intro","markdown":"# Intro"}]}'),
      () => loom.importNotion(store, "main", "notion", "notion://fixture", cases[7][1], "wiki", true, null, null, null),
      1,
    ],
  ];
  for (const [expectedProfile, payload, call, minRows] of cases) {
    const out = call();
    assert.ok(out instanceof Uint8Array);
    const parsed = report(out);
    assert.equal(parsed.profile, expectedProfile);
    assert.equal(parsed.bytesIn, payload.length);
    assert.equal(parsed.dryRun, true);
    assert.equal(parsed.operationsApplied, 0);
    assert.ok(parsed.rowsImported >= minRows || parsed.operationsPlanned >= minRows, expectedProfile);
  }
  assert.equal(report(cases[3][2]()).sourceScope, "confluence://fixture");
  assert.equal(report(cases[7][2]()).sourceScope, "notion://fixture");
}

{
  const store = tempStore("node-interchange-errors");
  loom.createLoom(store, "default", null, null);
  loom.workspaceCreate(store, "main", null, null);
  const payload = Buffer.from("{ bad\u0000json");

  assertStableError(() => loom.importRedmine(store, "main", "redmine", "redmine://bad", payload, "reject", true, null, null, null), /INVALID_ARGUMENT|invalid/i);
  assertStableError(() => loom.importNotion(store, "main", "notion", "notion://bad", payload, "wiki", true, null, null, null), /INVALID_ARGUMENT|invalid/i);
  assertStableError(() => loom.importNotion(store, "main", "notion", "notion://bad", payload, "wiki", true, null, "principal-only", null), /authPrincipal and authPassphrase/);
}
