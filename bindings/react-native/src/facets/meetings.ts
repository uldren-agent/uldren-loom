import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

export function meetingsImportSnapshot(
  loomPath: string,
  workspace: string,
  inputProfile: string,
  snapshot: Uint8Array | number[],
  dryRun = false,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.meetingsImportSnapshot(
    loomPath,
    workspace,
    inputProfile,
    Array.from(snapshot),
    dryRun,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export async function meetingsSourceRead(
  loomPath: string,
  workspace: string,
  sourceId: string,
  leaf: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.meetingsSourceRead(
    loomPath,
    workspace,
    sourceId,
    leaf,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}
