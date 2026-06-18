import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

/** Create dataframe frame `name` from canonical DataframePlan CBOR. */
export function dataframeCreate(
  loomPath: string,
  workspace: string,
  name: string,
  plan: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.dataframeCreate(
    loomPath,
    workspace,
    name,
    Array.from(plan),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

/** Execute dataframe frame `name` and return canonical CBOR `[columns, rows]`. */
export async function dataframeCollectCbor(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.dataframeCollect(loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/** Execute dataframe frame `name` and return at most `rows` rows as canonical CBOR `[columns, rows]`. */
export async function dataframePreviewCbor(
  loomPath: string,
  workspace: string,
  name: string,
  rows: number,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.dataframePreview(loomPath, workspace, name, rows, passphrase, kek, authPrincipal, authPassphrase)
  );
}

/** Materialize dataframe frame `name`; returns a CAS digest when the materialization target emits one. */
export function dataframeMaterialize(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<string | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.dataframeMaterialize(loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase);
}

/** Canonical dataframe plan digest as `algo:hex`. */
export function dataframePlanDigest(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.dataframePlanDigest(loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase);
}

/** Source digests pinned in the dataframe plan as canonical CBOR array of `algo:hex` strings. */
export async function dataframeSourceDigestsCbor(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return Uint8Array.from(
    await UldrenLoom.dataframeSourceDigests(loomPath, workspace, name, passphrase, kek, authPrincipal, authPassphrase)
  );
}
