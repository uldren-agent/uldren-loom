import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..", "..");

function snakeToCamel(name) {
  return name.replace(/_([a-z0-9])/g, (_match, value) => value.toUpperCase());
}

function manifestRows() {
  const contract = JSON.parse(readFileSync(join(root, "idl/binding-targets.json"), "utf8"));
  const idl = readFileSync(join(root, "idl/loom.idl"), "utf8");
  assert.equal(contract.schema_version, 1);
  assert.deepEqual(contract.native_targets, [
    "c_abi",
    "cpp",
    "jvm",
    "android",
    "ios",
    "react_native",
    "nodejs",
    "python",
  ]);
  assert.deepEqual(contract.wasm_capability_gated, {
    reason: "profile_unsupported",
    code: "UNSUPPORTED",
  });
  const rows = contract.methods.map((entry) => {
    const [owner, method] = entry.name.split(".");
    const signature = idlSignature(idl, owner, method);
    assert.ok(["supported", "capability_gated"].includes(entry.wasm), entry.name);
    return {
      owner,
      method,
      publicName: snakeToCamel(method),
      result: signature.startsWith("bytes ") ? "Uint8Array" : "string",
    };
  });
  assert.equal(rows.length, 80);
  assert.equal(new Set(rows.map((row) => `${row.owner}.${row.method}`)).size, 80);
  assert.equal(new Set(rows.map((row) => row.method)).size, 80);
  return rows;
}

function idlSignature(idl, owner, method) {
  const interfaceStart = idl.indexOf(`interface ${owner} {`);
  assert.notEqual(interfaceStart, -1, `${owner} IDL interface`);
  const interfaceTail = idl.slice(interfaceStart);
  const interfaceEnd = interfaceTail.indexOf("\n}");
  assert.notEqual(interfaceEnd, -1, `${owner} IDL interface close`);
  const block = interfaceTail.slice(0, interfaceEnd);
  const methodStart = block.indexOf(` ${method}(`);
  assert.notEqual(methodStart, -1, `${owner}.${method} IDL method`);
  const before = Math.max(block.lastIndexOf(";", methodStart), block.lastIndexOf("{", methodStart)) + 1;
  const after = block.indexOf(";", methodStart);
  assert.notEqual(after, -1, `${owner}.${method} IDL terminator`);
  return block.slice(before, after).replace(/\s+/g, " ").trim();
}

function rustWrappers(source) {
  return [...source.matchAll(/#\[napi\]\s+pub fn ([a-z0-9_]+)\(/g)].map((match) => match[1]);
}

function bodyFor(source, name) {
  const start = source.indexOf(`pub fn ${name}(`);
  assert.notEqual(start, -1, `${name} exists`);
  const next = source.indexOf("\n#[napi]", start + 1);
  return next === -1 ? source.slice(start) : source.slice(start, next);
}

function generatedOwnedWrappers(sources) {
  const out = [];
  for (const [path, source] of sources) {
    for (const name of rustWrappers(source)) {
      const body = bodyFor(source, name);
      if (body.includes("generated_session::open_generated_session") && body.includes(`>::${name}`)) {
        out.push([name, path, body]);
      }
    }
  }
  return out;
}

const rows = manifestRows();
const methods = rows.map((row) => row.method);
const publicNames = rows.map((row) => row.publicName);
const sourceDir = join(here, "src");
const sources = new Map(
  readdirSync(sourceDir)
    .filter((path) => path.endsWith(".rs"))
    .map((path) => [path, readFileSync(join(sourceDir, path), "utf8")]),
);
const generated = readFileSync(join(root, "crates/loom-remote-protocol/src/generated_api.rs"), "utf8");
const indexJs = readFileSync(join(here, "index.js"), "utf8");
const dts = readFileSync(join(here, "index.d.ts"), "utf8");

const wrappers = generatedOwnedWrappers(sources);
assert.deepEqual(
  wrappers.map(([name]) => name).sort(),
  [...methods].sort(),
);
assert.equal(wrappers.length, 80);
assert.equal(new Set(wrappers.map(([name]) => name)).size, 80);

for (const row of rows) {
  const matches = wrappers.filter(([name]) => name === row.method);
  assert.equal(matches.length, 1, row.method);
  const [, path, body] = matches[0];
  assert.ok(generated.includes(`Generated binding for \`${row.owner}.${row.method}\``), row.method);
  assert.ok(body.includes("generated_session::open_generated_session"), row.method);
  assert.ok(body.includes(`>::${row.method}`), row.method);
  assert.equal(body.includes("loom_chat::"), false, `${row.method} direct chat owner in ${path}`);
  assert.equal(body.includes("loom_drive::"), false, `${row.method} direct drive owner in ${path}`);
  assert.equal(body.includes("loom_lifecycle::"), false, `${row.method} direct lifecycle owner in ${path}`);
  assert.equal(body.includes("loom_reference::"), false, `${row.method} direct refs owner in ${path}`);
  assert.equal(body.includes("LoomSqlStore"), false, `${row.method} direct SQL owner in ${path}`);
  assert.equal(body.includes("open_loom"), false, `${row.method} direct open in ${path}`);
  assert.equal(body.includes("save_loom"), false, `${row.method} direct save in ${path}`);
  assert.equal(body.includes("LocalLoomClient::new"), false, `${row.method} direct client construction in ${path}`);
}

const exported = [...indexJs.matchAll(/module\.exports\.([A-Za-z0-9]+) = nativeBinding\.\1/g)]
  .map((match) => match[1])
  .filter((name) => publicNames.includes(name));
assert.equal(exported.length, 80);
assert.equal(new Set(exported).size, 80);
assert.deepEqual([...exported].sort(), [...publicNames].sort());

const declared = [...dts.matchAll(/export declare function ([A-Za-z0-9]+)\([^)]*\): (Uint8Array|string)/g)]
  .filter((match) => publicNames.includes(match[1]))
  .map((match) => [match[1], match[2]]);
assert.equal(declared.length, 80);
assert.equal(new Set(declared.map(([name]) => name)).size, 80);
assert.deepEqual(
  [...declared].sort(([left], [right]) => left.localeCompare(right)),
  rows
    .map((row) => [row.publicName, row.result])
    .sort(([left], [right]) => left.localeCompare(right)),
);

const exclusions = [];
assert.deepEqual(exclusions, []);
