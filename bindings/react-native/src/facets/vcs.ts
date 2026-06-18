import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/** Workspace/entry-level blame for `branch` (which commit last set each path) as raw Loom Canonical CBOR. */
export async function vcsBlameCbor(
  loomPath: string,
  workspace: string,
  branch: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.vcsBlame(
    loomPath,
    workspace,
    branch,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

/** Structural diff between two commits as raw LMDIFF Loom Canonical CBOR. */
export async function vcsDiffCbor(
  loomPath: string,
  workspace: string,
  fromCommit: string,
  toCommit: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.vcsDiff(
    loomPath,
    workspace,
    fromCommit,
    toCommit,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

/** Subscribe to workspace history changes and return an opaque watch cursor string. */
export async function watchSubscribe(
  loomPath: string,
  workspace: string,
  branch: string,
  facet?: string | null,
  pathPrefix?: string | null,
  changeKinds: string[] = [],
  fromCommit?: string | null,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.watchSubscribe(
    loomPath,
    workspace,
    branch,
    facet ?? '',
    pathPrefix ?? '',
    changeKinds.join(','),
    fromCommit ?? '',
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Poll an opaque watch cursor and return a canonical-CBOR `loom.watch.batch.v1` batch. */
export async function watchPollCbor(
  loomPath: string,
  cursor: string,
  max: number,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.watchPoll(
    loomPath,
    cursor,
    max,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}
