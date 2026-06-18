// Public loader + client for the Uldren Loom multi-tab SQL engine.
//
// Hosting model (Model 1): this file and everything it pulls (engine-worker.js, coordinator-impl.js,
// pkg/loom_wasm*.wasm) are served by the VENDOR origin X (a CDN). An integrator page on origin Y does:
//
//     const { createLoomClient } = await import("https://X/loom.js");
//     const loom = createLoomClient({ source: "demo.loom", coordinatorUrl: "./loom-coordinator.js" });
//
// The workers it creates run in Y's origin (a Worker inherits the creating document's origin), so the
// OPFS data lives in Y's origin-private filesystem - isolated per integrator. X only supplies code.
//
// Two cross-origin tricks make "host as few files as possible" work for the integrator:
//   - Engine (dedicated) worker: a Worker script URL must be same-origin, so we boot a tiny same-origin
//     blob module worker whose only line is `import "https://X/engine-worker.js"`. A static module
//     import honours CORS, and the imported module's OWN relative imports (./pkg/...) resolve against X
//     - so the engine + wasm come from X while the worker runs in Y.
//   - Coordinator (SharedWorker): a SharedWorker must be same-origin to be shared across tabs and can't
//     be blob-booted (blob SharedWorkers aren't shared). So the integrator hosts ONE tiny same-origin
//     stub (loom-coordinator.js = `importScripts("https://X/coordinator-impl.js")`); cross-origin
//     importScripts honours CORS, so the relay logic still lives on X.
//
// Net: integrator Y hosts only its page + a 1-line coordinator stub; X hosts the wasm and all logic.

export function createLoomClient({
  source,
  ns = "app",
  db = "main",
  coordinatorUrl = "./loom-coordinator.js", // resolved against the integrator page (Y), must be same-origin
  onStatus = () => {},
}) {
  const X_BASE = new URL(".", import.meta.url).href; // where this module (and the rest of X) is served from
  let role = null;
  let engineWorker = null;
  const setStatus = (extra) =>
    onStatus(`source: ${source} • role: ${role ?? "connecting…"}` + (extra ? " • " + extra : ""));

  const sw = new SharedWorker(new URL(coordinatorUrl, location.href)); // Y-origin stub (or X's own coordinator)
  const port = sw.port;
  port.start();

  // --- ops we issue (host: straight to our engine; client: relayed via the coordinator) ---
  let reqSeq = 1;
  const pending = new Map();
  const settle = (id, ok, res, err) => {
    const p = pending.get(id); if (!p) return; pending.delete(id);
    ok ? p.resolve(res) : p.reject(new Error(err));
  };
  function sendOp(op, ...args) {
    if (role === "host") return callEngine(op, args);
    return new Promise((resolve, reject) => {
      const reqId = reqSeq++;
      const t = setTimeout(() => {
        if (pending.delete(reqId)) reject(new Error("timed out - the host tab may have closed; reload to re-elect a host"));
      }, 6000);
      pending.set(reqId, { resolve: (v) => { clearTimeout(t); resolve(v); }, reject: (e) => { clearTimeout(t); reject(e); } });
      port.postMessage({ type: "op", source, reqId, op, args });
    });
  }

  // --- host-side bridge to the dedicated engine worker ---
  let engineSeq = 1;
  const enginePending = new Map();
  function callEngine(op, args) {
    return new Promise((resolve, reject) => {
      const eid = engineSeq++;
      enginePending.set(eid, (ok, res, err) => (ok ? resolve(res) : reject(new Error(err))));
      engineWorker.postMessage({ reqId: eid, op, args });
    });
  }
  function spawnEngine() {
    // Same-origin (Y) blob module worker that imports X's engine module; its ./pkg imports resolve to X.
    const boot = `import ${JSON.stringify(X_BASE + "engine-worker.js")};`;
    const url = URL.createObjectURL(new Blob([boot], { type: "text/javascript" }));
    engineWorker = new Worker(url, { type: "module" });
    engineWorker.onerror = (e) => onStatus("engine worker failed: " + (e.message || e.filename || e));
    engineWorker.onmessage = (e) => {
      const { reqId, ok, result, error } = e.data;
      const h = enginePending.get(reqId); if (h) { enginePending.delete(reqId); h(ok, result, error); }
    };
  }
  async function openWithRetry(tries = 4) {
    for (let i = 0; i < tries; i++) {
      try { return await callEngine("open", [source, ns, db]); }
      catch (err) {
        const transient = /already open|createSyncAccessHandle/i.test(String(err && err.message));
        if (i < tries - 1 && transient) { await new Promise((r) => setTimeout(r, 300)); continue; } // prior host's handle not yet freed
        throw err;
      }
    }
  }
  async function becomeHost() {
    role = "host"; setStatus("starting engine…");
    if (!engineWorker) spawnEngine();
    await openWithRetry();
    setStatus("engine open (this tab is the writer host)");
  }
  async function localTeardown() {
    if (engineWorker) engineWorker.terminate(); // release the exclusive OPFS handle
    const root = await navigator.storage.getDirectory();
    await root.removeEntry(source).catch(() => {});
  }

  port.onmessage = async (e) => {
    const m = e.data;
    switch (m.type) {
      case "role":
        if (m.role === "host") { try { await becomeHost(); } catch (err) { onStatus("error: " + (err.message || err)); } }
        else { role = "client"; setStatus("client (read+write, routed through the host tab)"); }
        break;
      case "becomeHost":
        try { await becomeHost(); } catch (err) { onStatus("error: " + (err.message || err)); }
        break;
      case "result":
        settle(m.reqId, m.ok, m.result, m.error);
        break;
      case "hostOp": // I'm host: run a relayed op on my engine, return the answer
        callEngine(m.op, m.args).then(
          (result) => port.postMessage({ type: "opResult", reqId: m.reqId, ok: true, result, origin: m.origin }),
          (err) => port.postMessage({ type: "opResult", reqId: m.reqId, ok: false, error: String(err && err.message ? err.message : err), origin: m.origin })
        );
        break;
      case "doReset": // a client asked; I'm host: release + remove, then have the coordinator reload everyone
        try { await localTeardown(); port.postMessage({ type: "resetDone", source }); } catch (err) { onStatus("error: " + (err.message || err)); }
        break;
      case "reload":
        location.reload();
        break;
    }
  };

  port.postMessage({ type: "attach", source });
  // Heartbeat so the coordinator can detect a crashed host (one that never fired pagehide) and promote.
  const hb = setInterval(() => { try { port.postMessage({ type: "ping" }); } catch {} }, 2000);
  setStatus();
  addEventListener("pagehide", () => { clearInterval(hb); try { port.postMessage({ type: "detach" }); } catch {} });

  return {
    get role() { return role; },
    exec: (sql) => sendOp("exec", sql),
    commit: (message, author) => sendOp("commit", message, author),
    // run the deterministic vector over OPFS on the host engine; returns { got, expected } where
    // `got` is computed live on wasm32 and `expected` is the native pin - equal iff canonical bytes match.
    conformance: async () => JSON.parse(await sendOp("conformance")),
    // Reset from any tab: the host does it locally (robust - no dependency on the coordinator's code
    // version), and asks the coordinator to reload the other tabs; a client routes the request to host.
    async reset() {
      if (role === "host") { await localTeardown(); port.postMessage({ type: "resetDone", source }); location.reload(); }
      else port.postMessage({ type: "reset", source });
    },
  };
}
