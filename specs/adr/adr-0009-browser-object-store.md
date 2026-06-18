# ADR-0009 - Browser ObjectStore: OPFS-sync-in-a-Worker, hard-fail without OPFS

**Status:** Accepted · **Date:** 2026-06-19 · **Deciders:** Nas (+ Loom maintainers)

## Context

The wasm SQL binding (#194) needs **persistent** browser storage so a `.loom` survives page reloads.
The blocker is an impedance mismatch:

- The engine's storage trait (`ObjectStore`, and `loom-store::FileStore` / the storage-v2 layer:
  superblock, bobbins, selvedge journal, freemap, CoW object index) is **synchronous**, and the SQL
  path runs it under `block_on`.
- `wasm32` has **no filesystem**, and `FileStore` is native-only.
- The two browser persistence primitives differ sharply:
  - **OPFS `FileSystemSyncAccessHandle`** - genuinely *synchronous* read/write/getSize/truncate/flush,
    but usable **only inside a Web Worker**. This is the primitive SQLite-WASM's `opfs-sahpool` VFS
    uses. Supported in all current evergreen browsers (Chrome/Edge, Firefox 111+, Safari 16.4+).
  - **IndexedDB** - **async-only**. Driving it from the synchronous engine requires either whole-module
    Asyncify (large size/perf cost) or `SharedArrayBuffer` + `Atomics.wait`, which in turn requires the
    page to be **cross-origin isolated** (COOP/COEP headers) - a real deployment burden.

This ADR pins the browser ObjectStore approach (the `#197` umbrella; spec 0160 specifies the store).

## Decision

1. **Primary backend: OPFS `FileSystemSyncAccessHandle`, with the wasm engine running inside a
   dedicated Web Worker.** The worker uses synchronous OPFS handles, so the existing synchronous
   storage-v2 logic runs unchanged behind the backing-IO trait (#200). The main thread talks to the
   worker via async `postMessage`; therefore the **JS-facing API is async** (`open`/`exec`/`commit`
   return Promises) - idiomatic for the web and acceptable.

2. **No `SharedArrayBuffer`, no COOP/COEP.** Because the engine lives in the worker and the bridge is
   async `postMessage` (not a synchronous SAB call from the main thread), cross-origin isolation is
   **not required**. This deliberately avoids the COOP/COEP deployment burden and keeps the binding
   drop-in for any host page.

3. **Hard-fail when OPFS is unavailable.** If OPFS sync-access-handles are not available (old/edge
   contexts), the binding **returns a clear `UNSUPPORTED` error** ("persistent browser storage
   requires OPFS") rather than silently degrading. We do **not** add an in-memory fallback (it would
   silently lose data on reload - a worse failure mode than an explicit error) and we do **not** add an
   IndexedDB fallback (it would force the SAB/COOP-COEP complexity this ADR exists to avoid). For the
   rare ephemeral/compute use case, a caller can still construct an explicit in-memory `MemoryStore`
   session; that is an opt-in choice, never an automatic downgrade of a "persistent" request.

## Consequences

- The wasm SQL surface is **async** (unlike the synchronous native C ABI and the node/python sessions).
  That asymmetry is inherent to the browser and is documented at the binding boundary.
- The native storage-v2 format is reused verbatim over OPFS via the backing-IO trait (#200), so a
  browser-written `.loom` is **byte-identical** to a native one and passes the same conformance digest
  vectors (#203) - the browser store is a true backend of one format, not a parallel implementation.
- Persistence is available on every current evergreen browser. Contexts without OPFS get a clear error,
  not data loss.
- If a concrete consumer ever requires persistence without OPFS, revisit with the IndexedDB + SAB path
  as a separately-justified effort (its own ADR), accepting the COOP/COEP requirement then.

## Implementation (the #197 chain)

- #200 - extract a `FileStore` backing-IO trait (`read_at`/`write_at`/`len`/`truncate`/`flush`/`lock`)
  so storage-v2 runs over `std::fs` (native) or OPFS (browser).
- #201 - implement the OPFS sync-access-handle backend of that trait (wasm32); the single-writer lock
  maps to exclusive sync-handle acquisition.
- #202 - Web Worker + wasm-bindgen harness exposing the async SQL session to the main thread.
- #203 - in-browser conformance: the pinned digest vectors over OPFS must equal native.
- #194 - the wasm `LoomSql` binding sits on top, async, hard-failing when OPFS is absent.
