import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

export function workspaceCreate(
  loomPath: string,
  name = '',
  facet = '',
  key?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.workspaceCreate(loomPath, name, facet, passphrase, kek, authPrincipal, authPassphrase);
}

export function workspaceListJson(loomPath: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.workspaceListJson(loomPath, passphrase, kek, authPrincipal, authPassphrase);
}

export function workspaceRename(
  loomPath: string,
  workspace: string,
  newName: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.workspaceRename(loomPath, workspace, newName, passphrase, kek, authPrincipal, authPassphrase);
}

export function workspaceDelete(
  loomPath: string,
  workspace: string,
  key?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.workspaceDelete(loomPath, workspace, passphrase, kek, authPrincipal, authPassphrase);
}
