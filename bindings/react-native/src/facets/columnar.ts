import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/** Create columnar dataset `name` in `workspace` (created with the `columnar` facet if absent).
 * `columns` is raw Loom Canonical CBOR (an array of `[name, type_tag]`); `targetSegmentRows` is the
 * segment size (0 for the default). */
export function columnarCreate(
  loomPath: string,
  workspace: string,
  name: string,
  columns: Uint8Array | number[],
  targetSegmentRows: number,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.columnarCreate(
    loomPath,
    workspace,
    name,
    Array.from(columns),
    targetSegmentRows,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Append `row` (raw Loom Canonical CBOR, a cell array) to dataset `name`, validating arity and column
 * types. */
export function columnarAppend(
  loomPath: string,
  workspace: string,
  name: string,
  row: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.columnarAppend(
    loomPath,
    workspace,
    name,
    Array.from(row),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** All rows of dataset `name` in append order as raw Loom Canonical CBOR (an array of cell arrays). */
export async function columnarScanCbor(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.columnarScan(loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/** The `(name, type_tag)` columns of dataset `name` as raw Loom Canonical CBOR (an array of
 * `[name, type_tag]`). */
export async function columnarColumnsCbor(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.columnarColumns(loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/** The total row count of dataset `name`. */
export function columnarRows(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<number> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.columnarRows(loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase);
}

/** Compact dataset `name` at its target segment size. */
export function columnarCompact(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.columnarCompact(loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase);
}

/** Inspect dataset metadata as raw Loom Canonical CBOR. */
export async function columnarInspectCbor(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.columnarInspect(loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/** Source digest used by derived columnar projections as raw Loom Canonical CBOR text. */
export async function columnarSourceDigestCbor(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.columnarSourceDigest(loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/** Project `columns` (raw Loom Canonical CBOR, an array of text) from rows of dataset `name` matching
 * `filter` (raw Loom Canonical CBOR `[col, op, value_cell]`; empty for all), as raw Loom Canonical
 * CBOR (an array of cell arrays). */
export async function columnarSelectCbor(
  loomPath: string,
  workspace: string,
  name: string,
  columns: Uint8Array | number[],
  filter: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.columnarSelect(
      loomPath,
      workspace,
      name,
      Array.from(columns),
      Array.from(filter),
      passphrase,
      kek,
      authPrincipal,
      authPassphrase
    )
  );
}

/** Evaluate aggregate expressions from raw Loom Canonical CBOR `[[op, column?] ...]`, with optional filter. */
export async function columnarAggregateCbor(
  loomPath: string,
  workspace: string,
  name: string,
  aggregates: Uint8Array | number[],
  filter: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.columnarAggregate(
      loomPath,
      workspace,
      name,
      Array.from(aggregates),
      Array.from(filter),
      passphrase,
      kek,
      authPrincipal,
      authPassphrase
    )
  );
}
