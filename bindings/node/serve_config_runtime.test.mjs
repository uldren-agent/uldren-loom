import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const loom = require("./index.js");

for (const name of [
  "serveListenerConfigureJson",
  "serveListenerListJson",
  "serveListenerSetEnabledJson",
  "serveListenerRemoveJson",
  "serveWebRouteListJson",
  "serveWebRouteSetJson",
  "serveWebRouteRemoveJson",
]) {
  assert.equal(typeof loom[name], "function", `${name} is exported`);
}

const path = join(mkdtempSync(join(tmpdir(), "loom-serve-config-")), "serve.loom");
loom.createLoom(path, "default", null, null);

const initial = JSON.parse(loom.serveListenerListJson(path));
assert.deepEqual(initial.listeners, []);

const listener = JSON.parse(
  loom.serveListenerConfigureJson(
    path,
    JSON.stringify({
      surface: "admin",
      selectors: [],
      bind: "127.0.0.1:18083",
      transport: "rest",
      enabled: true,
      auth_mode: "owner-or-passphrase",
      exposure: "read-write",
      audit_mode: "management-and-security",
      request_size_limit: 4096,
      idle_timeout_ms: 1000,
      session_timeout_ms: 2000,
    }),
  ),
);
assert.equal(listener.surface, "admin");
assert.equal(listener.enabled, true);
assert.equal(listener.limits.request_size_limit, 4096);

const disabled = JSON.parse(loom.serveListenerSetEnabledJson(path, listener.id, false));
assert.equal(disabled.enabled, false);
const enabled = JSON.parse(loom.serveListenerSetEnabledJson(path, listener.id, true));
assert.equal(enabled.enabled, true);

const listed = JSON.parse(loom.serveListenerListJson(path));
assert.deepEqual(
  listed.listeners.map((item) => item.id),
  [listener.id],
);

const workspace = loom.workspaceCreate(path, "web-root", "files", null);
const webListener = JSON.parse(
  loom.serveListenerConfigureJson(
    path,
    JSON.stringify({
      surface: "web",
      selectors: ["web-root"],
      bind: "127.0.0.1:18084",
      transport: "rest",
      enabled: true,
    }),
  ),
);
assert.equal(webListener.surface, "web");

const route = JSON.parse(
  loom.serveWebRouteSetJson(
    path,
    JSON.stringify({
      listener: webListener.id,
      route: "docs",
      prefix: "docs",
      workspace: "web-root",
      root: "/",
    }),
  ),
);
assert.equal(route.listener, webListener.id);
assert.equal(route.default_workspace, workspace);
assert.deepEqual(
  route.routes.map((item) => item.route_id),
  ["docs"],
);
assert.equal(route.routes[0].path_prefix, "/docs");

const routeList = JSON.parse(loom.serveWebRouteListJson(path, webListener.id));
assert.deepEqual(routeList.routes, route.routes);

const routeRemoved = JSON.parse(loom.serveWebRouteRemoveJson(path, webListener.id, "docs"));
assert.deepEqual(routeRemoved.routes, []);

const removed = JSON.parse(loom.serveListenerRemoveJson(path, listener.id));
assert.equal(removed.id, listener.id);
const reopenedList = JSON.parse(loom.serveListenerListJson(path));
assert.deepEqual(
  reopenedList.listeners.map((item) => item.id),
  [webListener.id],
);
