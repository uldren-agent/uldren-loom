import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/**
 * Put `content` into workspace `workspace`'s CAS facet in the `.loom` at `loomPath`; resolves the
 * content address ("algo:hex"). Putting identical bytes is idempotent.
 */
export function casPut(
  loomPath: string,
  workspace: string,
  content: Uint8Array | number[],
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.casPut(loomPath, workspace, Array.from(content), passphrase, kek, authPrincipal, authPassphrase);
}

/** Fetch the CAS blob addressed by `digest` from `workspace`, or null when the digest is absent. */
export async function casGet(
  loomPath: string,
  workspace: string,
  digest: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.casGet(loomPath, workspace, digest, passphrase, kek, authPrincipal, authPassphrase);
  return bytes == null ? null : Uint8Array.from(bytes);
}

/** Whether a CAS blob addressed by `digest` is present in `workspace`. */
export function casHas(
  loomPath: string,
  workspace: string,
  digest: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.casHas(loomPath, workspace, digest, passphrase, kek, authPrincipal, authPassphrase);
}

/** The content addresses reachable in `workspace`'s CAS facet, as a parsed sorted string array. */
export async function casList(
  loomPath: string,
  workspace: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string[]> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return JSON.parse(
    await UldrenLoom.casListJson(loomPath, workspace, passphrase, kek, authPrincipal, authPassphrase)
  ) as string[];
}

/**
 * Drop the blob addressed by `digest` from `workspace`'s working tree (unreachable going forward);
 * resolves whether it was present. CAS stays immutable: an earlier commit that held it still restores
 * it, and bytes are reclaimed by GC once unreferenced.
 */
export function casDelete(
  loomPath: string,
  workspace: string,
  digest: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<boolean> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.casDelete(loomPath, workspace, digest, passphrase, kek, authPrincipal, authPassphrase);
}
