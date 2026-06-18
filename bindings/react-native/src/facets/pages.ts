import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

function args(key?: LoomKey, auth?: LoomAuth): [string, number[], string, string] {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return [passphrase, kek, authPrincipal, authPassphrase];
}

function opt(value?: string | null): string {
  return value ?? '';
}

export function spacesCreateJson(loomPath: string, workspace: string, pageWorkspaceId: string, spaceId: string, title: string, expectedRoot?: string | null, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.spacesCreateJson(loomPath, workspace, pageWorkspaceId, spaceId, title, opt(expectedRoot), ...args(key, auth));
}

export function spacesListJson(loomPath: string, workspace: string, pageWorkspaceId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.spacesListJson(loomPath, workspace, pageWorkspaceId, ...args(key, auth));
}

export function spacesGetJson(loomPath: string, workspace: string, pageWorkspaceId: string, spaceId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.spacesGetJson(loomPath, workspace, pageWorkspaceId, spaceId, ...args(key, auth));
}

export function pagesCreateJson(loomPath: string, workspace: string, pageWorkspaceId: string, pageId: string, spaceId: string, parentPageId: string | null | undefined, title: string, expectedRoot?: string | null, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.pagesCreateJson(loomPath, workspace, pageWorkspaceId, pageId, spaceId, opt(parentPageId), title, opt(expectedRoot), ...args(key, auth));
}

export function pagesUpdateJson(loomPath: string, workspace: string, pageWorkspaceId: string, pageId: string, bodyText: string, expectedRoot?: string | null, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.pagesUpdateJson(loomPath, workspace, pageWorkspaceId, pageId, bodyText, opt(expectedRoot), ...args(key, auth));
}

export function pagesPublishJson(loomPath: string, workspace: string, pageWorkspaceId: string, pageId: string, expectedRoot?: string | null, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.pagesPublishJson(loomPath, workspace, pageWorkspaceId, pageId, opt(expectedRoot), ...args(key, auth));
}

export function pagesGetJson(loomPath: string, workspace: string, pageWorkspaceId: string, pageId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.pagesGetJson(loomPath, workspace, pageWorkspaceId, pageId, ...args(key, auth));
}

export function pagesListJson(loomPath: string, workspace: string, pageWorkspaceId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.pagesListJson(loomPath, workspace, pageWorkspaceId, ...args(key, auth));
}

export function pagesHistoryJson(loomPath: string, workspace: string, pageWorkspaceId: string, pageId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.pagesHistoryJson(loomPath, workspace, pageWorkspaceId, pageId, ...args(key, auth));
}

export function structuresCreateJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, spaceId: string, kind: string, title: string, expectedRoot?: string | null, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.structuresCreateJson(loomPath, workspace, pageWorkspaceId, structureId, spaceId, kind, title, opt(expectedRoot), ...args(key, auth));
}

export function structuresAddNodeJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, nodeId: string, kind: string, label: string, bodyDigest?: string | null, entityRef?: string | null, expectedRoot?: string | null, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.structuresAddNodeJson(loomPath, workspace, pageWorkspaceId, structureId, nodeId, kind, label, opt(bodyDigest), opt(entityRef), opt(expectedRoot), ...args(key, auth));
}

export function structuresUpdateNodeJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, nodeId: string, kind: string, label: string, bodyDigest?: string | null, entityRef?: string | null, expectedRoot?: string | null, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.structuresUpdateNodeJson(loomPath, workspace, pageWorkspaceId, structureId, nodeId, kind, label, opt(bodyDigest), opt(entityRef), opt(expectedRoot), ...args(key, auth));
}

export function structuresBindJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, nodeId: string, entityRef?: string | null, expectedRoot?: string | null, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.structuresBindJson(loomPath, workspace, pageWorkspaceId, structureId, nodeId, opt(entityRef), opt(expectedRoot), ...args(key, auth));
}

export function structuresMoveNodeJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, nodeId: string, parentNodeId?: string | null, label?: string | null, expectedRoot?: string | null, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.structuresMoveNodeJson(loomPath, workspace, pageWorkspaceId, structureId, nodeId, opt(parentNodeId), opt(label), opt(expectedRoot), ...args(key, auth));
}

export function structuresLinkNodeJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, edgeId: string, srcNodeId: string, dstNodeId: string, label: string, targetRef?: string | null, expectedRoot?: string | null, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.structuresLinkNodeJson(loomPath, workspace, pageWorkspaceId, structureId, edgeId, srcNodeId, dstNodeId, label, opt(targetRef), opt(expectedRoot), ...args(key, auth));
}

export function structuresDecomposeToTicketsJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, itemsJson: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.structuresDecomposeToTicketsJson(loomPath, workspace, pageWorkspaceId, structureId, itemsJson, ...args(key, auth));
}

export function structuresGetJson(loomPath: string, workspace: string, pageWorkspaceId: string, structureId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.structuresGetJson(loomPath, workspace, pageWorkspaceId, structureId, ...args(key, auth));
}

export function structuresListJson(loomPath: string, workspace: string, pageWorkspaceId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.structuresListJson(loomPath, workspace, pageWorkspaceId, ...args(key, auth));
}
