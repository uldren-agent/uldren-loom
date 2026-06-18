import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey, LoomStatement } from '../internal';

export async function sqlReadTableCbor(
  loomPath: string,
  workspace: string,
  table: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.sqlReadTable(loomPath, workspace, table, passphrase, kek, authPrincipal, authPassphrase);
  return Uint8Array.from(bytes);
}

export async function sqlReadTableAtCbor(
  loomPath: string,
  workspace: string,
  table: string,
  commit: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.sqlReadTableAt(
    loomPath,
    workspace,
    table,
    commit,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

export async function sqlIndexScanCbor(
  loomPath: string,
  workspace: string,
  table: string,
  index: string,
  prefix: Uint8Array | number[],
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.sqlIndexScan(
    loomPath,
    workspace,
    table,
    index,
    Array.from(prefix),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

export async function sqlIndexScanAtCbor(
  loomPath: string,
  workspace: string,
  table: string,
  index: string,
  prefix: Uint8Array | number[],
  commit: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.sqlIndexScanAt(
    loomPath,
    workspace,
    table,
    index,
    Array.from(prefix),
    commit,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

export async function sqlBlameCbor(
  loomPath: string,
  workspace: string,
  branch: string,
  table: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.sqlBlame(loomPath, workspace, branch, table, passphrase, kek, authPrincipal, authPassphrase);
  return Uint8Array.from(bytes);
}

export async function sqlDiffCbor(
  loomPath: string,
  workspace: string,
  table: string,
  fromCommit: string,
  toCommit: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.sqlDiff(
    loomPath,
    workspace,
    table,
    fromCommit,
    toCommit,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

export async function sqlTableDiffCbor(
  loomPath: string,
  workspace: string,
  table: string,
  fromCommit: string,
  toCommit: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.sqlTableDiff(
    loomPath,
    workspace,
    table,
    fromCommit,
    toCommit,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

/**
 * Run write-capable SQL against workspace `ns`'s SQL facet (database `db`) in the `.loom` at
 * `loomPath`. Resolves **typed** results: an array of statement objects with idiomatic, lossless
 * cells. The native layer returns lossless bridge JSON (decoded once in Rust) and this parses it - so
 * no CBOR is decoded in JS. For the raw canonical bytes use {@link sqlExecBytes}; for the JSON debug
 * form use {@link sqlExecJson}; for read-only row streaming use {@link sqlQueryBytes}.
 */
export async function sqlExec(
  loomPath: string,
  ns: string,
  db: string,
  sql: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<LoomStatement[]> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return JSON.parse(
    await UldrenLoom.sqlExecTyped(loomPath, ns, db, sql, passphrase, kek, authPrincipal, authPassphrase)
  ) as LoomStatement[];
}

/**
 * Run a list of statements as one **atomic transaction/batch** in a single native round-trip
 * the native layer opens a held-open batch, runs each statement in order (including
 * `BEGIN`/`COMMIT`/`ROLLBACK`), and on success commits with one atomic save; any error aborts and
 * discards every change. The writer lock is held entirely inside native code, off the JS thread, never
 * across the bridge. Resolves the typed results of the **final** statement (e.g. a closing `SELECT`).
 *
 * This is the coarse, stateless batch API. Interactive cross-call transactions (app-code branching
 * between statements inside one transaction) are intentionally not exposed here; the core/C ABI can
 * support a held-open handle, but that needs a guarded native handle registry and a concrete consumer.
 */
export async function sqlBatch(
  loomPath: string,
  ns: string,
  db: string,
  statements: string[],
  key?: LoomKey,
  auth?: LoomAuth
): Promise<LoomStatement[]> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return JSON.parse(
    await UldrenLoom.sqlBatch(loomPath, ns, db, statements, passphrase, kek, authPrincipal, authPassphrase)
  ) as LoomStatement[];
}

/**
 * Run write-capable SQL; resolves a JSON array of the result payloads (debug/admin form, rendered
 * from canonical CBOR - not the type-faithful API; use {@link sqlExec}).
 */
export function sqlExecJson(
  loomPath: string,
  ns: string,
  db: string,
  sql: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.sqlExecJson(loomPath, ns, db, sql, passphrase, kek, authPrincipal, authPassphrase);
}

/**
 * Run write-capable SQL and resolve the result payloads as canonical-CBOR bytes - the type-faithful
 * form. Each call opens the loom, runs, and closes off the JS thread.
 */
export async function sqlExecBytes(
  loomPath: string,
  ns: string,
  db: string,
  sql: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.sqlExecBytes(loomPath, ns, db, sql, passphrase, kek, authPrincipal, authPassphrase);
  return Uint8Array.from(bytes);
}

/**
 * Run read-only SQL and resolve one canonical-CBOR row byte array per result row. Mutating statements
 * are rejected by the native query path.
 */
export async function sqlQueryBytes(
  loomPath: string,
  ns: string,
  db: string,
  sql: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array[]> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const rows = await UldrenLoom.sqlQueryBytes(loomPath, ns, db, sql, passphrase, kek, authPrincipal, authPassphrase);
  return rows.map((row) => Uint8Array.from(row));
}

/**
 * Commit the staged state of workspace `ns`'s SQL facet (database `db`) in the `.loom` at `loomPath`.
 * Resolves the new commit's content address ("algo:hex"), off the JS thread.
 */
export function sqlCommit(
  loomPath: string,
  ns: string,
  db: string,
  message: string,
  author: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.sqlCommit(loomPath, ns, db, message, author, passphrase, kek, authPrincipal, authPassphrase);
}
