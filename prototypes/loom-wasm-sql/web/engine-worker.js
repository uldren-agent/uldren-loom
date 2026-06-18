// Dedicated worker that OWNS the OPFS engine for ONE source (.loom / ns / db).
//
// It is spawned by whichever tab is currently elected HOST for that source. The OPFS
// `createSyncAccessHandle` (the synchronous block device the engine needs) is only exposed in a
// dedicated worker - NOT in a SharedWorker or the main thread - so the engine must live here. Every
// tab's read/write op for this source ultimately runs against this single engine, which is what makes
// concurrent multi-tab read+write safe (one writer, one in-memory cache).
//
// Protocol (from the host tab): { reqId, op, args } -> { reqId, ok, result } | { reqId, ok:false, error }
//   op "open":   args = [path, ns, db]   acquire the OPFS handle + open the sql namespace (once)
//   op "exec":   args = [sql]            run statements; returns a JSON array of result payloads
//   op "commit": args = [message, author] commit the staged db; returns the commit address

import init, { LoomSql, conformance_digest_opfs, conformance_expected } from "./pkg/loom_wasm.js";

let sql = null;
const ready = init();

async function handle(op, args) {
  switch (op) {
    case "open": {
      const [path, ns, db] = args;
      if (!sql) sql = await LoomSql.open(path, ns, db); // exclusive OPFS sync handle = the writer lock
      return "";
    }
    case "exec":
      if (!sql) throw new Error("engine not open");
      return sql.exec(args[0]);
    case "commit":
      if (!sql) throw new Error("engine not open");
      return sql.commit(args[0], args[1]);
    case "conformance": {
      // Run the deterministic conformance vector over a throwaway OPFS file and report both the digest
      // computed here (wasm32) and the native-pinned expected value, for the page to compare.
      const got = await conformance_digest_opfs("__loom_conformance__.loom");
      return JSON.stringify({ got, expected: conformance_expected() });
    }
    default:
      throw new Error(`unknown op: ${op}`);
  }
}

// Process messages strictly in arrival order: chain each on the previous (and on wasm init). This
// guarantees an "open" finishes before any relayed "exec"/"commit" runs, even if they arrive together.
let chain = ready;
self.onmessage = (e) => {
  const { reqId, op, args } = e.data;
  chain = chain.then(async () => {
    try {
      const result = await handle(op, args || []);
      self.postMessage({ reqId, ok: true, result });
    } catch (err) {
      self.postMessage({ reqId, ok: false, error: String(err && err.message ? err.message : err) });
    }
  });
};
