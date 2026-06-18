import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/** Record `value` at timestamp `ts` (i64 as a decimal string) in series `collection` of `workspace`. */
export function tsPut(
  loomPath: string,
  workspace: string,
  collection: string,
  ts: number | bigint | string,
  value: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.tsPut(
    loomPath,
    workspace,
    collection,
    String(ts),
    Array.from(value),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Fetch the point at timestamp `ts` (i64 as a decimal string) in series `collection`, or null if absent. */
export async function tsGet(
  loomPath: string,
  workspace: string,
  collection: string,
  ts: number | bigint | string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.tsGet(
    loomPath,
    workspace,
    collection,
    String(ts),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** The half-open range `[from, to)` of series `collection` as raw Loom Canonical CBOR `[ts, value]` pairs. */
export async function tsRangeCbor(
  loomPath: string,
  workspace: string,
  collection: string,
  from: number | bigint | string,
  to: number | bigint | string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.tsRange(
    loomPath,
    workspace,
    collection,
    String(from),
    String(to),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

/** The most recent point of series `collection` as `{ ts, value }` (ts a decimal string), or null if absent/empty. */
export async function tsLatest(
  loomPath: string,
  workspace: string,
  collection: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<{ ts: string; value: Uint8Array } | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const point = await UldrenLoom.tsLatest(
    loomPath,
    workspace,
    collection,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return point == null ? null : { ts: point.ts, value: Uint8Array.from(point.value) };
}
