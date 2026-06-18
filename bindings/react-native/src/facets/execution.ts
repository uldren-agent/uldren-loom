import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

export async function execCbor(
  loomPath: string,
  request: Uint8Array | number[],
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.execCbor(
    loomPath,
    Array.from(request),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}
