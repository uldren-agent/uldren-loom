import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/**
 * Put `value` at the typed `key` (a Loom Canonical CBOR cell) in map `collection` of `workspace` (UUID or
 * name, created with the `kv` facet if absent). Putting the same key again replaces the value.
 */
export function kvPut(
  loomPath: string,
  workspace: string,
  collection: string,
  key: Uint8Array | number[],
  value: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.kvPut(
    loomPath,
    workspace,
    collection,
    Array.from(key),
    Array.from(value),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Fetch the value at typed `key` in map `collection` of `workspace`, or null if the key or map is absent. */
export async function kvGet(
  loomPath: string,
  workspace: string,
  collection: string,
  key: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.kvGet(
    loomPath,
    workspace,
    collection,
    Array.from(key),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** Remove the typed `key` from map `collection` of `workspace`; resolves whether it was present. */
export function kvDelete(
  loomPath: string,
  workspace: string,
  collection: string,
  key: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.kvDelete(
    loomPath,
    workspace,
    collection,
    Array.from(key),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Map `collection` of `workspace` as raw Loom Canonical CBOR (an array of `[key, value]` pairs in key order). */
export async function kvListCbor(
  loomPath: string,
  workspace: string,
  collection: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.kvList(loomPath, workspace, collection, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/** The half-open range `[lo, hi)` of map `collection` as raw Loom Canonical CBOR `[key, value]` pairs. */
export async function kvRangeCbor(
  loomPath: string,
  workspace: string,
  collection: string,
  lo: Uint8Array | number[],
  hi: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.kvRange(
    loomPath,
    workspace,
    collection,
    Array.from(lo),
    Array.from(hi),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}
