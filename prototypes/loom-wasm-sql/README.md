# loom-wasm-sql - multi-tab SQL over OPFS, vendor-hosted (Model 1)

Runs the Uldren Loom **SQL engine in the browser** over a `.loom` in **OPFS**, with **multiple tabs
reading and writing concurrently**, and demonstrates the **integration model**: the vendor origin **X**
(a CDN) hosts the wasm + all logic; an integrator origin **Y** loads it and runs it **in Y's own
origin** (so OPFS data is Y's, isolated per integrator).

## Architecture

- One tab is elected **host**; it runs the engine in a **dedicated worker** holding the single
  exclusive OPFS handle per source (the OPFS write lock is **per file**, so different sources = different
  hosts). A **SharedWorker** relays every other tab's read/write ops to the host. One engine per source
  means concurrent multi-tab read+write with no corruption. A 2s heartbeat fails a crashed host over.
- Workers execute in the **origin of the page that created them**, regardless of where the *code* came
  from. So with the integrator page on Y, the workers + OPFS are Y's - even though the bytes are from X.

```
Y page  --import https://X/loom.js-->  loom.js (runs in Y)
                                          |  spawns
   SharedWorker (Y stub -> importScripts X) <----> dedicated engine worker (blob -> import X engine) --> OPFS (Y)
```

Two cross-origin facts shape it (Worker/SharedWorker scripts must be **same-origin**):

- **Engine (dedicated) worker:** booted from a tiny same-origin **blob** whose one line is
  `import "https://X/engine-worker.js"`. The imported module's own `./pkg/...` imports resolve against
  X, so engine + wasm come from X while the worker runs in Y.
- **Coordinator (SharedWorker):** must be same-origin to be shared, so Y hosts a **one-line stub**
  (`importScripts("https://X/coordinator-impl.js")`). Cross-origin `importScripts` honours CORS.

**Net for an integrator (Y): two tiny files** - their page + `loom-coordinator.js`. Everything heavy
(wasm, engine, coordinator logic, loader) ships from X.

## What each origin hosts

- **X (`web/`)** - `loom.js` (loader/API), `engine-worker.js`, `coordinator-impl.js`, `pkg/` (wasm).
  Served with permissive CORS.
- **Y (`integrator/`)** - `index.html` (dynamic-imports `https://X/loom.js`) and `loom-coordinator.js`
  (the one-line stub).

## Run (two origins)

```bash
./run.sh                 # builds web/pkg, serves X on :8000 and Y on :8001, opens Y
# SKIP_BUILD=1 ./run.sh  # reuse web/pkg
```

- **Y (integrator):** http://localhost:8001/ - the real Model-1 demo (Loom loaded from X).
- **X (same-origin demo):** http://localhost:8000/ - the engine served and run from one origin.

Needs `wasm-pack` + `python3`. OPFS needs a secure context - `localhost` qualifies.

## What to try

1. Open **http://localhost:8001/** (Y). It becomes **host**; click **Create + insert + select**.
2. Open the same Y URL in a **second tab** - it becomes a **client**; **Select** and **Create...** both
   work, executed on the host's engine. Both tabs read+write one `.loom` that lives in **Y's** OPFS.
3. **Reset OPFS** from any tab (host resets locally; a client routes it to the host); all tabs reload.
4. Close the host tab - a client is promoted (heartbeat detects it within ~8s if no clean exit).
5. Different looms per tab: append `?source=alpha.loom` vs `?source=beta.loom`.

## Files

- `web/loom.js` - the single client implementation + loader (host/client routing, heartbeat, engine
  spawn, reset). Integrators `import` this from X.
- `web/engine-worker.js` - dedicated module worker: OPFS handle + `LoomSql` engine for one source.
- `web/coordinator-impl.js` - SharedWorker relay (host election + op routing + heartbeat + reset).
- `integrator/index.html`, `integrator/loom-coordinator.js` - the integrator's two files.
- `cors-server.py`, `run.sh` - local two-origin harness.

The Rust engine is `bindings/wasm/src/lib.rs` (`mod opfs_sql`): `OpfsBacking` over the sync handle, the
writer `LoomSql.open`, and `LoomSql.open_read` (lock-free read-only snapshot from raw bytes).
