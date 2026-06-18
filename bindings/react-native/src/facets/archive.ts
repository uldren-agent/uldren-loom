import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

export async function fsImport(
  loomPath: string,
  workspace: string,
  srcPath: string,
  commit = false,
  dryRun = false,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.fsImport(
    loomPath, workspace, srcPath, commit, dryRun, passphrase, kek, authPrincipal, authPassphrase
  );
  return Uint8Array.from(bytes);
}

export async function fsExport(
  loomPath: string,
  workspace: string,
  dstPath: string,
  revision?: string | null,
  dryRun = false,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.fsExport(
    loomPath, workspace, dstPath, revision ?? '', dryRun, passphrase, kek, authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

export async function archiveImport(
  loomPath: string,
  workspace: string,
  srcPath: string,
  kind: string,
  dryRun = false,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.archiveImport(
    loomPath, workspace, srcPath, kind, dryRun, passphrase, kek, authPrincipal, authPassphrase
  );
  return Uint8Array.from(bytes);
}

export async function archiveExport(
  loomPath: string,
  workspace: string,
  dstPath: string,
  kind: string,
  revision?: string | null,
  dryRun = false,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.archiveExport(
    loomPath, workspace, dstPath, kind, revision ?? '', dryRun, passphrase, kek, authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

export async function carImport(
  loomPath: string,
  srcPath: string,
  dryRun = false,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.carImport(
    loomPath, srcPath, dryRun, passphrase, kek, authPrincipal, authPassphrase
  );
  return Uint8Array.from(bytes);
}

export async function carExport(
  loomPath: string,
  workspace: string,
  dstPath: string,
  dryRun = false,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.carExport(
    loomPath, workspace, dstPath, dryRun, passphrase, kek, authPrincipal, authPassphrase
  );
  return Uint8Array.from(bytes);
}
