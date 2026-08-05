import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "..", "..");

const inventory = [
  ["serveListenerConfigureJson", "serve_listener_configure_json", "requestJson: string"],
  ["serveListenerListJson", "serve_listener_list_json", ""],
  ["serveListenerSetEnabledJson", "serve_listener_set_enabled_json", "listenerId: string, enabled: boolean"],
  ["serveListenerRemoveJson", "serve_listener_remove_json", "listenerId: string"],
  ["serveWebRouteListJson", "serve_web_route_list_json", "listenerId: string"],
  ["serveWebRouteSetJson", "serve_web_route_set_json", "requestJson: string"],
  ["serveWebRouteRemoveJson", "serve_web_route_remove_json", "listenerId: string, routeId: string"],
];

function compact(value) {
  return value.replace(/\s+/g, " ").replace(/\s*([(),;])\s*/g, "$1").trim();
}

function wrappers(source) {
  return [...source.matchAll(/#\[napi\]\s+pub fn ([a-z0-9_]+)\(([\s\S]*?)\) -> napi::Result<String>/g)].map(
    (match) => [match[1], compact(match[2])],
  );
}

const source = readFileSync(join(here, "src/serve_config.rs"), "utf8");
const idl = compact(readFileSync(join(root, "idl/loom.idl"), "utf8"));
const generated = readFileSync(join(root, "crates/loom-remote-protocol/src/generated_api.rs"), "utf8");
const indexJs = readFileSync(join(here, "index.js"), "utf8");
const dts = readFileSync(join(here, "index.d.ts"), "utf8");

assert.deepEqual(
  wrappers(source).map(([name]) => name),
  inventory.map(([, rustName]) => rustName),
);
assert.equal(new Set(wrappers(source).map(([name]) => name)).size, 7);

assert.deepEqual(
  [...indexJs.matchAll(/module\.exports\.([A-Za-z0-9]+) = nativeBinding\.\1/g)]
    .map((match) => match[1])
    .filter((name) => inventory.some((entry) => entry[0] === name)),
  inventory.map(([publicName]) => publicName),
);

assert.deepEqual(
  [...dts.matchAll(/export declare function ([A-Za-z0-9]+)\(([^)]*)\): string/g)]
    .filter((match) => inventory.some((entry) => entry[0] === match[1]))
    .map((match) => [match[1], match[2]]),
  inventory.map(([publicName, , idlArgs]) => [
    publicName,
    ["loomPath: string", idlArgs, "storePassphrase?: string | undefined | null", "authPrincipal?: string | undefined | null", "authPassphrase?: string | undefined | null"]
      .filter(Boolean)
      .join(", "),
  ]),
);

for (const [, rustName] of inventory) {
  assert.ok(source.includes(`>::${rustName}`), rustName);
  assert.ok(source.includes("generated_session::open_generated_session"), rustName);
  assert.ok(generated.includes(`Generated binding for \`ServeConfig.${rustName}\``), rustName);
}

for (const signature of [
  "string serve_listener_configure_json(LoomSession handle,string request_json);",
  "string serve_listener_list_json(LoomSession handle);",
  "string serve_listener_set_enabled_json(LoomSession handle,string listener_id,bool enabled);",
  "string serve_listener_remove_json(LoomSession handle,string listener_id);",
  "string serve_web_route_list_json(LoomSession handle,string listener_id);",
  "string serve_web_route_set_json(LoomSession handle,string request_json);",
  "string serve_web_route_remove_json(LoomSession handle,string listener_id,string route_id);",
]) {
  assert.ok(idl.includes(compact(signature)), signature);
}

assert.equal(source.includes("LocalLoomClient::new"), false);
assert.equal(source.includes(".close("), false);
assert.equal(source.includes("serde_json"), false);
assert.equal(source.includes(".serve_listener_"), false);
assert.equal(source.includes(".serve_web_route_"), false);
