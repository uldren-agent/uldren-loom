// SharedWorker COORDINATOR / relay - NOT the engine.
//
// The OPFS engine cannot live here: createSyncAccessHandle is unavailable in a SharedWorker. So this
// worker only (a) elects exactly one HOST tab per source (a .loom file), (b) relays read/write ops
// from any tab to that source's host and routes the result back, and (c) watches host liveness via a
// heartbeat so a crashed host is replaced promptly. The host runs ops on its own dedicated engine
// worker. All tabs on the same source share one engine = concurrent read+write without corruption;
// tabs on different sources are independent (the OPFS write lock is per FILE, so one host per file).
//
// Classic (non-module) worker on purpose: module SharedWorkers are Chromium-only, and this file needs
// no imports - it just shuffles JSON messages.
//
// Tab -> coordinator:
//   { type:"attach", source }                              -> { type:"role", source, role }
//   { type:"ping" }                                        (heartbeat; keeps this client "alive")
//   { type:"op", source, reqId, op, args }                 (client asks; relayed to host)
//   { type:"opResult", reqId, ok, result, error, origin }  (host answers; routed to origin)
//   { type:"reset", source }                               (any tab; relayed to host as "doReset")
//   { type:"resetDone", source }                           (host: file removed; broadcast a reload)
//   { type:"detach" }                                      (tab leaving; promote a new host if needed)
// coordinator -> tab:
//   { type:"role", source, role:"host"|"client" }
//   { type:"becomeHost", source }   { type:"hostOp", source, reqId, op, args, origin }
//   { type:"result", reqId, ok, result, error }
//   { type:"doReset", source }      { type:"reload", source }

const PING_TIMEOUT_MS = 8000; // a client unheard-from this long is treated as gone
const SWEEP_MS = 3000;

let nextClientId = 1;
const ports = new Map(); // clientId -> MessagePort
const sourceByClient = new Map(); // clientId -> source it is attached to
const hostBySource = new Map(); // source -> clientId currently hosting it
const lastSeen = new Map(); // clientId -> last message timestamp (heartbeat)

function send(clientId, msg) {
  const p = ports.get(clientId);
  if (p) p.postMessage(msg);
}

function broadcast(source, msg) {
  for (const [cid, src] of sourceByClient) if (src === source) send(cid, msg);
}

// Pick any still-attached client of `source` (other than `exclude`) to become the next host.
function pickHost(source, exclude) {
  for (const [cid, src] of sourceByClient) if (src === source && cid !== exclude) return cid;
  return null;
}

function leave(clientId) {
  const source = sourceByClient.get(clientId);
  sourceByClient.delete(clientId);
  ports.delete(clientId);
  lastSeen.delete(clientId);
  if (source !== undefined && hostBySource.get(source) === clientId) {
    hostBySource.delete(source);
    const next = pickHost(source, clientId);
    if (next != null) {
      hostBySource.set(source, next);
      send(next, { type: "becomeHost", source }); // promote a survivor; it re-acquires the freed handle
    }
  }
}

// Heartbeat sweep: only evict a stale HOST (a crashed/killed host tab that never sent "detach"),
// which makes leave() promote a replacement. We deliberately do NOT evict stale non-host clients:
// they hold no handle, so a lingering entry is harmless, and evicting one would orphan its port (its
// later op results would be dropped). Host liveness is the only thing failover depends on.
//
// Caveat: background tabs throttle timers (~1/s for the first 5 min, then ~1/min), so a host
// backgrounded for many minutes can look dead and be failed over. That only risks a transient error
// (the new host retries acquiring the handle and reports cleanly if the old one still holds it) - never
// corruption. navigator.locks would distinguish "dead" from "throttled" precisely if needed later.
setInterval(() => {
  const now = Date.now();
  for (const [source, hostId] of [...hostBySource]) {
    if (now - (lastSeen.get(hostId) ?? now) > PING_TIMEOUT_MS) leave(hostId); // promotes a survivor
  }
}, SWEEP_MS);

self.onconnect = (e) => {
  const port = e.ports[0];
  const clientId = nextClientId++;
  ports.set(clientId, port);
  lastSeen.set(clientId, Date.now());
  port.start();

  port.onmessage = (ev) => {
    lastSeen.set(clientId, Date.now()); // any message counts as liveness
    const m = ev.data;
    switch (m.type) {
      case "ping":
        break; // liveness already recorded above
      case "attach": {
        sourceByClient.set(clientId, m.source);
        const role = hostBySource.has(m.source) ? "client" : "host";
        if (role === "host") hostBySource.set(m.source, clientId);
        send(clientId, { type: "role", source: m.source, role });
        break;
      }
      case "op": {
        const host = hostBySource.get(m.source);
        if (host == null) send(clientId, { type: "result", reqId: m.reqId, ok: false, error: "no host for source (reattach)" });
        else send(host, { type: "hostOp", source: m.source, reqId: m.reqId, op: m.op, args: m.args, origin: clientId });
        break;
      }
      case "opResult":
        send(m.origin, { type: "result", reqId: m.reqId, ok: m.ok, result: m.result, error: m.error });
        break;
      case "reset": {
        const host = hostBySource.get(m.source);
        if (host == null) send(clientId, { type: "reload", source: m.source }); // nothing open; just refresh
        else send(host, { type: "doReset", source: m.source });
        break;
      }
      case "resetDone":
        broadcast(m.source, { type: "reload", source: m.source }); // host removed the file; everyone reloads
        break;
      case "detach":
        leave(clientId);
        break;
    }
  };
};
