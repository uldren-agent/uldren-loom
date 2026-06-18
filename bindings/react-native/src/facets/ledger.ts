import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/** Append `payload` to ledger `collection` of `workspace`; resolves the new entry's u64 sequence (decimal string). */
export function ledgerAppend(
  loomPath: string,
  workspace: string,
  collection: string,
  payload: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.ledgerAppend(
    loomPath,
    workspace,
    collection,
    Array.from(payload),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Fetch the payload at `seq` (u64 as a decimal string) in ledger `collection`, or null if absent. */
export async function ledgerGet(
  loomPath: string,
  workspace: string,
  collection: string,
  seq: number | bigint | string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.ledgerGet(
    loomPath,
    workspace,
    collection,
    String(seq),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** The head chain hash of ledger `collection` as "algo:hex", or null when absent or empty. */
export function ledgerHead(
  loomPath: string,
  workspace: string,
  collection: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<string | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.ledgerHead(loomPath, workspace, collection, passphrase, kek, authPrincipal, authPassphrase);
}

/** The number of entries in ledger `collection` (0 when absent), as a decimal string. */
export function ledgerLen(
  loomPath: string,
  workspace: string,
  collection: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.ledgerLen(loomPath, workspace, collection, passphrase, kek, authPrincipal, authPassphrase);
}

/** Recompute ledger `collection`'s chain and confirm every stored hash matches; rejects if the chain is broken. */
export function ledgerVerify(
  loomPath: string,
  workspace: string,
  collection: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.ledgerVerify(loomPath, workspace, collection, passphrase, kek, authPrincipal, authPassphrase);
}
